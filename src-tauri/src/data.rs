use std::{collections::HashMap, path::{Path, PathBuf}};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::peer_id::PeerId;

#[derive(Serialize, Deserialize, Debug)]
pub struct ShareDirectory {
    pub signature: ShareDirectorySignature,
    pub shared_files: HashMap<Uuid, SharedFile>,
}

impl Clone for ShareDirectory {
    fn clone(&self) -> Self {
        let mut shared_files = HashMap::with_capacity(self.shared_files.len());
        shared_files.clone_from(&self.shared_files);

        Self {
            signature: self.signature.clone(),
            shared_files,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all="camelCase")]
pub struct ShareDirectorySignature {
    pub name: String,
    pub identifier: Uuid,
    pub last_transaction_id: Uuid,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all="camelCase")]
pub struct SharedFile {
    pub name: String,
    pub identifier: Uuid,
    pub content_hash: u64,
    pub last_modified: DateTime<Utc>,
    pub content_location: ContentLocation,
    pub owned_peers: Vec<PeerId>,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all="camelCase")]
pub enum ContentLocation {
    LocalPath(PathBuf),
    NetworkOnly,
}
