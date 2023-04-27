use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use cryptohelpers::crc::compute_stream;
use mdns_sd::ServiceInfo;
use serde::{Deserialize, Serialize};
use tauri::{
    async_runtime::{JoinHandle, Mutex},
    AppHandle, Manager,
};
use tokio::{
    net::TcpStream,
    sync::{
        broadcast,
        mpsc::{self, Sender},
        MutexGuard,
    },
};
use uuid::Uuid;

use crate::{
    config::StoredConfig,
    data::{ContentLocation, ShareDirectory, ShareDirectorySignature, SharedFile},
    peer_id::PeerId,
};

use super::{
    client_handle::{client_loop, ClientData, MessageFromServer},
    mdns::MessageToMdns,
};

const CHANNEL_SIZE: usize = 16;
const UPDATE_PERIOD: u64 = 5;
const MAIN_WINDOW_LABEL: &str = "main";

#[derive(Clone)]
pub struct ServerHandle {
    pub channel: Sender<MessageToServer>,
    pub config: Arc<StoredConfig>,
    pub peer_id: PeerId,
}

pub struct ClientHandle {
    pub id: Option<PeerId>,
    pub passive_sender: mpsc::Sender<MessageFromServer>,
    pub active_sender: mpsc::Sender<MessageFromServer>,
    pub join: JoinHandle<()>,
    pub service_info: Option<ServiceInfo>,
    pub job_queue: VecDeque<MessageFromServer>,
}

pub type ClientConnectionId = IpAddr;

#[derive(Debug)]
pub enum MessageToServer {
    SetPeerId(ClientConnectionId, PeerId),
    //ReceivedSignatures(Vec<ShareDirectorySignature>),
    ServiceFound(ServiceInfo),
    ConnectionAccepted(TcpStream, SocketAddr),
    KillClient(ClientConnectionId),

    SynchronizeDirectories(Vec<ShareDirectory>, PeerId),

    SharedDirectory(ShareDirectory),
    //NewShareDirectory(ShareDirectorySignature),
}

struct ServerData<'a> {
    window_manager: &'a AppHandle,
    server_handle: &'a ServerHandle,
    clients: &'a mut Mutex<HashMap<ClientConnectionId, ClientHandle>>,
    mdns_sender: &'a mpsc::Sender<MessageToMdns>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum WindowRequest {
    CreateShareDirectory(String),
    GetAllShareDirectoryData(bool),
    AddFiles {
        directory_identifier: String,
        file_paths: Vec<String>,
    },
    ShareDirectoryToPeers {
        directory_identifier: String,
        peers: Vec<PeerId>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddedFiles {
    pub directory_identifier: Uuid,
    pub shared_files: Vec<SharedFile>,
}

pub async fn server_loop(
    window_manager: AppHandle,
    mut client_receiver: mpsc::Receiver<MessageToServer>,
    mut window_receiver: mpsc::Receiver<WindowRequest>,
    mdns_sender: mpsc::Sender<MessageToMdns>,
    server_handle: ServerHandle,
) {
    // let (broadcast_sender, mut _broadcast_receiver) =
    //     broadcast::channel::<MessageFromServer>(CHANNEL_SIZE);
    let mut clients: Mutex<HashMap<ClientConnectionId, ClientHandle>> = Mutex::new(HashMap::new());
    let mut interval = tokio::time::interval(Duration::from_secs(UPDATE_PERIOD));

    loop {
        let server_data = ServerData {
            window_manager: &window_manager,
            server_handle: &server_handle,
            clients: &mut clients,
            mdns_sender: &mdns_sender,
        };

        tokio::select! {
            Some(msg) = client_receiver.recv() => {
                let result = handle_message(msg, server_data).await;

                if let Err(e) = result {
                    error!("{}", e);
                }
            }
            Some(request) = window_receiver.recv() => {
                let result = handle_request(request, server_data).await;

                if let Err(e) = result {
                    error!("{}", e);
                }
            }
            _ = interval.tick() => {
                do_periodic_work(server_data).await;
            }
        }
    }
}

async fn do_periodic_work<'a>(server_data: ServerData<'a>) {
    let mut clients = server_data.clients.lock().await;
    let mut clients_to_remove = vec![];

    for (key, value) in clients.iter_mut() {
        if !value.job_queue.is_empty() {
            let next_job = value.job_queue.pop_front();

            if let Some(job) = next_job {
                let send_result = value.passive_sender.send(job).await;

                if let Err(e) = send_result {
                    error!(
                    "Could not send value to client {} because {}. Client will be disconnected.",
                    key, e
                );
                    clients_to_remove.push(key.to_owned());
                }
            }
        }
    }

    for client_key in clients_to_remove.iter() {
        let removed = clients.remove(client_key);

        match removed {
            Some(client) => disconnected_client(client, server_data.mdns_sender).await,
            None => (),
        }
    }
}

