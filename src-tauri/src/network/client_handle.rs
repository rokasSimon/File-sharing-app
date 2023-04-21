use std::collections::HashMap;

use futures::{StreamExt, SinkExt};
use tauri::async_runtime::{JoinHandle};
use tokio::{net::TcpStream, sync::mpsc};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::{peer_id::PeerId, network::server_handle::MessageToServer};

use super::{server_handle::{ServerHandle, ClientConnectionId}, codec::{MessageCodec, TcpMessage}};

pub enum MessageFromServer {
    GetPeerId
}

pub struct ClientData {
    pub server: ServerHandle,
    pub stream: TcpStream,
    pub passive_receiver: mpsc::Receiver<MessageFromServer>,
    pub active_receiver: mpsc::Receiver<MessageFromServer>,
    pub addr: ClientConnectionId
}

pub struct ClientHandle {
    pub id: Option<PeerId>,
    pub passive_sender: mpsc::Sender<MessageFromServer>,
    pub active_sender: mpsc::Sender<MessageFromServer>,
    pub join: JoinHandle<()>
}

pub async fn client_loop(mut client_data: ClientData) {
    let (read, write) = client_data.stream.split();

    let mut framed_reader = FramedRead::new(read, MessageCodec {});
    let mut framed_writer = FramedWrite::new(write, MessageCodec {});
    let mut unfinished_messages: HashMap<u8, Vec<TcpMessage>> = HashMap::new();
    let mut client_peer_id: Option<PeerId> = None;

    loop {
        tokio::select! {
            incoming = framed_reader.next() => {
                if let Some(result) = incoming {
                    match result {
                        Ok(message) => {
                            match message {

                                TcpMessage::RequestPeerId => {
                                    let res = framed_writer.send(TcpMessage::SendPeerId(client_data.server.peer_id.clone())).await;

                                    if let Err(e) = res {
                                        error!("{}", e);
                                    }
                                },

                                TcpMessage::SendPeerId(id) => {
                                    warn!("Received {} peer id", &id);

                                    client_peer_id = Some(id);
                                    let _ = client_data.server.channel.send(MessageToServer::SetPeerId(client_data.addr, client_peer_id.unwrap())).await;
                                }

                                _ => { }
                            }
                        }
                        Err(e) => error!("{}", e)
                    }
                } else {
                    let _ = client_data.server.channel.send(MessageToServer::KillClient(client_data.addr)).await;

                    warn!("Received empty message from framed reader so client is being shut down");

                    return;
                }
            }
            server_message = client_data.passive_receiver.recv() => {
                if let Some(message_from_server) = server_message {
                    match message_from_server {

                        MessageFromServer::GetPeerId => {
                            let result = framed_writer.send(TcpMessage::RequestPeerId).await;

                            if let Err(e) = result {
                                error!("{}", e);
                            }
                        },
                        
                        _ => ()
                    }
                }
            }
        }
    }
}