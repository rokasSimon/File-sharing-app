use std::{collections::HashMap, net::{SocketAddr, Ipv4Addr, SocketAddrV4}, sync::Arc};

use anyhow::Result;
use mdns_sd::ServiceInfo;
use tauri::async_runtime::{Receiver};
use tokio::{sync::{mpsc::{Sender, self}}, net::TcpStream};
use uuid::Uuid;

use crate::{config::StoredConfig, peer_id::PeerId};

use super::{client_handle::{ClientData, ClientHandle, client_loop, MessageFromServer}};

const CHANNEL_SIZE: usize = 64;

#[derive(Clone)]
pub struct ServerHandle {
    pub channel: Sender<MessageToServer>,
    pub config: Arc<StoredConfig>,
    pub peer_id: PeerId
}

pub enum MessageToServer {
    SetPeerId(SocketAddr, PeerId),
    ServiceFound(ServiceInfo),
    ConnectionAccepted(TcpStream, SocketAddr),
    KillClient(SocketAddr)
}

impl ServerHandle {
    
}

pub async fn server_loop(
    mut recv: Receiver<MessageToServer>,
    server_handle: ServerHandle
) {
    let mut clients: HashMap<SocketAddr, ClientHandle> = HashMap::new();

    while let Some(msg) = recv.recv().await {
        match msg {

            MessageToServer::ServiceFound(service) => {
                let ip_addr = service.get_addresses().iter().next();

                match ip_addr {
                    Some(ip) => {
                        let properties = service.get_properties();
                        let port = properties.get("port");

                        let port = match port {
                            Some(p) => p,
                            None => continue
                        };

                        let port: u16 = match port.parse() {
                            Ok(p) => p,
                            Err(_) => continue
                        };

                        let socket_addr = SocketAddrV4::new(*ip, port);
                        let socket_addr = SocketAddr::V4(socket_addr);

                        if !clients.contains_key(&socket_addr) {
                            let connection_result = TcpStream::connect(socket_addr).await;

                            match connection_result {
                                Ok(tcp_stream) => {
                                    add_client(&mut clients, tcp_stream, socket_addr, &server_handle).await;
                                },
                                Err(e) => error!("{}", e)
                            }
                        } else {
                            warn!("Client already connected: {}", socket_addr);
                        }
                    },
                    None => error!("Service had no associated IP addresses")
                }
            }

            MessageToServer::ConnectionAccepted(tcp, addr) => {
                if !clients.contains_key(&addr) {
                    add_client(&mut clients, tcp, addr, &server_handle).await;
                } else {
                    warn!("Client already connected: {}", addr);
                }
            }

            MessageToServer::SetPeerId(addr, id) => {
                let client = clients.get_mut(&addr);

                match client {
                    Some(client) => {
                        client.id = Some(id);
                    },
                    None => {
                        error!("No such client for {}", addr);
                    }
                }
            }

            MessageToServer::KillClient(client_addr) => {
                let client = clients.remove(&client_addr);

                match client {
                    Some(client) => {
                        client.join.abort();
                    },
                    None => {
                        error!("No such client to drop: {}", client_addr);
                    }
                }
            }

            _ => {
                info!("Something was sent to server handle");
            }
        }
    }
}

async fn add_client(clients: &mut HashMap<SocketAddr, ClientHandle>, tcp: TcpStream, addr: SocketAddr, server: &ServerHandle) {
    let (passive_sender, passive_receiver) = mpsc::channel(CHANNEL_SIZE);
    let (active_sender, active_receiver) = mpsc::channel(CHANNEL_SIZE);

    let client_data = ClientData {
        server: server.clone(),
        stream: tcp,
        passive_receiver,
        active_receiver
    };

    let join = tauri::async_runtime::spawn(client_loop(client_data));

    let client = ClientHandle {
        id: None,
        passive_sender,
        active_sender,
        join
    };

    let send_result  = client.passive_sender.send(MessageFromServer::GetPeerId).await;

    if let Err(e) = send_result {
        error!("{}", e);
    }

    let add_result = clients.insert(addr, client);

    if let None = add_result {
        error!("Could not add client: {}", addr);
    }
}