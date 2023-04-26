use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use tauri::Manager;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{data::{ShareDirectorySignature, ShareDirectory, ContentLocation}, peer_id::PeerId};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all="camelCase")]
pub struct ExternalSharedDirectory {
    pub signature: ShareDirectorySignature,
    pub files: Vec<ExternalSharedFile>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all="camelCase")]
pub struct ExternalSharedFile {
    pub name: String,
    pub identifier: Uuid,
    pub content_hash: u64,
    pub last_modified: DateTime<Utc>,
    pub directory_path: Option<String>,
    pub peers: Vec<String>,
    pub size: u64,
}

// fn map_to_external(directories: Vec<ShareDirectory>) -> Vec<ExternalSharedDirectory> {
//     let external_data = directories.iter().map(|sd| {
//         ExternalSharedDirectory {
//             signature: sd.signature.clone(),
//             files: sd.shared_files.values().map(|file| {
//                 ExternalSharedFile {
//                     name: file.name.clone(),
//                     identifier: file.identifier,
//                     content_hash: file.content_hash.clone(),
//                     last_modified: file.last_modified,
//                     directory_path: match file.content_location.clone() {
//                         ContentLocation::NetworkOnly => None,
//                         ContentLocation::LocalPath(path) => Some(path.to_str().expect("file is not valid UTF-8").to_string())
//                     },
//                     peers: file.owned_peers.iter().map(|peer| {
//                         peer.to_string()
//                     }).collect(),
//                     size: file.size,
//                 }
//             }).collect()
//         }
//     }).collect();

//     external_data
// }

// pub async fn window_loop<R: tauri::Runtime>(
//     mut server_receiver: mpsc::Receiver<WindowAction>,
    
// ) {
    
//     while let Some(action) = server_receiver.recv().await {
//         info!("Received action {:?}", action);
//         match action {

//             WindowAction::NewShareDirectory(dir) => {
//                 let _ = window_manager.emit_to(MAIN_WINDOW_LABEL, "NewShareDirectory", dir);
//             }

//             WindowAction::UpdateShareDirectories(directories) => {
//                 let _ = window_manager.emit_to(MAIN_WINDOW_LABEL, "UpdateShareDirectories", map_to_external(directories));
//             }

//             _ => {
//                 error!("Unhandled window action");
//             }

//         }
//     }

// }