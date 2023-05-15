use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use cryptohelpers::crc::compute_stream;
use mdns_sd::ServiceInfo;
use tauri::async_runtime::{JoinHandle, Mutex};
use tokio::{
    fs,
    net::TcpStream,
    sync::{mpsc, MutexGuard},
};
use uuid::Uuid;

use crate::{
    client::{client_loop, ClientData, DownloadError},
    config::StoredConfig,
    data::{ContentLocation, PeerId, ShareDirectory, ShareDirectorySignature, SharedFile},
    mdns::MessageToMdns,
    window::{
        BackendError, Download, DownloadCanceled, DownloadUpdate, WindowManager, WindowRequest,
        WindowResponse,
    },
};

pub type ClientConnectionId = IpAddr;

const CHANNEL_SIZE: usize = 16;
const UPDATE_PERIOD: u64 = 5;

#[derive(Clone)]
pub struct ServerHandle {
    pub channel: mpsc::Sender<MessageToServer>,
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

#[derive(Debug)]
pub enum MessageToServer {
    SetPeerId(ClientConnectionId, PeerId),
    ServiceFound(ServiceInfo),
    ConnectionAccepted(TcpStream, SocketAddr),
    KillClient(ClientConnectionId),

    SynchronizeDirectories(Vec<ShareDirectory>, PeerId),
    UpdatedDirectory(Uuid),

    StartedDownload {
        download_info: Download,
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
        cancel_reason: String,
    },

    SharedDirectory(ShareDirectory),
}

#[derive(Debug, Clone)]
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
    CancelDownload {
        download_id: Uuid,
    },
    UpdateOwners {
        peer_id: PeerId,
        directory_identifier: Uuid,
        file_identifier: Uuid,
        date_modified: DateTime<Utc>,
    },

    LeftDirectory {
        directory_identifier: Uuid,
    },
}

struct ServerData<'a, M>
where
    M: WindowManager,
{
    window_manager: &'a M,
    server_handle: &'a ServerHandle,
    clients: &'a mut HashMap<ClientConnectionId, ClientHandle>,
    mdns_sender: &'a mpsc::Sender<MessageToMdns>,
}

