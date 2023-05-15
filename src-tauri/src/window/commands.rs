use std::{path::PathBuf, sync::Arc, str::FromStr};

use serde::{Deserialize, Serialize};
use tauri::async_runtime::Mutex;
use tokio::sync::mpsc;

use crate::config::StoredConfig;

use super::{WindowResponse};

#[derive(Deserialize, Debug)]
pub struct OpenFile {
    pub file_path: PathBuf,
}

#[tauri::command]
pub async fn open_file(message: OpenFile) -> Result<(), String> {
    info!("{:?}", message);

    let result = opener::open(&message.file_path);

    if let Err(e) = result {
        return Err(format!(
            "Could not open file {}: {}",
            message.file_path.display(),
            e
        ));
    }

    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    minimize_on_close: bool,
    theme: String,
    download_directory: String,
}

#[tauri::command]
pub async fn get_settings(
    message: String,
    state: tauri::State<'_, Arc<StoredConfig>>,
) -> Result<Settings, String> {
    let config = state.app_config.lock().await;

    let download_dir = match config.download_directory.to_str() {
        None => {
            return Err("Could not load settings because download directory is invalid".to_string())
        }
        Some(dir) => dir.to_string(),
    };

    Ok(Settings {
        download_directory: download_dir,
        theme: config.theme.clone(),
        minimize_on_close: config.hide_on_close,
    })
}

#[tauri::command]
pub async fn save_settings(
    message: Settings,
    state: tauri::State<'_, Arc<StoredConfig>>,
) -> Result<(), String> {
    info!("Received new settings {:#?}", message);

    let mut config = state.app_config.lock().await;

    config.hide_on_close = message.minimize_on_close;
    config.theme = message.theme;

    let path = match PathBuf::from_str(&message.download_directory) {
        Err(e) => return Err(e.to_string()),
        Ok(path) => {
            if path.is_dir() {
                path
            } else {
                return Err("Path is not for a directory".to_string());
            }
        }
    };

    config.download_directory = path;

    Ok(())
}

pub struct Window {
    pub server: Mutex<mpsc::Sender<WindowResponse>>,
}

#[tauri::command]
pub async fn network_command(
    message: WindowResponse,
    state: tauri::State<'_, Window>,
) -> Result<(), String> {

    info!("{:?}", message);
    let sender = state.server.lock().await;

    sender.send(message).await.map_err(|e| e.to_string())
}