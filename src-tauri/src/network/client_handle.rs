use std::{collections::HashMap, net::IpAddr, path::PathBuf};

use anyhow::{anyhow, Result, Ok};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader, AsyncWriteExt},
    net::{tcp::WriteHalf, TcpStream},
    sync::mpsc,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use uuid::Uuid;

use crate::{
    data::{ContentLocation, ShareDirectory, ShareDirectorySignature, SharedFile},
    network::server_handle::MessageToServer,
    peer_id::PeerId,
};

use super::{
    codec::{MessageCodec, TcpMessage},
    server_handle::{ClientConnectionId, ServerHandle, BackendError},
};

const FILE_CHUNK_SIZE: usize = 1024 * 4;

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

    SharedDirectory(ShareDirectory),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum DownloadError {
    DirectoryMissing,
    FileMissing,
    FileNotOwned,
    Canceled,
    ReadError,
}

struct DownloadHandle {
    bytes_total: u64,
    bytes_done: u64,
    output_file: File,
}

struct UploadHandle {
    is_cancelled: bool,
}

pub struct ClientData {
    pub server: ServerHandle,
    pub receiver: mpsc::Receiver<MessageFromServer>,
    pub addr: ClientConnectionId,
    pub downloads: HashMap<Uuid, DownloadHandle>,
    pub uploads: HashMap<Uuid, UploadHandle>,
}

struct ClientDataHandle<'a> {
    client_data: &'a mut ClientData,
    tcp_write: &'a mut FramedWrite<WriteHalf<'a>, MessageCodec>,
    client_peer_id: &'a mut Option<PeerId>,
}

