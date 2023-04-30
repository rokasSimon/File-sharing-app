use std::{
    cmp::Eq,
    collections::{HashMap, HashSet, VecDeque},
    fmt::Display,
    fs::File,
    hash::Hash,
    net::{IpAddr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
    vec,
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
    client_handle::{client_loop, ClientData, MessageFromServer, DownloadError},
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
    pub sender: mpsc::Sender<MessageFromServer>,
    pub join: JoinHandle<()>,
    pub service_info: Option<ServiceInfo>,
    pub job_queue: VecDeque<MessageFromServer>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub download_id: Uuid,
    pub file_identifier: Uuid,
    pub directory_identifier: Uuid,
    pub progress: u64,
    pub file_name: String,
    pub file_path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DownloadUpdate {
    pub progress: u64,
    pub download_id: Uuid,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DownloadCanceled {
    pub reason: String,
    pub download_id: Uuid,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct DownloadNotStarted {
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BackendError {
    pub error: String,
    pub title: String,
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

    StartedDownload {
        download_info: Download
    },
    FinishedDownload {
        download_id: Uuid,
        directory_identifier: Uuid,
        file_identifier: Uuid,
    },
    DownloadUpdate {
        download_id: Uuid,
        new_progress: u64,
    },
    CanceledDownload {
        download_id: Uuid,
        cancel_reason: String
    },

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
            let _ = c.sender.send(msg.clone()).await;
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
    pub const UPDATE_DIRECTORY: &str = "UpdateDirectory";
    pub const UPDATE_SHARE_DIRECTORIES: &str = "UpdateShareDirectories";
    pub const GET_PEERS: &str = "GetPeers";
    pub const NEW_SHARE_DIRECTORY: &str = "NewShareDirectory";
    pub const ERROR: &str = "Error";
    pub const DOWNLOAD_STARTED: &str = "DownloadStarted";
    pub const DOWNLOAD_UPDATE: &str = "DownloadUpdate";
    pub const DOWNLOAD_CANCELED: &str = "CanceledDownload";
}

pub async fn server_loop(
    window_manager: AppHandle,
    mut client_receiver: mpsc::Receiver<MessageToServer>,
    mut window_receiver: mpsc::Receiver<WindowRequest>,
    mdns_sender: mpsc::Sender<MessageToMdns>,
    server_handle: ServerHandle,
) {
    let mut clients: Mutex<HashMap<ClientConnectionId, ClientHandle>> = Mutex::new(HashMap::new());
    let mut job_interval = tokio::time::interval(Duration::from_secs(UPDATE_PERIOD));

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
            _ = job_interval.tick() => {
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
                let send_result = value.sender.send(job.clone()).await;

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
            let mut peer_ids: Vec<PeerId> = clients.iter().filter_map(|(_, c)| c.id.clone()).collect();
            let client = clients.get_mut(&addr);

            match client {
                Some(client) => {
                    client.id = Some(id.clone());
                    peer_ids.push(id);

                    let _ = server_data.window_manager.emit_to(MAIN_WINDOW_LABEL, WindowAction::GET_PEERS, peer_ids)?;
                    let _ = client.sender.send(MessageFromServer::Synchronize).await?;

                    Ok(())
                }
                None => Err(anyhow!("No such client for {}", addr)),
            }
        }

        MessageToServer::KillClient(client_addr) => {
            let mut clients = server_data.clients.lock().await;
            let mut peer_ids: Vec<PeerId> = clients.iter().filter_map(|(_, c)| c.id.clone()).collect();
            let client = clients.remove(&client_addr);

            match client {
                Some(client) => {
                    let disconnected_peer_id = client.id.clone();
                    disconnected_client(client, server_data.mdns_sender).await;

                    match disconnected_peer_id {
                        None => (),
                        Some(id) => {
                            peer_ids.retain(|peer| peer != &id);
                        }
                    }

                    let _ = server_data.window_manager.emit_to(MAIN_WINDOW_LABEL, WindowAction::GET_PEERS, peer_ids)?;

                    Ok(())
                }
                None => Err(anyhow!("No such client to drop: {}", client_addr)),
            }
        }

        MessageToServer::SharedDirectory(directory) => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;

            if !directories.contains_key(&directory.signature.identifier) {
                directories.insert(directory.signature.identifier, directory.clone());

                let _ = server_data.window_manager.emit_to(
                    MAIN_WINDOW_LABEL,
                    WindowAction::UPDATE_DIRECTORY,
                    directory.clone(),
                )?;

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
                                if dir.signature.last_modified > matched_dir.signature.last_modified
                                {
                                    matched_dir.signature.shared_peers = dir.signature.shared_peers;

                                    if !matched_dir.signature.shared_peers.contains(&myself) {
                                        matched_dir
                                            .signature
                                            .shared_peers
                                            .push(server_data.server_handle.peer_id.clone());
                                    }

                                    let mut files_to_delete = vec![];
                                    for (file_id, file) in matched_dir.shared_files.iter_mut() {
                                        if let None = dir.shared_files.get(file_id) {
                                            if !file.owned_peers.contains(myself) {
                                                files_to_delete.push(file_id.clone());
                                            }
                                        }
                                    }

                                    let mut files_to_add = vec![];
                                    for (file_id, file) in dir.shared_files.iter() {
                                        match matched_dir.shared_files.get_mut(file_id) {
                                            None => files_to_add.push(file.clone()),
                                            Some(matched_file) => {
                                                matched_file.owned_peers = file.owned_peers.clone();
                                            }
                                        }
                                    }

                                    matched_dir
                                        .shared_files
                                        .retain(|file_id, _| !files_to_delete.contains(&file_id));

                                    for file in files_to_add {
                                        matched_dir.shared_files.insert(file.identifier, file);
                                    }
                                } else {
                                    info!(
                                        "Received older directory signature: {} | {} < {}",
                                        matched_dir.signature.identifier,
                                        dir.signature.last_modified,
                                        matched_dir.signature.last_modified
                                    );
                                }
                            }
                            None => {
                                owned_dirs.insert(dir.signature.identifier, dir);
                            }
                        }
                    }

                    let _ = server_data.window_manager.emit_to(
                        MAIN_WINDOW_LABEL,
                        WindowAction::UPDATE_SHARE_DIRECTORIES,
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
                let _ = server_data.window_manager.emit_to(
                    MAIN_WINDOW_LABEL,
                    WindowAction::UPDATE_DIRECTORY,
                    dir,
                )?;
            }

            Ok(())
        }

        MessageToServer::StartedDownload {
            download_info
        } => {
            
            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                WindowAction::DOWNLOAD_STARTED,
                download_info,
            )?;

            Ok(())
        }

        MessageToServer::FinishedDownload { download_id, directory_identifier, file_identifier } => {
            let myself = server_data.server_handle.peer_id.clone();
            let directories = server_data.server_handle.config.cached_data.lock().await;
            let directory = directories.get(&directory_identifier);

            match directory {
                None => {
                    let _ = server_data.window_manager.emit_to(
                        MAIN_WINDOW_LABEL,
                        WindowAction::DOWNLOAD_CANCELED,
                        DownloadCanceled {
                            download_id,
                            reason: "Could not update other clients.".to_owned()
                        },
                    )?;
        
                    Ok(())
                },
                Some(directory) => {
                    server_data.broadcast(&directory.signature.shared_peers, MessageFromServer::UpdateOwners { peer_id: myself, directory_identifier, file_identifier }).await;

                    let _ = server_data.window_manager.emit_to(
                        MAIN_WINDOW_LABEL,
                        WindowAction::DOWNLOAD_UPDATE,
                        DownloadUpdate {
                            download_id,
                            progress: 100
                        },
                    )?;
        
                    Ok(())
                }
            }
        }

        MessageToServer::DownloadUpdate {
            download_id,
            new_progress,
        } => {

            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                WindowAction::DOWNLOAD_UPDATE,
                DownloadUpdate {
                    download_id,
                    progress: new_progress
                },
            )?;

            Ok(())
        }

        MessageToServer::CanceledDownload {
            download_id,
            cancel_reason,
        } => {
            
            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                WindowAction::DOWNLOAD_CANCELED,
                DownloadCanceled {
                    download_id,
                    reason: cancel_reason
                },
            )?;

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
                WindowAction::NEW_SHARE_DIRECTORY,
                signature,
            )?;

            Ok(())
        }

        WindowRequest::GetPeers(_) => {
            let clients = server_data.clients.lock().await;

            let ids: Vec<PeerId> = clients.iter().filter_map(|(_, c)| c.id.clone()).collect();
            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                WindowAction::GET_PEERS,
                ids,
            )?;

            Ok(())
        }

        WindowRequest::GetAllShareDirectoryData(_) => {
            let data = server_data.server_handle.config.cached_data.lock().await;

            let values: Vec<ShareDirectory> = data.values().cloned().collect();

            let _ = server_data.window_manager.emit_to(
                MAIN_WINDOW_LABEL,
                WindowAction::UPDATE_SHARE_DIRECTORIES,
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

                let _ = server_data.window_manager.emit_to(
                    MAIN_WINDOW_LABEL,
                    WindowAction::UPDATE_DIRECTORY,
                    dir.clone(),
                )?;

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

                server_data
                    .broadcast(
                        &dir.signature.shared_peers,
                        MessageFromServer::SendDirectories(vec![dir.clone()]),
                    )
                    .await;

                let _ = server_data.window_manager.emit_to(
                    MAIN_WINDOW_LABEL,
                    WindowAction::UPDATE_DIRECTORY,
                    dir.clone(),
                )?;

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
                if let Some(file) = file {
                    match &file.content_location {
                        ContentLocation::LocalPath(path) => {
                            if path.exists() {
                                fs::remove_file(path).await?;
                            }
                        }
                        _ => (),
                    }

                    dir.delete_files(
                        &server_data.server_handle.peer_id,
                        Utc::now(),
                        vec![file_id.clone()],
                    );

                    server_data
                        .broadcast(
                            &dir.signature.shared_peers,
                            MessageFromServer::DeleteFile(
                                server_data.server_handle.peer_id.clone(),
                                dir.signature.clone(),
                                file_id.clone(),
                            ),
                        )
                        .await;

                    let _ = server_data.window_manager.emit_to(
                        MAIN_WINDOW_LABEL,
                        WindowAction::UPDATE_DIRECTORY,
                        dir.clone(),
                    )?;
                }
            }

            Ok(())
        }

        WindowRequest::DownloadFile {
            directory_identifier,
            file_identifier,
        } => {
            let directories = server_data.server_handle.config.cached_data.lock().await;
            let dir_id = Uuid::parse_str(&directory_identifier)?;
            let file_id = Uuid::parse_str(&file_identifier)?;

            let files = get_file(&*directories, dir_id, file_id);
            let result = match files {
                None => Err(DownloadError::DirectoryMissing),
                Some((_, file)) => {
                    let mut clients = server_data.clients.lock().await;
                    let client = clients.iter_mut().find(|(_, c)| {
                        if let Some(id) = &c.id {
                            return file.owned_peers.contains(id);
                        }

                        return false;
                    });

                    match client {
                        None => Err(DownloadError::NoClientsConnected),
                        Some((_, c)) => {
                            let download_id = Uuid::new_v4();
                            let app_config =
                                server_data.server_handle.config.app_config.lock().await;

                            let download_directory = app_config.download_directory.clone();
                            let file_path = download_directory.join(&file.name);
                            let file_path = if file_path.exists() {
                                download_directory.join(download_id.to_string())
                            } else {
                                file_path
                            };

                            let _ = c
                                .sender
                                .send(MessageFromServer::StartDownload {
                                    download_id,
                                    file_identifier: file_id,
                                    directory_identifier: dir_id,
                                    destination: file_path,
                                })
                                .await?;

                            Ok(())
                        }
                    }
                }
            };

            if let Err(e) = result {
                let _ = server_data.window_manager.emit_to(
                    MAIN_WINDOW_LABEL,
                    WindowAction::DOWNLOAD_CANCELED,
                    DownloadNotStarted {
                        reason: e.to_string(),
                    },
                )?;
            }

            Ok(())
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

    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    let client_data = ClientData {
        server: server_handle,
        receiver,
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
        sender,
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

fn get_file(
    directories: &HashMap<Uuid, ShareDirectory>,
    dir_id: Uuid,
    file_id: Uuid,
) -> Option<(&ShareDirectory, &SharedFile)> {
    let directory = directories.get(&dir_id);

    match directory {
        None => None,
        Some(directory) => {
            let file = directory.shared_files.get(&file_id);

            match file {
                None => None,
                Some(file) => Some((directory, file)),
            }
        }
    }
}

fn update_downloaded_file(directories: &mut HashMap<Uuid, ShareDirectory>, myself: &PeerId, download: &Download) -> Result<Vec<PeerId>, BackendError> {
    let directory = directories.get_mut(&download.directory_identifier);

    match directory {
        None => Err(BackendError {
            error: format!(
                "Directory {} does not exist.",
                download.directory_identifier
            ),
            title: "Download Failure".to_owned(),
        }),
        Some(directory) => {
            let file = directory.shared_files.get_mut(&download.file_identifier);

            match file {
                None => Err(BackendError {
                    error: format!("File {} does not exist.", download.file_name),
                    title: "Download Failure".to_owned(),
                }),
                Some(file) => {
                    file.owned_peers.push(myself.clone());
                    file.content_location = ContentLocation::LocalPath(download.file_path.clone());

                    Ok(file.owned_peers.clone())
                }
            }
        }
    }
}
