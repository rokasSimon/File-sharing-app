pub mod protobuf_types {
    include!(concat!(env!("OUT_DIR"), "/file_share.tcp_messages.rs"));
}

use std::{collections::HashMap, path::PathBuf, str::FromStr};

use chrono::{DateTime, Utc};
use protobuf_types::tcp_message;
use uuid::Uuid;

use crate::{
    data::{ContentLocation, ShareDirectory, ShareDirectorySignature, SharedFile},
    network::client::DownloadError,
    peer_id::PeerId,
};

use self::protobuf_types::{AddedFiles, CancelDownload, DeleteFile, SignalType};

fn map_files(files: Vec<protobuf_types::SharedFile>) -> Result<Vec<SharedFile>, std::io::Error> {
    let mut mapped = vec![];

    for file in files {
        let f = file.try_into()?;

        mapped.push(f);
    }

    Ok(mapped)
}

fn map_files_out(files: Vec<SharedFile>) -> Vec<protobuf_types::SharedFile> {
    let mut mapped = vec![];

    for file in files {
        mapped.push(file.into());
    }

    mapped
}

fn map_directories(
    dirs: Vec<protobuf_types::ShareDirectory>,
) -> Result<Vec<ShareDirectory>, std::io::Error> {
    let mut mapped = vec![];

    for dir in dirs {
        let f = dir.try_into()?;

        mapped.push(f);
    }

    Ok(mapped)
}

fn map_directories_out(dirs: Vec<ShareDirectory>) -> Vec<protobuf_types::ShareDirectory> {
    let mut mapped = vec![];

    for dir in dirs {
        mapped.push(dir.into());
    }

    mapped
}

impl From<super::TcpMessage> for protobuf_types::tcp_message::Message {
    fn from(value: super::TcpMessage) -> Self {
        match value {
            super::TcpMessage::AddedFiles { directory, files } => {
                tcp_message::Message::AddedFiles(AddedFiles {
                    directory: directory.into(),
                    files: map_files_out(files),
                })
            }

            super::TcpMessage::CancelDownload { download_id } => {
                tcp_message::Message::CancelDownload(CancelDownload {
                    download_id: download_id.into(),
                })
            }

            super::TcpMessage::DeleteFile {
                peer_id,
                directory,
                file,
            } => tcp_message::Message::DeleteFile(DeleteFile {
                peer_id: peer_id.into(),
                directory: directory.into(),
                file_identifier: file.into(),
            }),

            super::TcpMessage::DownloadError { error, download_id } => {
                tcp_message::Message::DownloadError(protobuf_types::DownloadError {
                    download_id: download_id.into(),
                    error: error as i32,
                })
            }

            super::TcpMessage::DownloadedFile {
                peer_id,
                directory_identifier,
                file_identifier,
                date_modified,
            } => tcp_message::Message::DownloadedFile(protobuf_types::DownloadedFile {
                peer_id: peer_id.into(),
                directory_identifier: directory_identifier.into(),
                file_identifier: file_identifier.into(),
                date_modified: date_modified.into(),
            }),

            super::TcpMessage::LeftDirectory {
                directory_identifier,
                date_modified,
            } => tcp_message::Message::LeftDirectory(protobuf_types::LeftDirectory {
                directory_identifier: directory_identifier.into(),
                date_modified: date_modified.into(),
            }),

            super::TcpMessage::ReceiveDirectories(dirs) => {
                tcp_message::Message::ReceiveDirectories(protobuf_types::ReceiveDirectories {
                    directories: map_directories_out(dirs),
                })
            }
            super::TcpMessage::ReceiveFileEnd { download_id } => {
                tcp_message::Message::ReceiveFileEnd(protobuf_types::ReceiveFileEnd {
                    download_id: download_id.into(),
                })
            }
            super::TcpMessage::ReceiveFilePart { download_id, data } => {
                tcp_message::Message::ReceiveFilePart(protobuf_types::ReceiveFilePart {
                    download_id: download_id.into(),
                    data,
                })
            }
            super::TcpMessage::ReceivePeerId(id) => {
                tcp_message::Message::ReceivePeerId(protobuf_types::ReceivePeerId {
                    peer_id: id.into(),
                })
            }
            super::TcpMessage::RequestPeerId => {
                tcp_message::Message::Signal(SignalType::RequestPeerId.into())
            }
            super::TcpMessage::SharedDirectory(dir) => {
                tcp_message::Message::SharedDirectory(protobuf_types::SharedDirectory {
                    directory: dir.into(),
                })
            }
            super::TcpMessage::StartDownload {
                download_id,
                file_id,
                dir_id,
            } => tcp_message::Message::StartDownload(protobuf_types::StartDownload {
                download_id: download_id.into(),
                file_id: file_id.into(),
                dir_id: dir_id.into(),
            }),
            super::TcpMessage::Synchronize => {
                tcp_message::Message::Signal(SignalType::Synchronize.into())
            }
        }
    }
}

