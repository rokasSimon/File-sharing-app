pub mod protobuf_types {
    include!(concat!(env!("OUT_DIR"), "/file_share.tcp_messages.rs"));
}

use std::{collections::HashMap, path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use protobuf_types::tcp_message;
use uuid::Uuid;

use crate::{
    data::{ContentLocation, ShareDirectory, ShareDirectorySignature, SharedFile},
    network::client_handle::DownloadError,
    peer_id::PeerId,
};

fn map_files(files: Vec<protobuf_types::SharedFile>) -> Result<Vec<SharedFile>, std::io::Error> {
    let mut mapped = vec![];

    for file in files {
        let f = file.try_into()?;

        mapped.push(f);
    }

    Ok(mapped)
}

fn map_directories(dirs: Vec<protobuf_types::ShareDirectory>) -> Result<Vec<ShareDirectory>, std::io::Error> {
    let mut mapped = vec![];

    for dir in dirs {
        let f = dir.try_into()?;

        mapped.push(f);
    }

    Ok(mapped)
}

impl TryFrom<super::TcpMessage> for protobuf_types::tcp_message::Message {
    type Error = std::io::Error;

    fn try_from(value: super::TcpMessage) -> Result<Self, Self::Error> {
        value.try_into()
    }
}

impl TryFrom<protobuf_types::tcp_message::Message> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::tcp_message::Message) -> Result<Self, Self::Error> {
        match value {
            tcp_message::Message::AddedFiles(added_files) => Ok(super::TcpMessage::AddedFiles {
                directory: added_files.directory.try_into()?,
                files: map_files(added_files.files)?,
            }),
            tcp_message::Message::DeleteFile(delete_file) => Ok(super::TcpMessage::DeleteFile {
                peer_id: delete_file.peer_id.try_into()?,
                directory: delete_file.directory.try_into()?,
                file: delete_file.file_identifier.try_into()?,
            }),
            tcp_message::Message::Signal(signal) => {
                Ok(protobuf_types::SignalType::from_i32(signal)
                    .unwrap_or_default()
                    .into())
            }
            tcp_message::Message::CancelDownload(cancel_download) => {
                Ok(super::TcpMessage::CancelDownload {
                    download_id: cancel_download.download_id.try_into()?,
                })
            }
            tcp_message::Message::DownloadError(err) => Ok(super::TcpMessage::DownloadError {
                error: protobuf_types::DownloadErrorType::from_i32(err.error)
                    .unwrap_or_default()
                    .into(),
                download_id: err.download_id.try_into()?,
            }),
            tcp_message::Message::DownloadedFile(d) => Ok(super::TcpMessage::DownloadedFile {
                peer_id: d.peer_id.try_into()?,
                directory_identifier: d.directory_identifier.try_into()?,
                file_identifier: d.file_identifier.try_into()?,
                date_modified: d.date_modified.try_into()?,
            }),
            tcp_message::Message::LeftDirectory(d) => Ok(super::TcpMessage::LeftDirectory {
                directory_identifier: d.directory_identifier.try_into()?,
                date_modified: d.date_modified.try_into()?,
            }),
            tcp_message::Message::ReceiveDirectories(d) => Ok(super::TcpMessage::ReceiveDirectories(map_directories(d.directories)?)),
            tcp_message::Message::ReceiveFileEnd(f) => Ok(super::TcpMessage::ReceiveFileEnd { download_id: f.download_id.try_into()? }),
            tcp_message::Message::ReceiveFilePart(f) => Ok(super::TcpMessage::ReceiveFilePart { download_id: f.download_id.try_into()?, data: f.data }),
            tcp_message::Message::ReceivePeerId(p) => Ok(super::TcpMessage::ReceivePeerId(p.peer_id.try_into()?)),
            tcp_message::Message::SharedDirectory(d) => Ok(super::TcpMessage::SharedDirectory(d.directory.try_into()?)),
            tcp_message::Message::StartDownload(d) => Ok(super::TcpMessage::StartDownload { download_id: d.download_id.try_into()?, file_id: d.file_id.try_into()?, dir_id: d.dir_id.try_into()? })
        }
    }
}