impl<M> ServerData<'_, M>
where
    M: WindowManager,
{
    pub async fn broadcast(&self, peers: &Vec<PeerId>, msg: MessageFromServer) {
        let found_clients: Vec<_> = self
            .clients
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

pub async fn server_loop<M>(
    window_manager: M,
    mut client_receiver: mpsc::Receiver<MessageToServer>,
    mut window_receiver: mpsc::Receiver<WindowResponse>,
    mdns_sender: mpsc::Sender<MessageToMdns>,
    server_handle: ServerHandle,
) where
    M: WindowManager,
{
    let mut clients: HashMap<ClientConnectionId, ClientHandle> = HashMap::new();
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

async fn do_periodic_work<'a, M>(server_data: ServerData<'a, M>)
where
    M: WindowManager,
{
    let mut clients = server_data.clients;
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

async fn handle_message<'a, M>(msg: MessageToServer, mut server_data: ServerData<'a, M>) -> Result<()>
where
    M: WindowManager,
{
    match msg {
        MessageToServer::ServiceFound(service) => {
            let ip_addr = service.get_addresses().iter().next();

            match ip_addr {
                Some(ip) => {
                    let ipv4 = IpAddr::V4(*ip);
                    let socket_addr = SocketAddr::V4(SocketAddrV4::new(*ip, service.get_port()));

                    if !server_data.clients.contains_key(&ipv4) {
                        let tcp_stream = TcpStream::connect(socket_addr).await?;

                        add_client(
                            server_data.server_handle.clone(),
                            &mut server_data.clients,
                            tcp_stream,
                            ipv4,
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

            if !server_data.clients.contains_key(&ip_addr) {
                add_client(
                    server_data.server_handle.clone(),
                    &mut server_data.clients,
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
            let mut clients = server_data.clients;
            let mut peer_ids: Vec<PeerId> =
                clients.iter().filter_map(|(_, c)| c.id.clone()).collect();
            let client = clients.get_mut(&addr);

            match client {
                Some(client) => {
                    client.id = Some(id.clone());
                    peer_ids.push(id);

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::GetPeers(peer_ids));
                    let _ = client.sender.send(MessageFromServer::Synchronize).await?;

                    Ok(())
                }
                None => Err(anyhow!("No such client for {}", addr)),
            }
        }

        MessageToServer::KillClient(client_addr) => {
            let mut clients = server_data.clients;
            let mut peer_ids: Vec<PeerId> =
                clients.iter().filter_map(|(_, c)| c.id.clone()).collect();
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

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::GetPeers(peer_ids));

                    Ok(())
                }
                None => Err(anyhow!("No such client to drop: {}", client_addr)),
            }
        }

        MessageToServer::SharedDirectory(directory) => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;

            if !directories.contains_key(&directory.signature.identifier) {
                directories.insert(directory.signature.identifier, directory.clone());

                let _ = server_data
                    .window_manager
                    .send(WindowRequest::UpdateDirectory(directory.clone()));

                return Ok(());
            }

            Err(anyhow!("Directory was already shared"))
        }

        MessageToServer::SynchronizeDirectories(directories, peer) => {
            let mut owned_dirs = server_data.server_handle.config.cached_data.lock().await;
            let mut clients = server_data.clients;
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
                                }
                            }
                            None => {
                                owned_dirs.insert(dir.signature.identifier, dir);
                            }
                        }
                    }

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::UpdateShareDirectories(
                            owned_dirs
                                .values()
                                .cloned()
                                .collect::<Vec<ShareDirectory>>(),
                        ));

                    Ok(())
                }
                None => Err(anyhow!("No client found")),
            }
        }

        MessageToServer::UpdatedDirectory(directory_id) => {
            let directories = server_data.server_handle.config.cached_data.lock().await;
            let directory = directories.get(&directory_id);

            if let Some(dir) = directory {
                let _ = server_data
                    .window_manager
                    .send(WindowRequest::UpdateDirectory(dir.clone()));
            }

            Ok(())
        }

        MessageToServer::StartedDownload { download_info } => {
            let _ = server_data
                .window_manager
                .send(WindowRequest::DownloadStarted(download_info));

            Ok(())
        }

        MessageToServer::FinishedDownload {
            download_id,
            directory_identifier,
            file_identifier,
        } => {
            let myself = server_data.server_handle.peer_id.clone();
            let mut directories = server_data.server_handle.config.cached_data.lock().await;
            let directory = directories.get_mut(&directory_identifier);

            match directory {
                None => {
                    let msg = WindowRequest::DownloadCanceled(DownloadCanceled {
                        reason: "Could not update other clients.".to_owned(),
                        download_id,
                    });
                    let _ = server_data.window_manager.send(msg);

                    Ok(())
                }
                Some(directory) => {
                    server_data
                        .broadcast(
                            &directory.signature.shared_peers,
                            MessageFromServer::UpdateOwners {
                                peer_id: myself,
                                directory_identifier,
                                file_identifier,
                                date_modified: directory.signature.last_modified,
                            },
                        )
                        .await;

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::UpdateDirectory(directory.clone()));

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::DownloadUpdate(DownloadUpdate {
                            download_id,
                            progress: 100,
                        }));

                    Ok(())
                }
            }
        }

        MessageToServer::DownloadUpdate {
            download_id,
            new_progress,
        } => {
            let _ = server_data
                .window_manager
                .send(WindowRequest::DownloadUpdate(DownloadUpdate {
                    download_id,
                    progress: new_progress,
                }));

            Ok(())
        }

        MessageToServer::CanceledDownload {
            download_id,
            cancel_reason,
        } => {
            let _ = server_data
                .window_manager
                .send(WindowRequest::DownloadCanceled(DownloadCanceled {
                    download_id,
                    reason: cancel_reason,
                }));

            Ok(())
        }
    }
}

