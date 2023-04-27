use std::{collections::{HashMap, HashSet}, net::IpAddr};

use anyhow::{anyhow, bail, Result};
use futures::{SinkExt, StreamExt};
use mdns_sd::ServiceInfo;
use tauri::async_runtime::JoinHandle;
use tokio::{
    net::{
        tcp::{ReadHalf, WriteHalf},
        TcpStream,
    },
    sync::{broadcast, mpsc},
};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::{
    data::{ShareDirectory, ShareDirectorySignature},
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
    //GetDirectorySignatures,

    Synchronize,

    SendDirectories(Vec<ShareDirectory>),

    SharedDirectory(ShareDirectory),
}

pub struct ClientData {
    pub server: ServerHandle,
    pub passive_receiver: mpsc::Receiver<MessageFromServer>,
    pub active_receiver: mpsc::Receiver<MessageFromServer>,
    pub broadcast_receiver: broadcast::Receiver<MessageFromServer>,
    pub addr: ClientConnectionId,
}

pub struct ClientHandle {
    pub id: Option<PeerId>,
    pub passive_sender: mpsc::Sender<MessageFromServer>,
    pub active_sender: mpsc::Sender<MessageFromServer>,
    pub join: JoinHandle<()>,
    pub service_info: Option<ServiceInfo>,
    pub is_synchronised: bool,
}

struct ClientDataHandle<'a> {
    client_data: &'a mut ClientData,
    tcp_write: &'a mut FramedWrite<WriteHalf<'a>, MessageCodec>,
    client_peer_id: &'a mut Option<PeerId>,
}

pub async fn client_loop(mut client_data: ClientData, mut stream: TcpStream, mut client_peer_id: Option<PeerId>) {
    let (read, write) = stream.split();

    let mut framed_reader = FramedRead::new(read, MessageCodec {});
    let mut framed_writer = FramedWrite::new(write, MessageCodec {});
    let mut unfinished_messages: HashMap<u8, Vec<TcpMessage>> = HashMap::new();

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
                    error!("TCP connection err: {}", e);

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

            server_message = handle.client_data.broadcast_receiver.recv() => {
                match server_message {
                    Ok(message_from_server) => {
                        let result = handle_server_messages(message_from_server, &mut handle).await;

                        if let Err(e) = result {
                            error!("{}", e);

                            disconnect_self(handle.client_data.server.clone(), handle.client_data.addr).await;
                            return;
                        }
                    },
                    Err(e) => {
                        error!("{}", e);

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
                None => bail!("PeerId not yet set")
            };

            let _ = data
                .client_data
                .server
                .channel
                .send(msg)
                .await?;

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
                None => bail!("Client Peer Id not yet set"),
            };

            let directories = data.client_data.server.config.cached_data.lock().await;
            let directories: Vec<ShareDirectory> = directories
                .iter()
                .filter(|(dir_id, dir)| dir.signature.shared_peers.contains(id))
                .map(|(dir_id, dir)| dir.clone())
                .collect();

            if directories.len() > 0 {
                let _ = data
                    .tcp_write
                    .send(TcpMessage::SendDirectories(directories))
                    .await?;
            }

            Ok(())
        }

        TcpMessage::Part(_, _) => {
            todo!("Probably unused");
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

        MessageFromServer::SharedDirectory(directory) => {
            data.tcp_write
                .send(TcpMessage::SharedDirectory(directory))
                .await?;

            Ok(())
        }

        MessageFromServer::SendDirectories(directories) => {
            data.tcp_write
                .send(TcpMessage::SendDirectories(directories))
                .await?;

            Ok(())
        }

        MessageFromServer::Synchronize => {
            data.tcp_write
                .send(TcpMessage::Synchronize)
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