impl TryFrom<protobuf_types::tcp_message::Message> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::tcp_message::Message) -> Result<Self, Self::Error> {
        match value {
            tcp_message::Message::AddedFiles(added_files) => added_files.try_into(),
            tcp_message::Message::DeleteFile(delete_file) => delete_file.try_into(),
            tcp_message::Message::Signal(signal) => {
                Ok(protobuf_types::SignalType::from_i32(signal)
                    .unwrap_or_default()
                    .into())
            }
            tcp_message::Message::CancelDownload(cancel_download) => cancel_download.try_into(),
            tcp_message::Message::DownloadError(err) => err.try_into(),
            tcp_message::Message::DownloadedFile(d) => d.try_into(),
            tcp_message::Message::LeftDirectory(d) => d.try_into(),
            tcp_message::Message::ReceiveDirectories(d) => d.try_into(),
            tcp_message::Message::ReceiveFileEnd(f) => f.try_into(),
            tcp_message::Message::ReceiveFilePart(f) => f.try_into(),
            tcp_message::Message::ReceivePeerId(p) => p.try_into(),
            tcp_message::Message::SharedDirectory(d) => d.try_into(),
            tcp_message::Message::StartDownload(d) => d.try_into(),
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
            Err(_) => Err(std::io::Error::new(
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
            Err(_) => Err(std::io::Error::new(
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
                Err(_) => {
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

impl TryFrom<protobuf_types::AddedFiles> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::AddedFiles) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::AddedFiles {
            directory: value.directory.try_into()?,
            files: map_files(value.files)?,
        })
    }
}

impl TryFrom<protobuf_types::DeleteFile> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::DeleteFile) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::DeleteFile {
            peer_id: value.peer_id.try_into()?,
            directory: value.directory.try_into()?,
            file: value.file_identifier.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::CancelDownload> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::CancelDownload) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::CancelDownload {
            download_id: value.download_id.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::DownloadError> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::DownloadError) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::DownloadError {
            error: protobuf_types::DownloadErrorType::from_i32(value.error)
                .unwrap_or_default()
                .into(),
            download_id: value.download_id.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::DownloadedFile> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::DownloadedFile) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::DownloadedFile {
            peer_id: value.peer_id.try_into()?,
            directory_identifier: value.directory_identifier.try_into()?,
            file_identifier: value.file_identifier.try_into()?,
            date_modified: value.date_modified.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::LeftDirectory> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::LeftDirectory) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::LeftDirectory {
            directory_identifier: value.directory_identifier.try_into()?,
            date_modified: value.date_modified.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::ReceiveDirectories> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::ReceiveDirectories) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::ReceiveDirectories(map_directories(
            value.directories,
        )?))
    }
}

impl TryFrom<protobuf_types::ReceiveFileEnd> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::ReceiveFileEnd) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::ReceiveFileEnd {
            download_id: value.download_id.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::ReceiveFilePart> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::ReceiveFilePart) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::ReceiveFilePart {
            download_id: value.download_id.try_into()?,
            data: value.data,
        })
    }
}

impl TryFrom<protobuf_types::ReceivePeerId> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::ReceivePeerId) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::ReceivePeerId(value.peer_id.try_into()?))
    }
}

impl TryFrom<protobuf_types::StartDownload> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::StartDownload) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::StartDownload {
            download_id: value.download_id.try_into()?,
            file_id: value.file_id.try_into()?,
            dir_id: value.dir_id.try_into()?,
        })
    }
}

impl TryFrom<protobuf_types::SharedDirectory> for super::TcpMessage {
    type Error = std::io::Error;

    fn try_from(value: protobuf_types::SharedDirectory) -> Result<Self, Self::Error> {
        Ok(super::TcpMessage::SharedDirectory(
            value.directory.try_into()?,
        ))
    }
}

impl From<Uuid> for protobuf_types::Uuid {
    fn from(value: Uuid) -> Self {
        Self {
            data: value.as_bytes().to_vec(),
        }
    }
}

impl From<DateTime<Utc>> for protobuf_types::DateTime {
    fn from(value: DateTime<Utc>) -> Self {
        Self {
            date: value.to_string(),
        }
    }
}

impl From<PeerId> for protobuf_types::PeerId {
    fn from(value: PeerId) -> Self {
        Self {
            uuid: value.uuid.into(),
            hostname: value.hostname,
        }
    }
}

impl From<ContentLocation> for protobuf_types::ContentLocation {
    fn from(_: ContentLocation) -> Self {
        Self {
            content_location: None,
        }
    }
}

impl From<SharedFile> for protobuf_types::SharedFile {
    fn from(value: SharedFile) -> Self {
        let mut owned_peers = vec![];

        for p in value.owned_peers {
            owned_peers.push(p.into());
        }

        Self {
            name: value.name,
            identifier: value.identifier.into(),
            content_hash: value.content_hash,
            last_modified: value.last_modified.into(),
            content_location: value.content_location.into(),
            owned_peers,
            size: value.size,
        }
    }
}

impl From<ShareDirectorySignature> for protobuf_types::ShareDirectorySignature {
    fn from(value: ShareDirectorySignature) -> Self {
        let mut shared_peers = vec![];

        for p in value.shared_peers {
            shared_peers.push(p.into());
        }

        Self {
            name: value.name,
            identifier: value.identifier.into(),
            last_modified: value.last_modified.into(),
            shared_peers,
        }
    }
}

impl From<ShareDirectory> for protobuf_types::ShareDirectory {
    fn from(value: ShareDirectory) -> Self {
        let mut files = HashMap::new();

        for (id, file) in value.shared_files {
            files.insert(id.to_string(), file.into());
        }

        Self {
            signature: value.signature.into(),
            shared_files: files,
        }
    }
}
