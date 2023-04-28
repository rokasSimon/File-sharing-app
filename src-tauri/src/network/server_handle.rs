use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    net::{IpAddr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration, vec,
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
    fs,
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
    ServiceFound(ServiceInfo),
    ConnectionAccepted(TcpStream, SocketAddr),
    KillClient(ClientConnectionId),

    SynchronizeDirectories(Vec<ShareDirectory>, PeerId),
    UpdatedDirectory(Uuid),

    SharedDirectory(ShareDirectory),
}

struct ServerData<'a> {
    window_manager: &'a AppHandle,
    server_handle: &'a ServerHandle,
    clients: &'a mut Mutex<HashMap<ClientConnectionId, ClientHandle>>,
    mdns_sender: &'a mpsc::Sender<MessageToMdns>,
}

impl ServerData<'_> {
    pub async fn broadcast(&self, peers: &Vec<PeerId>, msg: MessageFromServer) {
        let clients = self.clients.lock().await;
        let found_clients: Vec<_> = clients
            .iter()
            .filter(|(_, c)| match &c.id {
                Some(id) => peers.contains(&id),
                None => false,
            })
            .collect();

        for (_, c) in found_clients {
            let _ = c.passive_sender.send(msg.clone()).await;
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum WindowRequest {
    CreateShareDirectory(String),
    GetAllShareDirectoryData(bool),
    GetPeers(bool),
    AddFiles {
        directory_identifier: String,
        file_paths: Vec<String>,
    },
    ShareDirectoryToPeers {
        directory_identifier: String,
        peers: Vec<PeerId>,
    },
    DownloadFile {
        directory_identifier: String,
        file_identifier: String,
    },
    DeleteFile {
        directory_identifier: String,
        file_identifier: String,
    },
}

pub struct WindowAction;

impl WindowAction {
    pub const UpdateDirectory: &str = "UpdateDirectory";
    pub const UpdateShareDirectories: &str = "UpdateShareDirectories";
    pub const GetPeers: &str = "GetPeers";
    pub const NewShareDirectory: &str = "NewShareDirectory";
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
                let send_result = value.passive_sender.send(job.clone()).await;

                if let Err(e) = send_result {
                    error!(
                    "Could not send value to client {} because {}. Client will be disconnected.",
                    key, e
                );
                    value.job_queue.push_back(job);
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
                        let _ = server_data
                            .mdns_sender
                            .send(MessageToMdns::ConnectedService(service))
                            .await?;

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
                .await?;

                Ok(())
            } else {
                Err(anyhow!(
                    "TCP accepted client that is already connected: {}",
                    addr
                ))
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
                directories.insert(directory.signature.identifier, directory.clone());

                let _ = server_data.window_manager.emit_to(MAIN_WINDOW_LABEL, WindowAction::UpdateDirectory, directory.clone())?;

                return Ok(());
            }

            Err(anyhow!("Directory was already shared"))
        }

        MessageToServer::SynchronizeDirectories(directories, peer) => {
            let mut owned_dirs = server_data.server_handle.config.cached_data.lock().await;
            let mut clients = server_data.clients.lock().await;
            let myself = &server_data.server_handle.peer_id;

            let client = clients.iter_mut().find(|(_, cdata)| match &cdata.id {
                Some(pid) => pid == &peer,
                None => false,
            });

            match client {
                Some((_, _)) => {
                    for dir in directories {
                        let od = owned_dirs.get_mut(&dir.signature.identifier);

                        match od {
                            Some(matched_dir) => {
                                if dir.signature.last_modified > matched_dir.signature.last_modified {
                                    matched_dir.signature.shared_peers = dir.signature.shared_peers;

                                    if !matched_dir.signature.shared_peers.contains(&myself) {
                                        matched_dir.signature.shared_peers.push(server_data.server_handle.peer_id.clone());
                                    }

                                    let mut files_to_delete = vec![];
                                    for (file_id, file) in matched_dir.shared_files.iter_mut() {
                                        match dir.shared_files.get(file_id) {
                                            None => {
                                                if file.owned_peers.len() == 1 && file.owned_peers.contains(myself) {
                                                    files_to_delete.push(file_id.clone());
                                                }
                                            },
                                            Some(matched_file) => {
                                                file.owned_peers = matched_file.owned_peers.clone();

                                                if !file.owned_peers.contains(myself) {
                                                    file.owned_peers.push(myself.clone());
                                                }
                                            }
                                        }
                                    }

                                    let mut files_to_add = vec![];
                                    for (file_id, file) in dir.shared_files.iter() {
                                        if let None = matched_dir.shared_files.get(file_id) {
                                            files_to_add.push(file.clone());
                                        }
                                    }

                                    matched_dir.shared_files.retain(|file_id, _| {
                                        !files_to_delete.contains(&file_id)
                                    });

                                    for file in files_to_add {
                                        matched_dir.shared_files.insert(file.identifier, file);
                                    }
                                }
                            }
                            None => {
                                owned_dirs.insert(dir.signature.identifier, dir);
                            }
                        }
                    }

                    let _ = server_data.window_manager.emit_to(
                        MAIN_WINDOW_LABEL,
                        WindowAction::UpdateShareDirectories,
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

        MessageToServer::UpdatedDirectory(directory_id) => {
            let directories = server_data.server_handle.config.cached_data.lock().await;
            let directory = directories.get(&directory_id);

            if let Some(dir) = directory {
                let _ =
                    server_data
                        .window_manager
                        .emit_to(MAIN_WINDOW_LABEL, WindowAction::UpdateDirectory, dir)?;
            }

            Ok(())
        }
    }
}

async fn handle_request<'a>(msg: WindowRequest, server_data: ServerData<'a>) -> Result<()> {
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
                WindowAction::NewShareDirectory,
                signature,
            )?;

            Ok(())
        }

        WindowRequest::GetPeers(_) => {
            let clients = server_data.clients.lock().await;

            let ids: Vec<PeerId> = clients.iter().filter_map(|(_, c)| c.id.clone()).collect();
            let _ = server_data
                .window_manager
                .emit_to(MAIN_WINDOW_LABEL, WindowAction::GetPeers, ids)?;

            Ok(())
        }

        WindowRequest::GetAllShareDirectoryData(_) => {
            let data = server_data.server_handle.config.cached_data.lock().await;

            let values: Vec<ShareDirectory> = data.values().cloned().collect();

            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                WindowAction::UpdateShareDirectories,
                values,
            )?;

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

                dir.add_files(shared_files.clone(), Utc::now());

                let _ =
                    server_data
                        .window_manager
                        .emit_to(MAIN_WINDOW_LABEL, WindowAction::UpdateDirectory, dir.clone())?;

                server_data
                    .broadcast(
                        &dir.signature.shared_peers,
                        MessageFromServer::AddedFiles(dir.signature.clone(), shared_files),
                    )
                    .await;

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

            if let Some(dir) = directory {
                dir.signature.shared_peers.extend(peers);

                server_data.broadcast(&dir.signature.shared_peers, MessageFromServer::SendDirectories(vec![dir.clone()])).await;

                let _ = server_data.window_manager.emit_to(MAIN_WINDOW_LABEL, WindowAction::UpdateDirectory, dir.clone())?;

                // for (_, cval) in clients.iter() {
                //     if let Some(pid) = &cval.id {
                //         if dir.signature.shared_peers.contains(&pid) {
                //             cval.passive_sender
                //                 .send(MessageFromServer::SendDirectories(vec![dir.clone()]))
                //                 .await?
                //         }
                //     }
                // }

                return Ok(());
            }

            Err(anyhow!("Directory not found"))
        }

        WindowRequest::DeleteFile {
            directory_identifier,
            file_identifier,
        } => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;
            let dir_id = Uuid::from_str(&directory_identifier)?;
            let file_id = Uuid::from_str(&file_identifier)?;

            let directory = directories.get_mut(&dir_id);
            if let Some(dir) = directory {
                let file = dir.shared_files.get_mut(&file_id);
                let peers_to_send_to = match file {
                    None => return Ok(()),
                    Some(file) => {
                        match &file.content_location {
                            ContentLocation::LocalPath(path) => {
                                if path.exists() {
                                    fs::remove_file(path).await?;
                                }
                            }
                            _ => (),
                        }

                        let peers = file.owned_peers.clone();

                        dir.delete_files(&server_data.server_handle.peer_id, Utc::now(), vec![file_id.clone()]);

                        peers
                    }
                };

                server_data.broadcast(&peers_to_send_to, MessageFromServer::DeleteFile(server_data.server_handle.peer_id.clone(), dir.signature.clone(), file_id.clone())).await;

                let _ = server_data.window_manager.emit_to(MAIN_WINDOW_LABEL, WindowAction::UpdateDirectory, dir.clone())?;


                // if let Some(file) = file {
                    

                //     file.owned_peers
                //         .retain(|peer| peer != &server_data.server_handle.peer_id);
                //     file.content_location = ContentLocation::NetworkOnly;
                //     dir.signature.last_modified = Utc::now();

                //     server_data
                //         .broadcast(
                //             &file.owned_peers,
                //             MessageFromServer::DeleteFile(
                //                 server_data.server_handle.peer_id.clone(),
                //                 dir.signature.clone(),
                //                 file.identifier,
                //             ),
                //         )
                //         .await;

                //     let _ = server_data.window_manager.emit_to(MAIN_WINDOW_LABEL, WindowAction::UpdateDirectory, dir.clone())?;
                // }
            }

            Ok(())
        }

        WindowRequest::DownloadFile {
            directory_identifier,
            file_identifier,
        } => Ok(()),
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

    if pid.is_none() {
        job_queue.push_back(MessageFromServer::GetPeerId);
    }
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
