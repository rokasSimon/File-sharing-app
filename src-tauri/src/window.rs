use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::data::{PeerId, ShareDirectory, ShareDirectorySignature};

pub mod commands;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub peer: PeerId,
    pub download_id: Uuid,
    pub file_identifier: Uuid,
    pub directory_identifier: Uuid,
    pub progress: u64,
    pub file_name: String,
    pub file_path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadUpdate {
    pub progress: u64,
    pub download_id: Uuid,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCanceled {
    pub reason: String,
    pub download_id: Uuid,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadNotStarted {
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackendError {
    pub error: String,
    pub title: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum WindowResponse {
    CreateShareDirectory(String),
    GetAllShareDirectoryData(bool),
    GetPeers(bool),
    AddFiles {
        directory_identifier: String,
        file_paths: Vec<String>,
    },
    ShareDirectoryToPeers {
        directory_identifier: String,
        peers: Vec<PeerId>,
    },
    DownloadFile {
        directory_identifier: String,
        file_identifier: String,
    },
    DeleteFile {
        directory_identifier: String,
        file_identifier: String,
    },
    CancelDownload {
        peer: PeerId,
        download_identifier: String,
    },
    LeaveDirectory {
        directory_identifier: String,
    },
}

#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum WindowRequest {
    UpdateDirectory(ShareDirectory),
    UpdateShareDirectories(Vec<ShareDirectory>),
    GetPeers(Vec<PeerId>),
    NewShareDirectory(ShareDirectorySignature),
    Error(BackendError),
    DownloadStarted(Download),
    DownloadUpdate(DownloadUpdate),
    DownloadCanceled(DownloadCanceled),
}

impl WindowRequest {
    pub fn to_string(&self) -> &'static str {
        match self {
            Self::UpdateDirectory(_) => "UpdateDirectory",
            Self::UpdateShareDirectories(_) => "UpdateShareDirectories",
            Self::GetPeers(_) => "GetPeers",
            Self::NewShareDirectory(_) => "NewShareDirectory",
            Self::Error(_) => "Error",
            Self::DownloadStarted(_) => "DownloadStarted",
            Self::DownloadUpdate(_) => "DownloadUpdate",
            Self::DownloadCanceled(_) => "DownloadCanceled",
        }
    }
}

pub trait WindowManager {
    fn send(&self, action: WindowRequest) -> Result<(), tauri::Error>;
}

pub struct MainWindowManager {
    pub window_label: &'static str,
    pub app_handle: AppHandle
}

impl WindowManager for MainWindowManager {
    fn send(&self, action: WindowRequest) -> Result<(), tauri::Error> {
        self.app_handle.emit_to(self.window_label, action.to_string(), action)
    }
}