use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::peer_id::PeerId;

#[derive(Serialize, Deserialize, Debug)]
pub struct ShareDirectory {
    pub signature: ShareDirectorySignature,
    pub shared_files: HashMap<Uuid, SharedFile>,
}

impl ShareDirectory {
    pub fn remove_peer(&mut self, peer: &PeerId, date_modified: DateTime<Utc>) {
        self.signature.last_modified = date_modified;

        self.signature.shared_peers.retain(|p| {
            p != peer
        });

        for (_, file) in self.shared_files.iter_mut() {
            file.owned_peers.retain(|p| p != peer);
        }

        self.shared_files.retain(|_, file| file.owned_peers.len() != 0);
    }

    pub fn add_files(&mut self, files: Vec<SharedFile>, date_modified: DateTime<Utc>) {
        self.signature.last_modified = date_modified;

        for file in files {
            self.shared_files.insert(file.identifier, file);
        }
    }

    pub fn delete_files(
        &mut self,
        peer_id: &PeerId,
        date_modified: DateTime<Utc>,
        file_ids: Vec<Uuid>,
    ) {
        self.signature.last_modified = date_modified;

        for file_id in file_ids {
            let some_file = self.shared_files.get_mut(&file_id);

            let should_delete_fully = match some_file {
                Some(file) => {
                    file.owned_peers.retain(|peer| peer != peer_id);

                    file.owned_peers.len() == 0
                }
                None => false,
            };

            if should_delete_fully {
                self.shared_files.remove(&file_id);
            }
        }
    }

    pub fn add_owner(
        &mut self,
        new_owner: PeerId,
        date_modified: DateTime<Utc>,
        file_ids: Vec<Uuid>,
    ) {
        self.signature.last_modified = date_modified;

        for file_id in file_ids {
            let some_file = self.shared_files.get_mut(&file_id);

            if let Some(file) = some_file {
                file.owned_peers.push(new_owner.clone());
            }
        }
    }
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
#[serde(rename_all = "camelCase")]
pub struct ShareDirectorySignature {
    pub name: String,
    pub identifier: Uuid,
    pub last_modified: DateTime<Utc>,
    pub shared_peers: Vec<PeerId>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub enum ContentLocation {
    LocalPath(PathBuf),
    NetworkOnly,
}
