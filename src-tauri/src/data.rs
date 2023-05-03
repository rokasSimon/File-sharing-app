use std::{collections::HashMap, path::PathBuf};

use anyhow::{Result, anyhow};
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

        self.signature.shared_peers.retain(|p| p != peer);

        for (_, file) in self.shared_files.iter_mut() {
            file.owned_peers.retain(|p| p != peer);
        }

        self.shared_files
            .retain(|_, file| file.owned_peers.len() != 0);
    }

    pub fn add_files(&mut self, files: Vec<SharedFile>, date_modified: DateTime<Utc>) -> Result<()> {
        for file in files {
            if self.shared_files.contains_key(&file.identifier) {
                return Err(anyhow!("File has already been added"));
            }

            if self.shared_files.values().any(|f| f.content_hash == file.content_hash) {
                return Err(anyhow!("File with same content has already been added"));
            }

            self.shared_files.insert(file.identifier, file);
        }

        self.signature.last_modified = date_modified;

        return Ok(())
    }

    pub fn remove_files(
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
                if !file.owned_peers.contains(&new_owner) {
                    file.owned_peers.push(new_owner.clone());
                }
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

#[cfg(tests)]
mod tests {

    use std::collections::HashMap;

    use chrono::Utc;
    use uuid::Uuid;

    use crate::peer_id::PeerId;

    use super::{ShareDirectorySignature, SharedFile, ShareDirectory};

    const HOSTNAME: &str = "test";
    const PEER_UUID: Uuid = Uuid::nil();

    fn setup() -> ShareDirectory {
        let now = Utc::now();
        let peer = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };

        let signature = ShareDirectorySignature {
            name: "test".to_string(),
            identifier: Uuid::new_v4(),
            last_modified: now,
            shared_peers: vec![peer.clone()],
        };

        let shared_file = SharedFile {
            name: "test file".to_string(),
            identifier: Uuid::nil(),
            content_hash: 0,
            last_modified: now,
            content_location: super::ContentLocation::NetworkOnly,
            owned_peers: vec![peer],
            size: 0
        };

        let shared_files = HashMap::from([
            (Uuid::nil(), shared_file)
        ]);

        ShareDirectory {
            signature,
            shared_files
        }
    }

    #[test]
    fn add_owner_should_contain_new_peer_id() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let peer_id_bytes = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let new_peer = PeerId {
            hostname: "owner".to_string(),
            uuid: Uuid::from_bytes(peer_id_bytes)
        };
        let file_ids = vec![Uuid::nil()];

        directory.add_owner(new_peer.clone(), mod_date, file_ids);

        assert!(directory.signature.last_modified == mod_date);
        assert!(directory.shared_files.get(&Uuid::nil()).unwrap().owned_peers.contains(&new_peer));
    }

    #[test]
    fn add_owner_should_not_add_twice() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let new_peer = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let file_ids = vec![Uuid::nil()];

        directory.add_owner(new_peer.clone(), mod_date, file_ids);

        assert!(directory.signature.last_modified == mod_date);
        assert_eq!(directory.shared_files.get(&Uuid::nil()).unwrap().owned_peers.len(), 1);
    }

    #[test]
    fn add_files_should_contain_new_file() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let file_id = Uuid::from_bytes([1; 16]);
        let files = vec![SharedFile {
            name: "file 1".to_string(),
            identifier: file_id,
            content_hash: 1,
            last_modified: mod_date,
            content_location: crate::data::ContentLocation::NetworkOnly,
            owned_peers: vec![myself],
            size: 1
        }];

        let result = directory.add_files(files, mod_date);

        assert!(directory.signature.last_modified == mod_date);
        assert_eq!(directory.shared_files.len(), 2);
        assert_eq!(directory.shared_files.get(&file_id).unwrap().name, "file 1");
        assert!(result.is_ok());
    }

    #[test]
    fn add_files_should_return_error_when_identifier_already_exists() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let file_id = Uuid::nil();
        let files = vec![SharedFile {
            name: "file 1".to_string(),
            identifier: file_id,
            content_hash: 1,
            last_modified: mod_date,
            content_location: crate::data::ContentLocation::NetworkOnly,
            owned_peers: vec![myself],
            size: 1
        }];

        let result = directory.add_files(files, mod_date);

        assert!(directory.signature.last_modified != mod_date);
        assert_eq!(directory.shared_files.len(), 1);
        assert!(result.is_err());
    }

    #[test]
    fn add_files_should_return_error_when_file_with_same_content_hash_exists() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let file_id = Uuid::from_bytes([1; 16]);
        let files = vec![SharedFile {
            name: "file 2".to_string(),
            identifier: file_id,
            content_hash: 0,
            last_modified: mod_date,
            content_location: crate::data::ContentLocation::NetworkOnly,
            owned_peers: vec![myself],
            size: 1
        }];

        let result = directory.add_files(files, mod_date);

        assert!(directory.signature.last_modified != mod_date);
        assert_eq!(directory.shared_files.len(), 1);
        assert!(result.is_err());
    }

    #[test]
    fn remove_files_no_files_should_remain() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let file_id = Uuid::nil();

        directory.remove_files(&myself, mod_date, vec![file_id]);

        assert!(directory.signature.last_modified == mod_date);
        assert_eq!(directory.shared_files.len(), 0);
    }

    #[test]
    fn remove_files_file_should_remain_with_fewer_owners() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let file_id = Uuid::nil();
        let new_peer = PeerId {
            hostname: "test 2".to_string(),
            uuid: Uuid::from_bytes([1; 16])
        };
        directory.shared_files.get_mut(&Uuid::nil()).unwrap().owned_peers.push(new_peer.clone());

        directory.remove_files(&myself, mod_date, vec![file_id]);

        assert!(directory.signature.last_modified == mod_date);
        assert_eq!(directory.shared_files.len(), 1);
        assert_eq!(directory.shared_files.get(&file_id).unwrap().owned_peers.get(0).unwrap(), &new_peer);
    }

    #[test]
    fn remove_peer_no_files_should_remain() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };

        directory.remove_peer(&myself, mod_date);

        assert!(directory.signature.last_modified == mod_date);
        assert_eq!(directory.shared_files.len(), 0);
    }

    #[test]
    fn remove_peer_single_file_should_remain() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let new_peer = PeerId {
            hostname: "test 2".to_owned(),
            uuid: Uuid::from_bytes([1; 16]),
        };
        directory.signature.shared_peers.push(new_peer.clone());
        directory.shared_files.get_mut(&Uuid::nil()).unwrap().owned_peers.push(new_peer.clone());

        directory.remove_peer(&myself, mod_date);

        assert!(directory.signature.last_modified == mod_date);
        assert!(directory.signature.shared_peers.contains(&new_peer));
        assert!(directory.shared_files.get(&Uuid::nil()).unwrap().owned_peers.contains(&new_peer));
        assert_eq!(directory.shared_files.len(), 1);
    }

    #[test]
    fn remove_peer_only_shared_peers_should_change() {
        let mut directory = setup();
        let mod_date = Utc::now();
        let myself = PeerId {
            hostname: HOSTNAME.to_string(),
            uuid: PEER_UUID,
        };
        let new_peer = PeerId {
            hostname: "test 2".to_owned(),
            uuid: Uuid::from_bytes([1; 16]),
        };
        directory.signature.shared_peers.push(new_peer.clone());

        directory.remove_peer(&new_peer, mod_date);

        assert!(directory.signature.last_modified == mod_date);
        assert!(directory.signature.shared_peers.contains(&myself));
        assert_eq!(directory.signature.shared_peers.len(), 1);
        assert_eq!(directory.shared_files.len(), 1);
        assert_eq!(directory.shared_files.get(&Uuid::nil()).unwrap().owned_peers.get(0).unwrap(), &myself);
    }

}