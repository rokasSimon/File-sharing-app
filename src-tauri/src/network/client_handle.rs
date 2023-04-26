use std::{collections::HashMap, net::IpAddr};

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
    data::ShareDirectorySignature, network::server_handle::MessageToServer, peer_id::PeerId,
};

use super::{
    codec::{MessageCodec, TcpMessage},
    server_handle::{ClientConnectionId, ServerHandle},
};

#[derive(Clone, Debug)]
pub enum MessageFromServer {
    GetPeerId,
    NotifyNewDirectory(ShareDirectorySignature),
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
}

struct ClientDataHandle<'a> {
    client_data: &'a mut ClientData,
    tcp_write: &'a mut FramedWrite<WriteHalf<'a>, MessageCodec>,
    client_peer_id: &'a mut Option<PeerId>,
}

pub async fn client_loop(mut client_data: ClientData, mut stream: TcpStream) {
    let (read, write) = stream.split();

    let mut framed_reader = FramedRead::new(read, MessageCodec {});
    let mut framed_writer = FramedWrite::new(write, MessageCodec {});
    let mut unfinished_messages: HashMap<u8, Vec<TcpMessage>> = HashMap::new();
    let mut client_peer_id: Option<PeerId> = None;

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
                    error!("{}", e);

                    disconnect_self(handle.client_data.server.clone(), handle.client_data.addr).await;
                    return;
                }
            }

            server_message = handle.client_data.passive_receiver.recv() => {
                match server_message {
                    Some(message_from_server) => {
                        let result = handle_server_messages(message_from_server, &mut handle).await;

                        if let Err(e) = result {
                            error!("{}", e);

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
) -> Result<(), String> {
    match incoming {
        Some(result) => match result {
            Ok(message) => {
                let result = handle_tcp_message(message, client_data).await;

                result
            }
            Err(e) => return Err(e.to_string()),
        },
        None => return Err("TCP Connection was closed".to_string()),
    }
}

async fn handle_tcp_message<'a>(
    incoming: TcpMessage,
    data: &mut ClientDataHandle<'a>,
) -> Result<(), String> {
    match incoming {

        TcpMessage::RequestPeerId => {
            let res = data.tcp_write
                .send(TcpMessage::SendPeerId(data.client_data.server.peer_id.clone()))
                .await;

            if let Err(e) = res {
                return Err(e.to_string());
            }

            Ok(())
        }

        TcpMessage::SendPeerId(id) => {
            info!("Received {} peer id", &id);

            let result = data.client_data
                .server
                .channel
                .send(MessageToServer::SetPeerId(
                    data.client_data.addr,
                    id.clone(),
                ))
                .await;

            if let Err(e) = result {
                return Err(e.to_string());
            }

            *data.client_peer_id = Some(id);

            Ok(())
        }

        TcpMessage::NewShareDirectory(directory) => {
            info!("New share directory started: {:?}", directory);

            let result = data.client_data
                .server
                .channel
                .send(MessageToServer::NewShareDirectory(directory))
                .await;

            if let Err(e) = result {
                return Err(e.to_string());
            }

            Ok(())
        }

        _ => Err("Unhandled TCP message".to_string()),
    }
}

async fn handle_server_messages(
    msg: MessageFromServer,
    data: &mut ClientDataHandle<'_>
) -> Result<(), String> {
    match msg {

        MessageFromServer::GetPeerId => {
            let result = data.tcp_write.send(TcpMessage::RequestPeerId).await;

            if let Err(e) = result {
                return Err(e.to_string())
            }

            Ok(())
        },

        MessageFromServer::NotifyNewDirectory(directory) => {
            let result = data.tcp_write.send(TcpMessage::NewShareDirectory(directory)).await;

            if let Err(e) = result {
                return Err(e.to_string())
            }

            Ok(())
        },

        _ => Err(format!("Unhandled server message {:?}", msg))
    }
}

async fn disconnect_self(server_handle: ServerHandle, addr: IpAddr) {
    let _ = server_handle
        .channel
        .send(MessageToServer::KillClient(addr))
        .await;

    warn!("Disconneting client {}.", addr);
}
