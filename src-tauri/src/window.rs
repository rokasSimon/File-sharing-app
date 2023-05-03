use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use std::{path::PathBuf, str::FromStr, sync::Arc};

use crate::{config::StoredConfig};

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

pub enum WindowAction {
    UpdateDirectory,
    UpdateShareDirectories,
    GetPeers,
    NewShareDirectory,
    Error,
    DownloadStarted,
    DownloadUpdate,
    DownloadCanceled,
}

impl WindowAction {
    pub fn to_string(&self) -> &'static str {
        match self {
            Self::UpdateDirectory => "UpdateDirectory",
            Self::UpdateShareDirectories => "UpdateShareDirectories",
            Self::GetPeers => "GetPeers",
            Self::NewShareDirectory => "NewShareDirectory",
            Self::Error => "Error",
            Self::DownloadStarted => "DownloadStarted",
            Self::DownloadUpdate => "DownloadUpdate",
            Self::DownloadCanceled => "DownloadCanceled",
        }
    }
}

pub trait WindowManager {
    fn send<P>(&self, action: WindowAction, payload: P) -> Result<(), tauri::Error> where P: Serialize + Clone;
}

pub struct MainWindowManager {
    pub window_label: &'static str,
    pub app_handle: AppHandle
}

impl WindowManager for MainWindowManager {
    fn send<P>(&self, action: WindowAction, payload: P) -> Result<(), tauri::Error> where P: Serialize + Clone {
        self.app_handle.emit_to(self.window_label, action.to_string(), payload)
    }
}