pub async fn client_loop(
    mut client_data: ClientData,
    mut stream: TcpStream,
    mut client_peer_id: Option<PeerId>,
) {
    let (read, write) = stream.split();

    let mut framed_reader = FramedRead::new(read, MessageCodec {});
    let mut framed_writer = FramedWrite::new(write, MessageCodec {});

    let mut handle = ClientDataHandle {
        client_data: &mut client_data,
        tcp_write: &mut framed_writer,
        client_peer_id: &mut client_peer_id,
    };

    loop {
        tokio::select! {

            incoming = framed_reader.next() => {
                let result = handle_response(incoming, &mut handle).await;

                if let Err(e) = result {
                    error!("TCP err: {}", e);

                    disconnect_self(handle.client_data.server.clone(), handle.client_data.addr).await;
                    return;
                }
            }

            server_message = handle.client_data.receiver.recv() => {
                match server_message {
                    Some(message_from_server) => {
                        let result = handle_server_messages(message_from_server, &mut handle).await;

                        if let Err(e) = result {
                            error!("Server err: {}", e);

                            disconnect_self(handle.client_data.server.clone(), handle.client_data.addr).await;
                            return;
                        }
                    },
                    None => {
                        disconnect_self(handle.client_data.server.clone(), handle.client_data.addr).await;
                        return;
                    }
                }
            }

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
                .send(TcpMessage::SendPeerId(
                    data.client_data.server.peer_id.clone(),
                ))
                .await?;

            Ok(())
        }

        TcpMessage::SendPeerId(id) => {
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

        TcpMessage::SendDirectories(directories) => {
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
                    .send(TcpMessage::SendDirectories(directories))
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
            let mut directories = data.client_data.server.config.cached_data.lock().await;
            let dir = directories.get_mut(&dir_id);

            let result = match dir {
                None => Err(DownloadError::DirectoryMissing),
                Some(dir) => {
                    let file = dir.shared_files.get(&file_id);

                    match file {
                        None => Err(DownloadError::FileMissing),
                        Some(file) => {
                            match &file.content_location {
                                ContentLocation::NetworkOnly => Err(DownloadError::FileNotOwned), // return error saying I don't have this file
                                ContentLocation::LocalPath(path) => {
                                    let file_handle = File::open(path).await;

                                    match file_handle {
                                        Err(_) => Err(DownloadError::FileMissing),
                                        core::result::Result::Ok(file_handle) => {
                                            data.client_data.uploads.insert(
                                                download_id,
                                                UploadHandle {
                                                    is_cancelled: false,
                                                },
                                            );

                                            upload_file(file_handle, &data.client_data.uploads, &download_id, data.tcp_write).await
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            if let Err(e) = result {
                data.client_data.uploads.remove(&download_id);
                let _ = data.tcp_write.send(TcpMessage::DownloadError { error: e, download_id }).await?;
            }

            data.client_data.uploads.remove(&download_id);

            Ok(())
        }

        TcpMessage::SendFilePart { download_id, data: raw_data } => {
            let download = data.client_data.downloads.get_mut(&download_id);

            let result = match download {
                None => {
                    error!("Received file part for unknown download");

                    return Ok(());
                },
                Some(download) => {
                    let res = download.output_file.write_all(&raw_data).await;

                    match res {
                        Err(e) => Err(BackendError {
                            error: String::from(format!("Could not write downloaded data to file: {}", e.to_string())),
                            title: String::from("Download Failure")
                        }),
                        core::result::Result::Ok(_) => {
                            let bytes_received = u64::try_from(raw_data.len()).expect("app should be running on a 64 bit system");
                            download.bytes_done += bytes_received;

                            let percent = u8::try_from(download.bytes_done / download.bytes_total);

                            match percent {
                                Err(_) => Err(BackendError {
                                    error: String::from("Downloaded file is larger than expected"),
                                    title: String::from("Download Failure")
                                }),
                                core::result::Result::Ok(percent) => {
                                    let percent = u8::clamp(percent, 0, 100);
                                    let _ = data.client_data.server.channel.send(MessageToServer::DownloadUpdate { client_id: data.client_data.addr, download_id, new_progress: percent }).await?;

                                    core::result::Result::Ok(())
                                }
                            }
                        }
                    }
                }
            };

            if let Err(e) = result {
                data.client_data.downloads.remove(&download_id);
                let _ = data.client_data.server.channel.send(MessageToServer::CanceledDownload { client_id: data.client_data.addr, download_id }).await?;
            }

            Ok(())
        }

        TcpMessage::DownloadError { error, download_id } => {
            
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

        MessageFromServer::SendDirectories(mut directories) => {
            for dir in directories.iter_mut() {
                for (_, file) in dir.shared_files.iter_mut() {
                    file.content_location = ContentLocation::NetworkOnly;
                }
            }

            data.tcp_write
                .send(TcpMessage::SendDirectories(directories))
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
            let directories = data.client_data.server.config.cached_data.lock().await;
            let directory = directories.get(&directory_identifier);

            let result = match directory {
                None => Err(anyhow!("Directory does not exist")),
                Some(dir) => {
                    let file = dir.shared_files.get(&file_identifier);

                    match file {
                        None => Err(anyhow!("File does not exist")),
                        Some(file) => {
                            match &file.content_location {
                                ContentLocation::LocalPath(_) => Err(anyhow!("File has already been downloaded")),
                                ContentLocation::NetworkOnly => {
                                    let file_path = destination.join(&file.name);
                                    let file_path = if file_path.exists() {
                                        destination.join(download_id.to_string())
                                    } else {
                                        file_path
                                    };

                                    let file_handle = File::create(file_path).await;
                                    match file_handle {
                                        Err(e) => Err(anyhow!(e)),
                                        Ok(file_handle) => {
                                            data.client_data.downloads.insert(download_id, DownloadHandle { bytes_done: 0, output_file: file_handle });

                                            let send_result = data.tcp_write.send(TcpMessage::StartDownload { download_id, file_id: file_identifier, dir_id: directory_identifier }).await;

                                            Ok(())
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            Ok(())
        }
    }
}

async fn disconnect_self(server_handle: ServerHandle, addr: IpAddr) {
    let _ = server_handle
        .channel
        .send(MessageToServer::KillClient(addr))
        .await;

    warn!("Disconneting client {}.", addr);
}

async fn upload_file(file_handle: File, uploads: &HashMap<Uuid, UploadHandle>, download_id: &Uuid, tcp_write: &mut FramedWrite<WriteHalf<'_>, MessageCodec>) -> Result<(), DownloadError> {
    let mut buffer = [0; FILE_CHUNK_SIZE];
    let mut reader = BufReader::new(file_handle);

    loop {
        let upload_status = uploads.get(&download_id);

        if let Some(upload_status) = upload_status {
            if upload_status.is_cancelled {
                return Err(DownloadError::Canceled);
            }

            let res = reader.read(&mut buffer).await;

            match res {
                Ok(n) => {
                    let msg = if n == 0 {
                        TcpMessage::SendFileEnd { download_id: *download_id, data: vec![] }
                    } else {
                        TcpMessage::SendFilePart { download_id: *download_id, data: buffer[..n].to_vec() }
                    };

                    let send_result = tcp_write.send(msg).await;

                    match send_result {
                        Ok(_) => return Ok(()),
                        Err(_) => return Err(DownloadError::Canceled)
                    }
                }
                Err(e) => {
                    error!("{}", e);
                    return Err(DownloadError::ReadError);
                }
            }
        }
    }
}
