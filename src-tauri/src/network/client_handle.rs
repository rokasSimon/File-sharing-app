use core::fmt;
use std::{collections::HashMap, path::PathBuf, error::Error};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::Mutex;
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::{tcp::WriteHalf, TcpStream},
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

use crate::{
    data::{ContentLocation, ShareDirectory, ShareDirectorySignature, SharedFile},
    network::{server_handle::MessageToServer, codec::TcpMessage},
    peer_id::PeerId,
};

use super::{
    codec::{MessageCodec},
    server_handle::{ClientConnectionId, Download, ServerHandle},
};

const FILE_CHUNK_SIZE: usize = 1024 * 50; // 50 KB

#[derive(Clone, Debug)]
pub enum MessageFromServer {
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

    SharedDirectory(ShareDirectory),
    LeftDirectory {
        directory_identifier: Uuid,
    }
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

impl DownloadError {
    pub fn to_string(&self) -> String {
        match self {
            DownloadError::NoClientsConnected => "No clients to download from. Reconnect other devices to network".to_owned(),
            DownloadError::DirectoryMissing => "Download directory is missing. File might have been removed while being downloaded.".to_owned(),
            DownloadError::FileMissing => "Download file is missing. File might have been removed while downloading.".to_owned(),
            DownloadError::FileNotOwned => "Client does not have this file locally.".to_owned(),
            DownloadError::FileTooLarge => "Download file size might have changed while it was being downloaded.".to_owned(),
            DownloadError::Canceled => "Download was manually canceled.".to_owned(),
            DownloadError::Disconnected => "Download was canceled since one of the clients disconnected".to_owned(),
            DownloadError::ReadError => "Could not read file to download.".to_owned(),
            DownloadError::WriteError => "Could not write file.".to_owned(),
        }
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
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
    file_id: Uuid,
    dir_id: Uuid,
}

pub struct ClientData {
    pub server: ServerHandle,
    pub receiver: mpsc::Receiver<MessageFromServer>,
    pub addr: ClientConnectionId,
}

struct ClientDataHandle<'a> {
    client_data: &'a mut ClientData,
    tcp_write: &'a mut FramedWrite<WriteHalf<'a>, MessageCodec>,
    client_peer_id: &'a mut Option<PeerId>,
    downloads: &'a mut Mutex<HashMap<Uuid, DownloadHandle>>,
    uploads: &'a mut Mutex<HashMap<Uuid, UploadHandle>>,
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
    let mut downloads: Mutex<HashMap<Uuid, DownloadHandle>> = Mutex::new(HashMap::new());
    let mut uploads: Mutex<HashMap<Uuid, UploadHandle>> = Mutex::new(HashMap::new());
    let mut uploading = false;

    let mut handle = ClientDataHandle {
        client_data: &mut client_data,
        tcp_write: &mut framed_writer,
        client_peer_id: &mut client_peer_id,
        downloads: &mut downloads,
        uploads: &mut uploads,
        uploading: &mut uploading,
    };