async fn handle_request<'a, M>(msg: WindowResponse, server_data: ServerData<'a, M>) -> Result<()>
where
    M: WindowManager,
{
    match msg {
        WindowResponse::CreateShareDirectory(name) => {
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
            let _ = server_data
                .window_manager
                .send(WindowRequest::NewShareDirectory(signature));

            Ok(())
        }

        WindowResponse::GetPeers(_) => {
            let clients = server_data.clients;

            let ids: Vec<PeerId> = clients.iter().filter_map(|(_, c)| c.id.clone()).collect();

            let _ = server_data
                .window_manager
                .send(WindowRequest::GetPeers(ids));

            Ok(())
        }

        WindowResponse::GetAllShareDirectoryData(_) => {
            let data = server_data.server_handle.config.cached_data.lock().await;

            let values: Vec<ShareDirectory> = data.values().cloned().collect();

            let _ = server_data
                .window_manager
                .send(WindowRequest::UpdateShareDirectories(values));

            Ok(())
        }

        WindowResponse::LeaveDirectory {
            directory_identifier,
        } => {
            let mut directories = server_data.server_handle.config.cached_data.lock().await;
            let dir_id = Uuid::parse_str(&directory_identifier)?;
            let directory = directories.remove(&dir_id);
            let directory = match directory {
                None => {
                    return Err(anyhow!(
                        "User is trying to leave directory that does not exist"
                    ))
                }
                Some(dir) => dir,
            };

            server_data
                .broadcast(
                    &directory.signature.shared_peers,
                    MessageFromServer::LeftDirectory {
                        directory_identifier: dir_id,
                    },
                )
                .await;

            let directories_data = directories
                .values()
                .cloned()
                .collect::<Vec<ShareDirectory>>();

            let _ = server_data
                .window_manager
                .send(WindowRequest::UpdateShareDirectories(directories_data));

            Ok(())
        }

        WindowResponse::AddFiles {
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

                let add_result = dir.add_files(shared_files.clone(), Utc::now());

                if let Err(_) = add_result {
                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::Error(BackendError {
                            title: "File Error".to_owned(),
                            error: "File has already been added to this directory".to_owned(),
                        }));

                    return Ok(());
                }

                let _ = server_data
                    .window_manager
                    .send(WindowRequest::UpdateDirectory(dir.clone()));

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

        WindowResponse::ShareDirectoryToPeers {
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

                let _ = server_data
                    .window_manager
                    .send(WindowRequest::UpdateDirectory(dir.clone()));

                return Ok(());
            }

            Err(anyhow!("Directory not found"))
        }

        WindowResponse::DeleteFile {
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

                    file.content_location = ContentLocation::NetworkOnly;
                    dir.remove_files(
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

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::UpdateDirectory(dir.clone()));
                }
            }

            Ok(())
        }

        WindowResponse::DownloadFile {
            directory_identifier,
            file_identifier,
        } => {
            let directories = server_data.server_handle.config.cached_data.lock().await;
            let dir_id = Uuid::parse_str(&directory_identifier)?;
            let file_id = Uuid::parse_str(&file_identifier)?;

            let files = get_file(&*directories, dir_id, file_id);
            let result = match files {
                None => {
                    error!("Directory missing {}", dir_id);
                    Err(DownloadError::DirectoryMissing)
                }
                Some((_, file)) => {
                    let mut clients = server_data.clients;
                    let client = clients.iter_mut().find(|(_, c)| {
                        if let Some(id) = &c.id {
                            return file.owned_peers.contains(id);
                        }

                        return false;
                    });

                    match client {
                        None => {
                            error!("Clients to download from not found");
                            Err(DownloadError::NoClientsConnected)
                        }
                        Some((_, c)) => {
                            let download_id = Uuid::new_v4();
                            let app_config =
                                server_data.server_handle.config.app_config.lock().await;

                            let download_directory = app_config.download_directory.clone();
                            let file_path = download_directory.join(&file.name);
                            let file_path = if file_path.exists() {
                                file_path.join(download_id.to_string())
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

            // if let Err(e) = result {
            //     error!("{}", e);

            //     let _ = server_data.window_manager.send(WindowRequest::DownloadCanceled(DownloadNotStarted {
            //         reason: e.to_string(),
            //     }));
            // }

            Ok(())
        }

        WindowResponse::CancelDownload {
            download_identifier,
            peer,
        } => {
            let download_id = Uuid::parse_str(&download_identifier)?;
            let peers = vec![peer];
            server_data
                .broadcast(&peers, MessageFromServer::CancelDownload { download_id })
                .await;

            let _ = server_data
                .window_manager
                .send(WindowRequest::DownloadCanceled(DownloadCanceled {
                    download_id,
                    reason: DownloadError::Canceled.to_string(),
                }));

            Ok(())
        }
    }
}

async fn add_client<'a>(
    server_handle: ServerHandle,
    clients: &mut HashMap<IpAddr, ClientHandle>,
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

#[cfg(tests)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use futures::{SinkExt, StreamExt};
    use mdns_sd::ServiceInfo;
    use tauri::AppHandle;
    use tokio::{join, net::TcpListener, sync::mpsc};
    use tokio_util::codec::{FramedRead, FramedWrite};
    use uuid::Uuid;

    use crate::{
        config::{AppConfig, StoredConfig},
        network::{
            codec::{MessageCodec, TcpMessage},
            mdns::MessageToMdns,
        },
        peer_id::PeerId,
        window::WindowManager,
    };

    use super::{server_loop, MessageToServer, ServerHandle, WindowRequest};

    struct MockWindowManager;

    impl WindowManager for MockWindowManager {
        fn send<P>(
            &self,
            action: crate::window::WindowAction,
            payload: P,
        ) -> Result<(), tauri::Error>
        where
            P: serde::Serialize + Clone,
        {
            Ok(())
        }
    }

    fn setup_config() -> Arc<StoredConfig> {
        let config = StoredConfig::new(AppConfig::default(), HashMap::new());

        Arc::new(config)
    }

    fn setup_server() -> (
        ServerHandle,
        mpsc::Receiver<MessageToMdns>,
        mpsc::Sender<WindowRequest>,
        tokio::task::JoinHandle<()>,
    ) {
        let config = setup_config();
        let (server_sender, server_receiver) = mpsc::channel(10);
        let (window_sender, window_receiver) = mpsc::channel(10);
        let (mdns_sender, mdns_receiver) = mpsc::channel(10);

        let server = ServerHandle {
            config: config.clone(),
            channel: server_sender,
            peer_id: PeerId {
                hostname: "test 1".to_string(),
                uuid: Uuid::nil(),
            },
        };

        let server_loop_handle = tokio::spawn(server_loop(
            MockWindowManager {},
            server_receiver,
            window_receiver,
            mdns_sender,
            server.clone(),
        ));

        (server, mdns_receiver, window_sender, server_loop_handle)
    }

    #[tokio::test]
    async fn message_to_server_handling() {
        let (server, mdns_recv, window_send, handle) = setup_server();
        let client_conn = TcpListener::bind("127.0.0.1:6000").await.unwrap();
        let ip = "127.0.0.1";
        let service = ServiceInfo::new(
            "_test._tcp.local.",
            "test",
            "test_host",
            ip,
            6001,
            Some(HashMap::from([("port".to_string(), "6000".to_string())])),
        );

        let connect_task = server
            .channel
            .send(MessageToServer::ServiceFound(service.unwrap()));
        let accept_task = client_conn.accept();

        let (_, b) = join!(connect_task, accept_task);

        let (mut stream, addr) = b.unwrap();
        let (read, write) = stream.split();
        let mut client_read = FramedRead::new(read, MessageCodec {});
        let mut client_write = FramedWrite::new(write, MessageCodec {});

        let _ = client_write.send(TcpMessage::RequestPeerId).await;
        let res = client_read.next().await;

        println!("{:?}", addr);
        println!("{:?}", res);

        assert!(true);
    }
}