impl From<protobuf_types::SignalType> for super::TcpMessage {
    fn from(value: protobuf_types::SignalType) -> Self {
        match value {
            protobuf_types::SignalType::Synchronize => super::TcpMessage::Synchronize,
            protobuf_types::SignalType::RequestPeerId => super::TcpMessage::RequestPeerId,
        }
    }
}

impl From<protobuf_types::DownloadErrorType> for DownloadError {
    fn from(value: protobuf_types::DownloadErrorType) -> Self {
        match value {
            protobuf_types::DownloadErrorType::NoClientsConnected => {
                DownloadError::NoClientsConnected
            }
            protobuf_types::DownloadErrorType::Canceled => DownloadError::Canceled,
            protobuf_types::DownloadErrorType::Disconnected => DownloadError::Disconnected,
            protobuf_types::DownloadErrorType::DirectoryMissing => DownloadError::DirectoryMissing,
            protobuf_types::DownloadErrorType::FileMissing => DownloadError::FileMissing,
            protobuf_types::DownloadErrorType::FileNotOwned => DownloadError::FileNotOwned,
            protobuf_types::DownloadErrorType::FileTooLarge => DownloadError::FileTooLarge,
            protobuf_types::DownloadErrorType::ReadError => DownloadError::ReadError,
            protobuf_types::DownloadErrorType::WriteError => DownloadError::WriteError,
        }
    }
}

impl TryFrom<protobuf_types::PeerId> for PeerId {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::PeerId) -> Result<Self, Self::Error> {
        let id = value.uuid.try_into()?;

        Ok(Self {
            hostname: value.hostname,
            uuid: id,
        })
    }
}

impl TryFrom<protobuf_types::Uuid> for Uuid {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::Uuid) -> Result<Self, Self::Error> {
        match Uuid::from_slice(&value.data[..]) {
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Could not parse UUID"),
            )),
            Ok(id) => Ok(id),
        }
    }
}

impl From<protobuf_types::ContentLocation> for ContentLocation {
    fn from(value: protobuf_types::ContentLocation) -> Self {
        match value.content_location {
            None => ContentLocation::NetworkOnly,
            Some(path) => match path {
                protobuf_types::content_location::ContentLocation::LocalPath(path) => {
                    ContentLocation::LocalPath(PathBuf::from(path))
                }
            },
        }
    }
}

impl TryFrom<protobuf_types::DateTime> for DateTime<Utc> {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::DateTime) -> Result<Self, Self::Error> {
        match DateTime::<Utc>::from_str(&value.date) {
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Invalid date time string"),
            )),
            Ok(date) => Ok(date),
        }
    }
}

impl TryFrom<protobuf_types::SharedFile> for SharedFile {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::SharedFile) -> Result<Self, Self::Error> {
        let content_location = value.content_location.into();
        let identifier = value.identifier.try_into()?;
        let last_modified = value.last_modified.try_into()?;
        let mut owned_peers = vec![];
        for peer in value.owned_peers {
            let p: PeerId = peer.try_into()?;

            owned_peers.push(p);
        }

        Ok(Self {
            name: value.name,
            identifier,
            content_hash: value.content_hash,
            last_modified,
            content_location,
            owned_peers,
            size: value.size,
        })
    }
}

impl TryFrom<protobuf_types::ShareDirectorySignature> for ShareDirectorySignature {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::ShareDirectorySignature) -> Result<Self, Self::Error> {
        let mut shared_peers = vec![];
        for peer in value.shared_peers {
            let p = peer.try_into()?;

            shared_peers.push(p);
        }

        Ok(Self {
            name: value.name,
            identifier: value.identifier.try_into()?,
            last_modified: value.last_modified.try_into()?,
            shared_peers,
        })
    }
}

impl TryFrom<protobuf_types::ShareDirectory> for ShareDirectory {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::ShareDirectory) -> Result<Self, Self::Error> {
        let mut shared_files = HashMap::with_capacity(value.shared_files.len());

        for (id, file) in value.shared_files {
            let file = file.try_into()?;
            let id = match Uuid::from_str(&id) {
                Err(e) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Invalid date time string"),
                    ))
                }
                Ok(id) => id,
            };

            shared_files.insert(id, file);
        }

        Ok(Self {
            signature: value.signature.try_into()?,
            shared_files,
        })
    }
}
