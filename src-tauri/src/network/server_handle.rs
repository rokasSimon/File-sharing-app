use std::{collections::HashMap, net::{SocketAddr, SocketAddrV4, Ipv4Addr, IpAddr}, sync::Arc, time::Duration};

use mdns_sd::ServiceInfo;
use tauri::async_runtime::{Receiver};
use tokio::{sync::{mpsc::{Sender, self}}, net::TcpStream};

use crate::{config::StoredConfig, peer_id::PeerId};

use super::{client_handle::{ClientData, ClientHandle, client_loop, MessageFromServer}};

const CHANNEL_SIZE: usize = 64;
const UPDATE_PERIOD: u64 = 10;

#[derive(Clone)]
pub struct ServerHandle {
    pub channel: Sender<MessageToServer>,
    pub config: Arc<StoredConfig>,
    pub peer_id: PeerId
}

pub type ClientConnectionId = IpAddr;

pub enum MessageToServer {
    SetPeerId(ClientConnectionId, PeerId),
    ServiceFound(ServiceInfo),
    ConnectionAccepted(TcpStream, SocketAddr),
    KillClient(ClientConnectionId)
}

struct ServerData<'a> {
    //recv: &'a mut Receiver<MessageToServer>,
    server_handle: &'a ServerHandle,
    clients: &'a mut HashMap<ClientConnectionId, ClientHandle>
}

impl ServerHandle {
    
}

pub async fn server_loop(
    mut recv: Receiver<MessageToServer>,
    server_handle: ServerHandle
) {
    let mut clients: HashMap<ClientConnectionId, ClientHandle> = HashMap::new();
    let mut interval = tokio::time::interval(Duration::from_secs(UPDATE_PERIOD));

    loop {
        tokio::select! {
            Some(msg) = recv.recv() => {
                let server_data = ServerData {
                    //recv: &mut recv,
                    server_handle: &server_handle,
                    clients: &mut clients
                };

                handle_message(msg, server_data).await;
            }
            _ = interval.tick() => {
                let server_data = ServerData {
                    //recv: &mut recv,
                    server_handle: &server_handle,
                    clients: &mut clients
                };

                do_periodic_work(server_data).await;
            }
        }
    }
}

async fn add_client(clients: &mut HashMap<ClientConnectionId, ClientHandle>, tcp: TcpStream, addr: ClientConnectionId, server: &ServerHandle) {
    warn!("Adding client with address {}", addr);

    let (passive_sender, passive_receiver) = mpsc::channel(CHANNEL_SIZE);
    let (active_sender, active_receiver) = mpsc::channel(CHANNEL_SIZE);

    let client_data = ClientData {
        server: server.clone(),
        stream: tcp,
        passive_receiver,
        active_receiver,
        addr
    };

    let join = tauri::async_runtime::spawn(client_loop(client_data));

    let client = ClientHandle {
        id: None,
        passive_sender,
        active_sender,
        join
    };

    let _ = clients.insert(addr, client);
}

async fn do_periodic_work<'a>(server_data: ServerData<'a>) {
    for (key, value) in server_data.clients {
        if value.id.is_none() {
            let send_result = value.passive_sender.send(MessageFromServer::GetPeerId).await;

            if let Err(e) = send_result {
                error!("Could not send value to client {} because {}", key, e);
            }
        }
    }
}

async fn handle_message<'a>(msg: MessageToServer, mut server_data: ServerData<'a>) {
    match msg {

        MessageToServer::ServiceFound(service) => {
            let ip_addr = service.get_addresses().iter().next();

            match ip_addr {
                Some(ip) => {
                    let properties = service.get_properties();
                    let port = properties.get("port");

                    let port = match port {
                        Some(p) => p,
                        None => return
                    };

                    let port: u16 = match port.parse() {
                        Ok(p) => p,
                        Err(_) => return
                    };

                    let ip_addr = IpAddr::V4(*ip);
                    let socket_addr = SocketAddrV4::new(*ip, port);
                    let socket_addr = SocketAddr::V4(socket_addr);

                    if !server_data.clients.contains_key(&ip_addr) {
                        let connection_result = TcpStream::connect(socket_addr).await;

                        match connection_result {
                            Ok(tcp_stream) => {
                                add_client(&mut server_data.clients, tcp_stream, ip_addr, &server_data.server_handle).await;
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
            let ip_addr = addr.ip();

            if !server_data.clients.contains_key(&ip_addr) {
                add_client(&mut server_data.clients, tcp, ip_addr, &server_data.server_handle).await;
            } else {
                warn!("Client already connected: {}", addr);
            }
        }

        MessageToServer::SetPeerId(addr, id) => {
            let client = server_data.clients.get_mut(&addr);

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
            let client = server_data.clients.remove(&client_addr);

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