use core::fmt;
use std::{collections::HashMap, error::Error, path::PathBuf, sync::Arc};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};

use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::{tcp::WriteHalf, TcpStream},
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

mod codec;
mod protobuf;

use crate::{
    config::StoredConfig,
    data::{ContentLocation, PeerId, ShareDirectory, ShareDirectorySignature, SharedFile},
    server::{ClientConnectionId, MessageToServer, ServerHandle},
    window::Download,
};

use self::codec::{MessageCodec, TcpMessage};

const FILE_CHUNK_SIZE: usize = 1024 * 50; // 50 KB

#[derive(Debug, Clone)]
pub enum MessageToClient {
    GetPeerId,
    Synchronize,

    SendDirectories(Vec<ShareDirectory>),

    AddedFiles(ShareDirectorySignature, Vec<SharedFile>),
    DeleteFile(PeerId, ShareDirectorySignature, Uuid),

    StartDownload {
        download_id: Uuid,
        file_identifier: Uuid,
        directory_identifier: Uuid,
        destination: PathBuf,
    },
    CancelDownload {
        download_id: Uuid,
    },
    UpdateOwners {
        peer_id: PeerId,
        directory_identifier: Uuid,
        file_identifier: Uuid,
        date_modified: DateTime<Utc>,
    },

    LeftDirectory {
        directory_identifier: Uuid,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum DownloadError {
    NoClientsConnected = 0,
    DirectoryMissing,
    FileMissing,
    FileNotOwned,
    FileTooLarge,
    Disconnected,
    Canceled,
    ReadError,
    WriteError,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            DownloadError::NoClientsConnected => "No clients to download from. Reconnect other devices to network".to_owned(),
            DownloadError::DirectoryMissing => "Download directory is missing. File might have been removed while being downloaded.".to_owned(),
            DownloadError::FileMissing => "Download file is missing. File might have been removed while downloading.".to_owned(),
            DownloadError::FileNotOwned => "Client does not have this file locally.".to_owned(),
            DownloadError::FileTooLarge => "Download file size might have changed while it was being downloaded.".to_owned(),
            DownloadError::Canceled => "Download was manually canceled.".to_owned(),
            DownloadError::Disconnected => "Download was canceled since one of the clients disconnected".to_owned(),
            DownloadError::ReadError => "Could not read file to download.".to_owned(),
            DownloadError::WriteError => "Could not write file.".to_owned(),
        };

        write!(f, "{}", msg)
    }
}

impl Error for DownloadError {}

struct DownloadHandle {
    canceled: bool,
    bytes_total: u64,
    bytes_done: u64,
    output_file: File,
    output_path: PathBuf,
    file_id: Uuid,
    dir_id: Uuid,
}

struct UploadHandle {
    canceled: bool,
    reader: BufReader<File>,
    buffer: [u8; FILE_CHUNK_SIZE],
}

pub struct ClientData {
    pub server: ServerHandle,
    pub receiver: mpsc::Receiver<MessageToClient>,
    pub addr: ClientConnectionId,
    pub config: Arc<StoredConfig>,
}

struct ClientDataHandle<'a> {
    client_data: &'a mut ClientData,
    tcp_write: &'a mut FramedWrite<WriteHalf<'a>, MessageCodec>,
    client_peer_id: &'a mut Option<PeerId>,
    downloads: &'a mut HashMap<Uuid, DownloadHandle>,
    uploads: &'a mut HashMap<Uuid, UploadHandle>,
    uploading: &'a mut bool,
}