    loop {
        let up = handle.uploading.clone();

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

async fn handle_uploads<'a>(
    client_data: &mut ClientDataHandle<'a>,
) -> Result<()> {
    let mut uploads = client_data.uploads.lock().await;

    let mut uploads_to_remove: Vec<Uuid> = vec![];
    for (download_id, upload) in uploads.iter_mut() {
        let mut directories = client_data.client_data.server.config.cached_data.lock().await;
        let upload_result = try_upload(*download_id, &mut directories, client_data.tcp_write, upload).await;

        match upload_result {
            Err(error) => {
                uploads_to_remove.push(*download_id);

                info!("Download {} errored out because {}", download_id, error);

                let _ = client_data.tcp_write.send(TcpMessage::DownloadError { error, download_id: *download_id }).await;
            },
            Ok(is_finished) => {
                if is_finished {
                    uploads_to_remove.push(*download_id);
                }
            }
        }
    }

    for download_id in uploads_to_remove {
        info!("Removing download {}", download_id);
        uploads.remove(&download_id);
    }

    if uploads.len() == 0 {
        *client_data.uploading = false;
    }

    Ok(())
}

async fn try_upload<'a>(
    download_id: Uuid,
    directories: &mut HashMap<Uuid, ShareDirectory>,
    tcp_write: &mut FramedWrite<WriteHalf<'_>, MessageCodec>,
    upload: &mut UploadHandle,
) -> Result<bool, DownloadError> {
    if upload.canceled {
        return Err(DownloadError::Canceled);
    }

    let directory = match directories.get(&upload.dir_id) {
        None => return Err(DownloadError::DirectoryMissing),
        Some(directory) => directory
    };

    let _ = match directory.shared_files.get(&upload.file_id) {
        None => return Err(DownloadError::FileMissing),
        Some(file) => file,
    };

    let read_res = upload.reader.read(&mut upload.buffer).await;
    let n = match read_res {
        Err(_) => return Err(DownloadError::ReadError),
        Ok(n) => n,
    };

    let msg = if n == 0 {
        TcpMessage::ReceiveFileEnd {
            download_id,
        }
    } else {
        TcpMessage::ReceiveFilePart {
            download_id,
            data: upload.buffer[..n].to_vec(),
        }
    };

    let send_result = tcp_write.send(msg).await;

    match send_result {
        Err(_) => return Err(DownloadError::Disconnected),
        Ok(_) => {
            return Ok(n == 0);
        }
    }
}

async fn handle_response<'a>(
    incoming: Option<Result<TcpMessage, std::io::Error>>,
    client_data: &mut ClientDataHandle<'a>,
) -> Result<()> {
    match incoming {
        Some(result) => match result {
            Ok(message) => {
                let result = handle_tcp_message(message, client_data).await;

                result
            }
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
            let _ = data
                .tcp_write
                .send(TcpMessage::ReceivePeerId(
                    data.client_data.server.peer_id.clone(),
                ))
                .await?;

            Ok(())
        }

        TcpMessage::ReceivePeerId(id) => {
            info!("Received {} peer id", &id);

            let _ = data
                .client_data
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

            let _ = data.client_data.server.channel.send(msg).await?;

            Ok(())
        }

        TcpMessage::SharedDirectory(directory) => {
            info!("Directory was shared {:?}", &directory);

            let _ = data
                .client_data
                .server
                .channel
                .send(MessageToServer::SharedDirectory(directory))
                .await?;

            Ok(())
        }

        TcpMessage::LeftDirectory { directory_identifier, date_modified} => {
            let peer = match data.client_peer_id {
                None => {
                    error!("Peer ID not yet set");
                    return Ok(());
                },
                Some(p) => p
            };

            let mut directories = data.client_data.server.config.cached_data.lock().await;
            let directory = directories.get_mut(&directory_identifier);
            let directory = match directory {
                None => return Ok(()),
                Some(dir) => dir,
            };

            directory.remove_peer(peer, date_modified);

            let _ = data.client_data.server.channel.send(MessageToServer::UpdatedDirectory(directory_identifier)).await?;

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

            let directories = data.client_data.server.config.cached_data.lock().await;
            let directories: Vec<ShareDirectory> = directories
                .iter()
                .filter(|(_, dir)| dir.signature.shared_peers.contains(id))
                .map(|(_, dir)| {
                    let mut out = dir.clone();

                    for (_, mut file) in out.shared_files.iter_mut() {
                        file.content_location = ContentLocation::NetworkOnly;
                    }

                    out
                })
                .collect();

            if directories.len() > 0 {
                let _ = data
                    .tcp_write
                    .send(TcpMessage::ReceiveDirectories(directories))
                    .await?;
            }

            Ok(())
        }

        TcpMessage::AddedFiles { directory, files } => {
            info!("Received add request for files {:?}", files);

            let mut directories = data.client_data.server.config.cached_data.lock().await;
            let dir = directories.get_mut(&directory.identifier);

            if let Some(dir) = dir {
                dir.add_files(files, dir.signature.last_modified);

                let _ = data
                    .client_data
                    .server
                    .channel
                    .send(MessageToServer::UpdatedDirectory(dir.signature.identifier))
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

            let mut directories = data.client_data.server.config.cached_data.lock().await;
            let dir = directories.get_mut(&directory.identifier);

            if let Some(dir) = dir {
                dir.delete_files(&peer_id, directory.last_modified, vec![file]);

                let _ = data
                    .client_data
                    .server
                    .channel
                    .send(MessageToServer::UpdatedDirectory(dir.signature.identifier))
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

            let mut directories = data.client_data.server.config.cached_data.lock().await;
            let result = start_upload(&mut directories, dir_id, file_id).await;

            match result {
                Err(e) => {
                    match e {
                        DownloadError::Disconnected => return Err(anyhow!("Client disconnected")),
                        _ => {
                            data.tcp_write.send(TcpMessage::DownloadError { error: e, download_id }).await?;
                        }
                    }
                },
                Ok(handle) => {
                    let mut uploads = data.uploads.lock().await;
                    uploads.insert(download_id, handle);

                    *data.uploading = true;
                    info!("Successfully added upload handle");
                }
            }

            Ok(())
        }

        TcpMessage::CancelDownload { download_id } => {
            info!("Trying to cancel download {}", download_id);
            let mut uploads = data.uploads.lock().await;
            let upload = uploads.get_mut(&download_id);

            if let Some(upload) = upload {
                upload.canceled = true;
            }

            Ok(())
        }

        TcpMessage::ReceiveFilePart {
            download_id,
            data: raw_data,
        } => {
            let mut downloads = data.downloads.lock().await;
            let download = downloads.get_mut(&download_id);

            let result = match download {
                None => {
                    //error!("Received file part for unknown download");

                    return Ok(());
                }
                Some(download) => {
                    let res = download.output_file.write_all(&raw_data).await;

                    match res {
                        Err(e) => {
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
                                let _ = data
                                    .client_data
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
                downloads.remove(&download_id);
                let _ = data
                    .client_data
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
            let mut downloads = data.downloads.lock().await;
            let download = downloads.remove(&download_id);

            match download {
                None => Ok(()),
                Some(download) => {
                    let _ = fs::remove_file(download.output_path).await;

                    let _ = data
                        .client_data
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
            let mut downloads = data.downloads.lock().await;
            if !downloads.contains_key(&download_id) {
                error!("Received file end for unknown download");

                return Ok(());
            }

            let mut directories = data.client_data.server.config.cached_data.lock().await;
            let mut download = downloads.remove(&download_id).unwrap();
            let directory = directories.get_mut(&download.dir_id);

            let result = match directory {
                None => {
                    download.canceled = true;
                    Err(DownloadError::DirectoryMissing)
                }
                Some(directory) => {
                    let file = directory.shared_files.get_mut(&download.file_id);

                    match file {
                        None => {
                            download.canceled = true;
                            Err(DownloadError::FileMissing)
                        }
                        Some(file) => {
                            directory.signature.last_modified = Utc::now();
                            file.owned_peers.push(data.client_data.server.peer_id.clone());
                            file.content_location = ContentLocation::LocalPath(download.output_path);

                            let _ = data
                                .client_data
                                .server
                                .channel
                                .send(MessageToServer::FinishedDownload {
                                    download_id,
                                    directory_identifier: download.dir_id,
                                    file_identifier: download.file_id,
                                })
                                .await?;

                            Ok(())
                        }
                    }
                }
            };

            if let Err(e) = result {
                let _ = data
                    .client_data
                    .server
                    .channel
                    .send(MessageToServer::CanceledDownload {
                        download_id,
                        cancel_reason: e.to_string(),
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
            let mut directories = data.client_data.server.config.cached_data.lock().await;

            if let Some(directory) = directories.get_mut(&directory_identifier) {
                if let Some(file) = directory.shared_files.get_mut(&file_identifier) {
                    directory.add_owner(peer_id, date_modified, vec![file_identifier]);

                    data.client_data
                        .server
                        .channel
                        .send(MessageToServer::UpdatedDirectory(directory_identifier))
                        .await?;
                }
            }

            Ok(())
        }
    }
}

async fn handle_server_messages(
    msg: MessageFromServer,
    data: &mut ClientDataHandle<'_>,
) -> Result<()> {
    match msg {
        MessageFromServer::GetPeerId => {
            data.tcp_write.send(TcpMessage::RequestPeerId).await?;

            Ok(())
        }

        MessageFromServer::SharedDirectory(mut directory) => {
            for (_, file) in directory.shared_files.iter_mut() {
                file.content_location = ContentLocation::NetworkOnly;
            }

            data.tcp_write
                .send(TcpMessage::SharedDirectory(directory))
                .await?;

            Ok(())
        }

        MessageFromServer::LeftDirectory { directory_identifier } => {
            data.tcp_write
                .send(TcpMessage::LeftDirectory { directory_identifier, date_modified: Utc::now() })
                .await?;

            Ok(())
        }

        MessageFromServer::SendDirectories(mut directories) => {
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

        MessageFromServer::Synchronize => {
            data.tcp_write.send(TcpMessage::Synchronize).await?;

            Ok(())
        }

        MessageFromServer::DeleteFile(peer_id, directory, file) => {
            data.tcp_write
                .send(TcpMessage::DeleteFile {
                    peer_id,
                    directory,
                    file,
                })
                .await?;

            Ok(())
        }

        MessageFromServer::AddedFiles(directory, mut files) => {
            for file in files.iter_mut() {
                file.content_location = ContentLocation::NetworkOnly;
            }

            data.tcp_write
                .send(TcpMessage::AddedFiles { directory, files })
                .await?;

            Ok(())
        }

        MessageFromServer::StartDownload {
            download_id,
            file_identifier,
            directory_identifier,
            destination,
        } => {
            let this_client = match data.client_peer_id {
                None => return Err(anyhow!("Client has not assigned peer ID yet")),
                Some(id) => id,
            };

            let directories = data.client_data.server.config.cached_data.lock().await;
            let directory = directories.get(&directory_identifier);

            let result = match directory {
                None => Err(DownloadError::DirectoryMissing),
                Some(dir) => {
                    let file = dir.shared_files.get(&file_identifier);

                    match file {
                        None => Err(DownloadError::FileMissing),
                        Some(file) => match &file.content_location {
                            ContentLocation::LocalPath(_) => {
                                error!("File has already been downloaded");

                                return Ok(());
                            }
                            ContentLocation::NetworkOnly => {
                                let file_handle = File::create(&destination).await;
                                match file_handle {
                                    Err(_) => Err(DownloadError::WriteError),
                                    Ok(file_handle) => {
                                        let mut downloads = data.downloads.lock().await;

                                        downloads.insert(
                                            download_id,
                                            DownloadHandle {
                                                canceled: false,
                                                bytes_total: file.size,
                                                bytes_done: 0,
                                                output_file: file_handle,
                                                output_path: destination.clone(),
                                                file_id: file_identifier,
                                                dir_id: directory_identifier,
                                            },
                                        );

                                        let _ = data
                                            .tcp_write
                                            .send(TcpMessage::StartDownload {
                                                download_id,
                                                file_id: file_identifier,
                                                dir_id: directory_identifier,
                                            })
                                            .await?;

                                        let _ = data
                                            .client_data
                                            .server
                                            .channel
                                            .send(MessageToServer::StartedDownload {
                                                download_info: Download {
                                                    peer: this_client.clone(),
                                                    download_id,
                                                    file_identifier,
                                                    directory_identifier,
                                                    progress: 0,
                                                    file_name: file.name.clone(),
                                                    file_path: destination,
                                                },
                                            })
                                            .await?;

                                        Ok(())
                                    }
                                }
                            }
                        },
                    }
                }
            };

            if let Err(e) = result {
                error!("{}", e);

                let _ = data
                    .client_data
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

        MessageFromServer::UpdateOwners {
            peer_id,
            directory_identifier,
            file_identifier,
            date_modified,
        } => {
            let _ = data
                .tcp_write
                .send(TcpMessage::DownloadedFile {
                    peer_id,
                    directory_identifier,
                    file_identifier,
                    date_modified
                })
                .await?;

            Ok(())
        }

        MessageFromServer::CancelDownload { download_id } => {
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
        let mut downloads = client_data_handle.downloads.lock().await;
        for (_, download) in downloads.iter_mut() {
            download.canceled = true;
            if let Ok(_) = download.output_file.shutdown().await {
                let _ = fs::remove_file(download.output_path.clone()).await;
            }
        }
    }

    {
        let mut uploads = client_data_handle.uploads.lock().await;
        for (_, upload) in uploads.iter_mut() {
            upload.canceled = true;
        }
    }

    warn!(
        "Disconneting client {}.",
        client_data_handle.client_data.addr
    );
}

async fn start_upload(
    directories: &mut HashMap<Uuid, ShareDirectory>,
    dir_id: Uuid,
    file_id: Uuid
) -> Result<UploadHandle, DownloadError> {
    let dir = directories.get(&dir_id);
    let dir = match dir {
        None => return Err(DownloadError::DirectoryMissing),
        Some(dir) => dir,
    };

    let file = dir.shared_files.get(&file_id);
    let file = match file {
        None => return Err(DownloadError::FileMissing),
        Some(file) => file,
    };

    let file_path = match &file.content_location {
        ContentLocation::NetworkOnly => return Err(DownloadError::FileNotOwned),
        ContentLocation::LocalPath(path) => path.clone()
    };

    let file = File::open(file_path).await;
    let file = match file {
        Err(_) => return Err(DownloadError::WriteError),
        Ok(file) => file,
    };

    Ok(UploadHandle {
        dir_id,
        file_id,
        canceled: false,
        reader: BufReader::new(file),
        buffer: [0; FILE_CHUNK_SIZE],
    })
}