use std::net::IpAddr;

use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use tokio::{
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
    server_handle::{ClientConnectionId, ServerHandle},
};

#[derive(Clone, Debug)]
pub enum MessageFromServer {
    GetPeerId,

    Synchronize,

    SendDirectories(Vec<ShareDirectory>),

    AddedFiles(ShareDirectorySignature, Vec<SharedFile>),
    DeleteFile(PeerId, ShareDirectorySignature, Uuid),

    SharedDirectory(ShareDirectory),
}

pub struct ClientData {
    pub server: ServerHandle,
    pub passive_receiver: mpsc::Receiver<MessageFromServer>,
    pub active_receiver: mpsc::Receiver<MessageFromServer>,
    pub addr: ClientConnectionId,
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

            server_message = handle.client_data.passive_receiver.recv() => {
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
    }
}

async fn disconnect_self(server_handle: ServerHandle, addr: IpAddr) {
    let _ = server_handle
        .channel
        .send(MessageToServer::KillClient(addr))
        .await;

    warn!("Disconneting client {}.", addr);
}