async fn handle_message<'a>(msg: MessageToServer, mut server_data: ServerData<'a>) -> Result<()> {
    match msg {
        MessageToServer::ServiceFound(service) => {
            let ip_addr = service.get_addresses().iter().next();

            match ip_addr {
                Some(ip) => {
                    let properties = service.get_properties();
                    let port = properties.get("port");

                    let port = match port {
                        Some(p) => p,
                        None => bail!("No port property found"),
                    };

                    let port: u16 = port.parse()?;

                    let ip_addr = IpAddr::V4(*ip);
                    let socket_addr = SocketAddrV4::new(*ip, port);
                    let socket_addr = SocketAddr::V4(socket_addr);

                    let mut clients = server_data.clients.lock().await;

                    if !clients.contains_key(&ip_addr) {
                        let tcp_stream = TcpStream::connect(socket_addr).await?;

                        add_client(
                            server_data.server_handle.clone(),
                            &mut clients,
                            tcp_stream,
                            ip_addr,
                            Some(service.clone()),
                        )
                        .await?;

                        let _ = server_data
                            .mdns_sender
                            .send(MessageToMdns::ConnectedService(service))
                            .await?;

                        return Ok(());
                    } else {
                        Err(anyhow!("Service client already connected: {}", socket_addr))
                    }
                }
                None => Err(anyhow!("Service had no associated IP addresses")),
            }
        }

        MessageToServer::ConnectionAccepted(tcp, addr) => {
            let ip_addr = addr.ip();
            let mut clients = server_data.clients.lock().await;

            if !clients.contains_key(&ip_addr) {
                add_client(
                    server_data.server_handle.clone(),
                    &mut clients,
                    tcp,
                    ip_addr,
                    None,
                )
                .await
            } else {
                Err(anyhow!("TCP accepted client already connected: {}", addr))
            }
        }

        MessageToServer::SetPeerId(addr, id) => {
            let mut clients = server_data.clients.lock().await;
            let client = clients.get_mut(&addr);

            match client {
                Some(client) => {
                    client.id = Some(id);

                    let _ = client
                        .passive_sender
                        .send(MessageFromServer::Synchronize)
                        .await?;

                    Ok(())
                }
                None => Err(anyhow!("No such client for {}", addr)),
            }
        }

        MessageToServer::KillClient(client_addr) => {
            let client = server_data.clients.lock().await.remove(&client_addr);

            match client {
                Some(client) => {
                    disconnected_client(client, server_data.mdns_sender).await;

                    Ok(())
                }
                None => Err(anyhow!("No such client to drop: {}", client_addr)),
            }
        }

        MessageToServer::SharedDirectory(directory) => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;

            if !directories.contains_key(&directory.signature.identifier) {
                directories.insert(directory.signature.identifier, directory);

                return Ok(());
            }

            Err(anyhow!("Directory was already shared"))
        }

        MessageToServer::SynchronizeDirectories(directories, peer) => {
            let mut owned_dirs = server_data.server_handle.config.cached_data.lock().await;
            let mut clients = server_data.clients.lock().await;

            let client = clients.iter_mut().find(|(_, cdata)| match &cdata.id {
                Some(pid) => pid == &peer,
                None => false,
            });

            match client {
                Some((_, c)) => {
                    for dir in directories {
                        let od = owned_dirs.get_mut(&dir.signature.identifier);

                        match od {
                            Some(matched_dir) => {
                                for pid in dir.signature.shared_peers {
                                    if !matched_dir.signature.shared_peers.contains(&pid) {
                                        matched_dir.signature.shared_peers.push(pid);
                                    }
                                }

                                if dir.signature.last_modified > matched_dir.signature.last_modified
                                {
                                    for (id, file) in dir.shared_files {
                                        if !matched_dir.shared_files.contains_key(&id) {
                                            matched_dir.shared_files.insert(id, file);
                                        }
                                    }

                                    matched_dir.signature.last_modified =
                                        dir.signature.last_modified;
                                }
                            }
                            None => {
                                owned_dirs.insert(dir.signature.identifier, dir);
                            }
                        }
                    }

                    let _ = server_data.window_manager.emit_to(
                        MAIN_WINDOW_LABEL,
                        "UpdateShareDirectories",
                        owned_dirs
                            .values()
                            .cloned()
                            .collect::<Vec<ShareDirectory>>(),
                    )?;

                    Ok(())
                }
                None => Err(anyhow!("No client found")),
            }
        }
    }
}