pub async fn client_loop(
    mut client_data: ClientData,
    mut stream: TcpStream,
    mut client_peer_id: Option<PeerId>,
) {
    let (read, write) = stream.split();

    let mut framed_reader = FramedRead::new(read, MessageCodec {});
    let mut framed_writer = FramedWrite::new(write, MessageCodec {});
    let mut downloads: HashMap<Uuid, DownloadHandle> = HashMap::new();
    let mut uploads: HashMap<Uuid, UploadHandle> = HashMap::new();
    let mut uploading = false;

    let _ = framed_writer.send(TcpMessage::RequestPeerId).await;

    let mut handle = ClientDataHandle {
        client_data: &mut client_data,
        tcp_write: &mut framed_writer,
        client_peer_id: &mut client_peer_id,
        downloads: &mut downloads,
        uploads: &mut uploads,
        uploading: &mut uploading,
    };

    loop {
        let up = *handle.uploading;

        tokio::select! {

            incoming = framed_reader.next() => {
                let result = handle_response(incoming, &mut handle).await;

                if let Err(e) = result {
                    error!("TCP err: {}", e);

                    disconnect_self(&mut handle).await;
                    return;
                }
            }

            server_message = handle.client_data.receiver.recv() => {
                match server_message {
                    Some(message_from_server) => {
                        let result = handle_server_messages(message_from_server, &mut handle).await;

                        if let Err(e) = result {
                            error!("Server err: {}", e);

                            disconnect_self(&mut handle).await;
                            return;
                        }
                    },
                    None => {
                        disconnect_self(&mut handle).await;
                        return;
                    }
                }
            }

            _ = async {}, if up => {
                let _ = handle_uploads(&mut handle).await;
            }

        }
    }
}

async fn handle_uploads<'a>(client_data: &mut ClientDataHandle<'a>) -> Result<()> {
    let mut uploads_to_remove: Vec<Uuid> = vec![];
    for (download_id, upload) in client_data.uploads.iter_mut() {
        let upload_result = try_upload(*download_id, client_data.tcp_write, upload).await;

        match upload_result {
            Err(error) => {
                uploads_to_remove.push(*download_id);

                let _ = client_data
                    .tcp_write
                    .send(TcpMessage::DownloadError {
                        error,
                        download_id: *download_id,
                    })
                    .await;
            }
            Ok(is_finished) => {
                if is_finished {
                    uploads_to_remove.push(*download_id);
                }
            }
        }
    }

    for download_id in uploads_to_remove {
        info!("Removing download {}", download_id);
        client_data.uploads.remove(&download_id);
    }

    if client_data.uploads.is_empty() {
        *client_data.uploading = false;
    }

    Ok(())
}

async fn try_upload<'a>(
    download_id: Uuid,
    tcp_write: &mut FramedWrite<WriteHalf<'_>, MessageCodec>,
    upload: &mut UploadHandle,
) -> Result<bool, DownloadError> {
    if upload.canceled {
        return Err(DownloadError::Canceled);
    }

    let read_res = upload.reader.read(&mut upload.buffer).await;
    let n = match read_res {
        Err(_) => return Err(DownloadError::ReadError),
        Ok(n) => n,
    };

    let msg = if n == 0 {
        TcpMessage::ReceiveFileEnd { download_id }
    } else {
        TcpMessage::ReceiveFilePart {
            download_id,
            data: upload.buffer[..n].to_vec(),
        }
    };

    let send_result = tcp_write.send(msg).await;

    match send_result {
        Err(_) => Err(DownloadError::Disconnected),
        Ok(_) => Ok(n == 0),
    }
}

async fn handle_response<'a>(
    incoming: Option<Result<TcpMessage, std::io::Error>>,
    client_data: &mut ClientDataHandle<'a>,
) -> Result<()> {
    match incoming {
        Some(result) => match result {
            Ok(message) => handle_tcp_message(message, client_data).await,
            Err(e) => Err(anyhow!(e.to_string())),
        },
        None => Err(anyhow!("TCP Connection was closed")),
    }
}

