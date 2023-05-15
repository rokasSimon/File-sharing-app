use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize};
use tauri::async_runtime::Mutex;
use tokio::sync::mpsc;

use crate::config::{StoredConfig, Settings};

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

#[tauri::command]
pub async fn get_settings(
    _message: String,
    state: tauri::State<'_, Arc<StoredConfig>>,
) -> Result<Settings, String> {
    Ok(state.get_settings().await)
}

#[tauri::command]
pub async fn save_settings(
    message: Settings,
    state: tauri::State<'_, Arc<StoredConfig>>,
) -> Result<(), String> {
    info!("Received new settings {:#?}", message);

    let _ = state.set_settings(message).await;

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