async fn handle_request<'a>(msg: WindowRequest, mut server_data: ServerData<'a>) -> Result<()> {
    match msg {
        WindowRequest::CreateShareDirectory(name) => {
            let mut data = server_data.server_handle.config.cached_data.lock().await;

            let id = Uuid::new_v4();
            let signature = ShareDirectorySignature {
                name,
                identifier: id,
                last_modified: Utc::now(),
                shared_peers: vec![server_data.server_handle.peer_id.clone()],
            };
            let sd = ShareDirectory {
                signature: signature.clone(),
                shared_files: HashMap::new(),
            };

            data.insert(id, sd);
            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                "NewShareDirectory",
                signature,
            );

            Ok(())
        }

        WindowRequest::GetAllShareDirectoryData(_) => {
            let data = server_data.server_handle.config.cached_data.lock().await;

            let values: Vec<ShareDirectory> = data.values().cloned().collect();

            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                "UpdateShareDirectories",
                values,
            );

            Ok(())
        }

        WindowRequest::AddFiles {
            file_paths,
            directory_identifier,
        } => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;
            let id = Uuid::from_str(&directory_identifier)?;
            let directory = directories.get_mut(&id);

            if let Some(dir) = directory {
                let mut shared_files = vec![];
                for file_path in file_paths {
                    let shared_file =
                        create_shared_file(file_path, &server_data.server_handle.peer_id).await?;

                    shared_files.push(shared_file);
                }

                let payload = AddedFiles {
                    directory_identifier: dir.signature.identifier,
                    shared_files: shared_files.clone(),
                };

                let _ =
                    server_data
                        .window_manager
                        .emit_to(MAIN_WINDOW_LABEL, "AddedFiles", payload)?;

                for sf in shared_files {
                    dir.shared_files.insert(sf.identifier, sf);
                }

                return Ok(());
            }

            Err(anyhow!("Directory not found"))
        }

        WindowRequest::ShareDirectoryToPeers {
            peers,
            directory_identifier,
        } => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;
            let id = Uuid::from_str(&directory_identifier)?;
            let directory = directories.get_mut(&id);

            let clients = server_data.clients.lock().await;

            if let Some(dir) = directory {
                dir.signature.shared_peers.extend(peers);

                for (_, cval) in clients.iter() {
                    if let Some(pid) = &cval.id {
                        if dir.signature.shared_peers.contains(&pid) {
                            cval.passive_sender
                                .send(MessageFromServer::SendDirectories(vec![dir.clone()]))
                                .await?
                        }
                    }
                }

                return Ok(());
            }

            Err(anyhow!("Directory not found"))
        }
    }
}

async fn add_client<'a>(
    server_handle: ServerHandle,
    clients: &mut MutexGuard<'a, HashMap<IpAddr, ClientHandle>>,
    tcp: TcpStream,
    addr: ClientConnectionId,
    service_info: Option<ServiceInfo>,
) -> Result<()> {
    info!("Adding client with address {}", addr);

    let (passive_sender, passive_receiver) = mpsc::channel(CHANNEL_SIZE);
    let (active_sender, active_receiver) = mpsc::channel(CHANNEL_SIZE);

    let client_data = ClientData {
        server: server_handle,
        passive_receiver,
        active_receiver,
        addr,
    };

    let pid = match &service_info {
        Some(service) => {
            let name = service.get_fullname();

            PeerId::parse(name)
        }
        None => None,
    };

    let join = tauri::async_runtime::spawn(client_loop(client_data, tcp, pid.clone()));
    let mut job_queue = VecDeque::new();

    job_queue.push_back(MessageFromServer::GetPeerId);
    job_queue.push_back(MessageFromServer::Synchronize);

    let client = ClientHandle {
        id: pid.clone(),
        passive_sender: passive_sender.clone(),
        active_sender,
        join,
        service_info,
        job_queue,
    };

    let _ = clients.insert(addr, client);

    Ok(())
}

async fn disconnected_client<'a>(client: ClientHandle, mdns_sender: &mpsc::Sender<MessageToMdns>) {
    client.join.abort();

    if let Some(service) = client.service_info {
        let _ = mdns_sender
            .send(MessageToMdns::RemoveService(service))
            .await;
    }
}

async fn create_shared_file(file_path: String, this_peer: &PeerId) -> Result<SharedFile> {
    let path = PathBuf::from_str(&file_path)?;

    let mut file = tokio::fs::File::open(&path).await?;
    let metadata = file.metadata().await?;
    let checksum = compute_stream(&mut file).await?;

    let identifier = Uuid::new_v4();
    let name = match path.file_name() {
        Some(name) => match name.to_str() {
            Some(os_name) => os_name.to_string(),
            None => bail!("Invalid file path: {:?}", path),
        },
        None => bail!("Invalid file path: {:?}", path),
    };
    let now = Utc::now();
    let size = metadata.len();

    Ok(SharedFile {
        name,
        identifier,
        content_hash: checksum,
        last_modified: now,
        content_location: ContentLocation::LocalPath(path),
        owned_peers: vec![this_peer.clone()],
        size,
    })
}