async fn handle_tcp_message<'a>(
    incoming: TcpMessage,
    data: &mut ClientDataHandle<'a>,
) -> Result<()> {
    match incoming {
        TcpMessage::RequestPeerId => {
            data.tcp_write
                .send(TcpMessage::ReceivePeerId(
                    data.client_data.server.peer_id.clone(),
                ))
                .await?;

            Ok(())
        }

        TcpMessage::ReceivePeerId(id) => {
            info!("Received {} peer id", &id);

            let _ = data.tcp_write.send(TcpMessage::Synchronize).await;

            data.client_data
                .server
                .channel
                .send(MessageToServer::SetPeerId(
                    data.client_data.addr,
                    id.clone(),
                ))
                .await?;

            *data.client_peer_id = Some(id);

            Ok(())
        }

        TcpMessage::ReceiveDirectories(directories) => {
            info!("Received {:?} directories", &directories);

            let msg = match data.client_peer_id {
                Some(pid) => MessageToServer::SynchronizeDirectories(directories, pid.clone()),
                None => {
                    warn!("Peer ID not yet set");

                    return Ok(());
                }
            };

            data.client_data.server.channel.send(msg).await?;

            Ok(())
        }

        TcpMessage::SharedDirectory(directory) => {
            info!("Directory was shared {:?}", &directory);

            data.client_data
                .server
                .channel
                .send(MessageToServer::SharedDirectory(directory))
                .await?;

            Ok(())
        }

        TcpMessage::LeftDirectory {
            directory_identifier,
            date_modified,
        } => {
            let peer = match data.client_peer_id {
                None => {
                    error!("Peer ID not yet set");
                    return Ok(());
                }
                Some(p) => p,
            };

            data.client_data
                .server
                .channel
                .send(MessageToServer::LeftDirectory {
                    directory_identifier,
                    peer_id: peer.clone(),
                    date_modified,
                })
                .await?;

            Ok(())
        }

        TcpMessage::Synchronize => {
            info!("Synchronizing with {:?}", &data.client_peer_id);

            let id = match data.client_peer_id {
                Some(pid) => pid,
                None => {
                    warn!("Client Peer Id not yet set");

                    return Ok(());
                }
            };

            let mut directories: Vec<ShareDirectory> =
                data.client_data.config.get_directories().await;

            for dir in directories.iter_mut() {
                for (_, mut file) in dir.shared_files.iter_mut() {
                    file.content_location = ContentLocation::NetworkOnly;
                }
            }

            directories.retain(|dir| dir.signature.shared_peers.contains(id));

            if !directories.is_empty() {
                data.tcp_write
                    .send(TcpMessage::ReceiveDirectories(directories))
                    .await?;
            }

            Ok(())
        }

        TcpMessage::AddedFiles { directory, files } => {
            info!("Received add request for files {:?}", files);

            let mut success = false;
            data.client_data
                .config
                .mutate_dir(directory.identifier, |dir| {
                    let result = dir.add_files(files, directory.last_modified);

                    if result.is_ok() {
                        success = true;
                    }
                })
                .await;

            if success {
                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::UpdatedDirectory(directory.identifier))
                    .await?;
            }

            Ok(())
        }

        TcpMessage::DeleteFile {
            peer_id,
            directory,
            file,
        } => {
            info!("Received delete request for file {}", file);

            let success = false;
            data.client_data
                .config
                .mutate_dir(directory.identifier, |dir| {
                    dir.remove_files(&peer_id, directory.last_modified, vec![file]);

                    success = true;
                })
                .await;

            if success {
                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::UpdatedDirectory(directory.identifier))
                    .await?;
            }

            Ok(())
        }

        TcpMessage::StartDownload {
            download_id,
            file_id,
            dir_id,
        } => {
            info!("Started uploading");

            let file_path = data.client_data.config.get_filepath(dir_id, file_id).await;

            match file_path {
                None => {
                    data.tcp_write
                        .send(TcpMessage::DownloadError {
                            error: DownloadError::FileNotOwned,
                            download_id,
                        })
                        .await?
                }
                Some(path) => {
                    let file = File::open(path).await;

                    match file {
                        Err(_e) => {
                            data.tcp_write
                                .send(TcpMessage::DownloadError {
                                    error: DownloadError::FileMissing,
                                    download_id,
                                })
                                .await?
                        }
                        Ok(file) => {
                            let upload = UploadHandle {
                                canceled: false,
                                reader: BufReader::new(file),
                                buffer: [0; FILE_CHUNK_SIZE],
                            };

                            data.uploads.insert(download_id, upload);
                            *data.uploading = true;
                        }
                    }
                }
            }

            Ok(())
        }

        TcpMessage::CancelDownload { download_id } => {
            info!("Trying to cancel download {}", download_id);
            let upload = data.uploads.get_mut(&download_id);

            if let Some(upload) = upload {
                upload.canceled = true;
            }

            Ok(())
        }

        TcpMessage::ReceiveFilePart {
            download_id,
            data: raw_data,
        } => {
            let download = data.downloads.get_mut(&download_id);

            let result = match download {
                None => {
                    error!("Received file part for unknown download");

                    return Ok(());
                }
                Some(download) => {
                    let res = download.output_file.write_all(&raw_data).await;

                    match res {
                        Err(_) => {
                            download.canceled = true;
                            Err(DownloadError::WriteError)
                        }
                        Ok(_) => {
                            let bytes_received = u64::try_from(raw_data.len())
                                .expect("app should be running on a 64 bit system");
                            download.bytes_done += bytes_received;

                            let percent =
                                (download.bytes_done as f64 / download.bytes_total as f64) * 100.0;
                            let percent = percent.round() as u64;

                            if percent > 100 {
                                download.canceled = true;

                                Err(DownloadError::FileTooLarge)
                            } else {
                                data.client_data
                                    .server
                                    .channel
                                    .send(MessageToServer::DownloadUpdate {
                                        download_id,
                                        new_progress: percent,
                                    })
                                    .await?;

                                Ok(())
                            }
                        }
                    }
                }
            };

            if let Err(e) = result {
                data.downloads.remove(&download_id);
                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::CanceledDownload {
                        cancel_reason: e.to_string(),
                        download_id,
                    })
                    .await?;
            }

            Ok(())
        }

        TcpMessage::DownloadError { error, download_id } => {
            error!("Download error: {:?}", error);
            let download = data.downloads.remove(&download_id);

            match download {
                None => Ok(()),
                Some(download) => {
                    let _ = fs::remove_file(download.output_path).await;

                    data.client_data
                        .server
                        .channel
                        .send(MessageToServer::CanceledDownload {
                            cancel_reason: error.to_string(),
                            download_id,
                        })
                        .await?;

                    Ok(())
                }
            }
        }

        TcpMessage::ReceiveFileEnd { download_id } => {
            if !data.downloads.contains_key(&download_id) {
                error!("Received file end for unknown download");

                return Ok(());
            }

            let download = data.downloads.remove(&download_id).unwrap();
            let mut success = false;
            data.client_data
                .config
                .mutate_dir(download.dir_id, |dir| {
                    dir.add_owner(
                        &data.client_data.server.peer_id,
                        Utc::now(),
                        vec![download.file_id],
                        Some(download.output_path),
                    );

                    success = true;
                })
                .await;

            if success {
                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::FinishedDownload {
                        download_id,
                        directory_identifier: download.dir_id,
                        file_identifier: download.file_id,
                    })
                    .await?;
            } else {
                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::CanceledDownload {
                        download_id,
                        cancel_reason: "Could not finish download".to_string(),
                    })
                    .await?;
            }

            Ok(())
        }

        TcpMessage::DownloadedFile {
            peer_id,
            directory_identifier,
            file_identifier,
            date_modified,
        } => {
            let mut success = false;
            data.client_data
                .config
                .mutate_dir(directory_identifier, |dir| {
                    dir.add_owner(&peer_id, date_modified, vec![file_identifier], None);

                    success = true;
                })
                .await;

            if success {
                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::UpdatedDirectory(directory_identifier))
                    .await?;
            }

            Ok(())
        }
    }
}

async fn handle_server_messages(
    msg: MessageToClient,
    data: &mut ClientDataHandle<'_>,
) -> Result<()> {
    match msg {
        MessageToClient::GetPeerId => {
            data.tcp_write.send(TcpMessage::RequestPeerId).await?;

            Ok(())
        }

        MessageToClient::LeftDirectory {
            directory_identifier,
        } => {
            data.tcp_write
                .send(TcpMessage::LeftDirectory {
                    directory_identifier,
                    date_modified: Utc::now(),
                })
                .await?;

            Ok(())
        }

        MessageToClient::SendDirectories(mut directories) => {
            for dir in directories.iter_mut() {
                for (_, file) in dir.shared_files.iter_mut() {
                    file.content_location = ContentLocation::NetworkOnly;
                }
            }

            data.tcp_write
                .send(TcpMessage::ReceiveDirectories(directories))
                .await?;

            Ok(())
        }

        MessageToClient::Synchronize => {
            data.tcp_write.send(TcpMessage::Synchronize).await?;

            Ok(())
        }

        MessageToClient::DeleteFile(peer_id, directory, file) => {
            data.tcp_write
                .send(TcpMessage::DeleteFile {
                    peer_id,
                    directory,
                    file,
                })
                .await?;

            Ok(())
        }

        MessageToClient::AddedFiles(directory, mut files) => {
            for file in files.iter_mut() {
                file.content_location = ContentLocation::NetworkOnly;
            }

            data.tcp_write
                .send(TcpMessage::AddedFiles { directory, files })
                .await?;

            Ok(())
        }

        MessageToClient::StartDownload {
            download_id,
            file_identifier,
            directory_identifier,
            destination,
        } => {
            let this_client = match data.client_peer_id {
                None => return Err(anyhow!("Client has not assigned peer ID yet")),
                Some(id) => id,
            };

            let mut file_size = None;
            data.client_data
                .config
                .mutate_file(directory_identifier, file_identifier, |file| {
                    file_size = Some(file.size);
                })
                .await;

            let result = match file_size {
                None => Err(DownloadError::FileMissing),
                Some(file_size) => {
                    let file_handle = File::create(&destination).await;

                    match file_handle {
                        Err(_) => Err(DownloadError::WriteError),
                        Ok(file_handle) => {
                            let file_name = &destination
                                .file_name()
                                .unwrap_or_default()
                                .to_str()
                                .unwrap_or_default();

                            data.downloads.insert(
                                download_id,
                                DownloadHandle {
                                    canceled: false,
                                    bytes_total: file_size,
                                    bytes_done: 0,
                                    output_file: file_handle,
                                    output_path: destination.clone(),
                                    file_id: file_identifier,
                                    dir_id: directory_identifier,
                                },
                            );

                            data.tcp_write
                                .send(TcpMessage::StartDownload {
                                    download_id,
                                    file_id: file_identifier,
                                    dir_id: directory_identifier,
                                })
                                .await?;

                            data.client_data
                                .server
                                .channel
                                .send(MessageToServer::StartedDownload {
                                    download_info: Download {
                                        peer: this_client.clone(),
                                        download_id,
                                        file_identifier,
                                        directory_identifier,
                                        progress: 0,
                                        file_name: file_name.to_string(),
                                        file_path: destination,
                                    },
                                })
                                .await?;

                            Ok(())
                        }
                    }
                }
            };

            if let Err(e) = result {
                error!("{}", e);

                data.client_data
                    .server
                    .channel
                    .send(MessageToServer::CanceledDownload {
                        cancel_reason: e.to_string(),
                        download_id,
                    })
                    .await?;
            }

            Ok(())
        }

        MessageToClient::UpdateOwners {
            peer_id,
            directory_identifier,
            file_identifier,
            date_modified,
        } => {
            data.tcp_write
                .send(TcpMessage::DownloadedFile {
                    peer_id,
                    directory_identifier,
                    file_identifier,
                    date_modified,
                })
                .await?;

            Ok(())
        }

        MessageToClient::CancelDownload { download_id } => {
            info!("Server says to cancel download {}", download_id);

            let _ = data
                .tcp_write
                .send(TcpMessage::CancelDownload { download_id })
                .await;

            Ok(())
        }
    }
}

async fn disconnect_self(client_data_handle: &mut ClientDataHandle<'_>) {
    let _ = client_data_handle
        .client_data
        .server
        .channel
        .send(MessageToServer::KillClient(
            client_data_handle.client_data.addr,
        ))
        .await;

    {
        for (id, download) in client_data_handle.downloads.iter_mut() {
            download.canceled = true;
            if download.output_file.shutdown().await.is_ok() {
                let _ = fs::remove_file(download.output_path.clone()).await;
            }

            let _ = client_data_handle
                .client_data
                .server
                .channel
                .send(MessageToServer::CanceledDownload {
                    download_id: *id,
                    cancel_reason: "Client was disconnected".to_string(),
                })
                .await;
        }
    }

    {
        for (_, upload) in client_data_handle.uploads.iter_mut() {
            upload.canceled = true;
        }
    }

    warn!(
        "Disconneting client {}.",
        client_data_handle.client_data.addr
    );
}
