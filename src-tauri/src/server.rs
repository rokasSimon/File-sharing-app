use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use cryptohelpers::crc::compute_stream;
use mdns_sd::ServiceInfo;
use tauri::async_runtime::JoinHandle;
use tokio::{net::TcpStream, sync::mpsc};
use uuid::Uuid;

use crate::{
    client::{client_loop, ClientData, DownloadError, MessageToClient},
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

#[derive(Clone)]
pub struct ServerHandle {
    pub channel: mpsc::Sender<MessageToServer>,
    pub peer_id: PeerId,
}

pub struct ClientHandle {
    pub id: Option<PeerId>,
    pub sender: mpsc::Sender<MessageToClient>,
    pub join: JoinHandle<()>,
    pub service_info: Option<ServiceInfo>,
}

#[derive(Debug)]
pub enum MessageToServer {
    SetPeerId(ClientConnectionId, PeerId),
    ServiceFound(ServiceInfo),
    ConnectionAccepted(TcpStream, SocketAddr),
    KillClient(ClientConnectionId),

    LeftDirectory {
        directory_identifier: Uuid,
        peer_id: PeerId,
        date_modified: DateTime<Utc>,
    },

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

struct ServerData<'a, M>
where
    M: WindowManager,
{
    window_manager: &'a M,
    server_handle: &'a ServerHandle,
    clients: &'a mut HashMap<ClientConnectionId, ClientHandle>,
    mdns_sender: &'a mpsc::Sender<MessageToMdns>,
    config: &'a Arc<StoredConfig>,
}

impl<M> ServerData<'_, M>
where
    M: WindowManager,
{
    pub async fn broadcast(&self, peers: &[PeerId], msg: MessageToClient) {
        let found_clients: Vec<_> = self
            .clients
            .iter()
            .filter(|(_, c)| match &c.id {
                Some(id) => peers.contains(id),
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
    config: Arc<StoredConfig>,
) where
    M: WindowManager,
{
    let mut clients: HashMap<ClientConnectionId, ClientHandle> = HashMap::new();

    loop {
        let server_data = ServerData {
            window_manager: &window_manager,
            server_handle: &server_handle,
            clients: &mut clients,
            mdns_sender: &mdns_sender,
            config: &config,
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
        }
    }
}

async fn handle_message<'a, M>(msg: MessageToServer, server_data: ServerData<'_, M>) -> Result<()>
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
                            server_data.clients,
                            tcp_stream,
                            ipv4,
                            Some(service.clone()),
                            server_data.config.clone(),
                        )
                        .await?;

                        server_data
                            .mdns_sender
                            .send(MessageToMdns::ConnectedService(service))
                            .await?;

                        Ok(())
                    } else {
                        server_data
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
                    server_data.clients,
                    tcp,
                    ip_addr,
                    None,
                    server_data.config.clone(),
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
            let clients = server_data.clients;
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
                    client.sender.send(MessageToClient::Synchronize).await?;

                    Ok(())
                }
                None => Err(anyhow!("No such client for {}", addr)),
            }
        }

        MessageToServer::KillClient(client_addr) => {
            let clients = server_data.clients;
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
            server_data
                .config
                .shared_directory(directory.clone())
                .await?;

            let _ = server_data
                .window_manager
                .send(WindowRequest::UpdateDirectory(directory));

            Ok(())
        }

        MessageToServer::SynchronizeDirectories(directories, peer) => {
            let clients = server_data.clients;
            let myself = &server_data.server_handle.peer_id;

            let client = clients.iter_mut().find(|(_, cdata)| match &cdata.id {
                Some(pid) => pid == &peer,
                None => false,
            });

            match client {
                Some((_, _)) => {
                    let new_dirs = server_data.config.synchronize(directories, myself).await;

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::UpdateShareDirectories(new_dirs));

                    Ok(())
                }
                None => Err(anyhow!("No client found")),
            }
        }

        MessageToServer::UpdatedDirectory(directory_id) => {
            let dir = server_data.config.get_directory(directory_id).await;

            if let Some(dir) = dir {
                let _ = server_data
                    .window_manager
                    .send(WindowRequest::UpdateDirectory(dir));
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
            let directory = server_data.config.get_directory(directory_identifier).await;

            match directory {
                None => {
                    let msg = WindowRequest::DownloadCanceled(DownloadCanceled {
                        reason: "Could not update other clients.".to_owned(),
                        download_id,
                    });
                    let _ = server_data.window_manager.send(msg);
                }
                Some(directory) => {
                    server_data
                        .broadcast(
                            &directory.signature.shared_peers,
                            MessageToClient::UpdateOwners {
                                peer_id: myself,
                                directory_identifier,
                                file_identifier,
                                date_modified: directory.signature.last_modified,
                            },
                        )
                        .await;

                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::UpdateDirectory(directory));
                    let _ = server_data
                        .window_manager
                        .send(WindowRequest::DownloadUpdate(DownloadUpdate {
                            progress: 100,
                            download_id,
                        }));
                }
            }

            Ok(())
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

        MessageToServer::LeftDirectory {
            directory_identifier,
            peer_id,
            date_modified,
        } => {
            server_data
                .config
                .mutate_dir(directory_identifier, |dir| {
                    dir.remove_peer(&peer_id, date_modified);
                })
                .await;

            let dir = server_data.config.get_directory(directory_identifier).await;

            if let Some(dir) = dir {
                let _ = server_data
                    .window_manager
                    .send(WindowRequest::UpdateDirectory(dir));
            }

            Ok(())
        }
    }
}

async fn handle_request<M>(msg: WindowResponse, server_data: ServerData<'_, M>) -> Result<()>
where
    M: WindowManager,
{
    match msg {
        WindowResponse::CreateShareDirectory(name) => {
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

            server_data.config.add_directory(sd).await;

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
            let _ = server_data
                .window_manager
                .send(WindowRequest::UpdateShareDirectories(
                    server_data.config.get_directories().await,
                ));

            Ok(())
        }

        WindowResponse::LeaveDirectory {
            directory_identifier,
        } => {
            let dir_id = Uuid::parse_str(&directory_identifier)?;
            let directory = server_data.config.remove_directory(dir_id).await;
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
                    MessageToClient::LeftDirectory {
                        directory_identifier: dir_id,
                    },
                )
                .await;

            let _ = server_data
                .window_manager
                .send(WindowRequest::UpdateShareDirectories(
                    server_data.config.get_directories().await,
                ));

            Ok(())
        }

        WindowResponse::AddFiles {
            file_paths,
            directory_identifier,
        } => {
            let id = Uuid::from_str(&directory_identifier)?;
            let mut shared_files = vec![];
            for file_path in file_paths {
                let shared_file =
                    create_shared_file(file_path, &server_data.server_handle.peer_id).await?;

                shared_files.push(shared_file);
            }

            let mut signature = None;
            server_data
                .config
                .mutate_dir(id, |directory| {
                    let add_result = directory.add_files(shared_files.clone(), Utc::now());

                    if let Ok(()) = add_result {
                        let _ = server_data
                            .window_manager
                            .send(WindowRequest::UpdateDirectory(directory.clone()));

                        signature = Some(directory.signature.clone());
                    }
                })
                .await;

            if let Some(signature) = signature {
                server_data
                    .broadcast(
                        &signature.shared_peers,
                        MessageToClient::AddedFiles(signature.clone(), shared_files),
                    )
                    .await;
            } else {
                let _ = server_data
                    .window_manager
                    .send(WindowRequest::Error(BackendError {
                        title: "File Error".to_owned(),
                        error: "File has already been added to this directory".to_owned(),
                    }));
            }

            Ok(())
        }

        WindowResponse::ShareDirectoryToPeers {
            peers,
            directory_identifier,
        } => {
            let id = Uuid::from_str(&directory_identifier)?;
            let mut success = false;
            server_data
                .config
                .mutate_dir(id, |dir| {
                    dir.add_peers(peers, Utc::now());

                    success = true;
                })
                .await;

            if success {
                let dir = server_data.config.get_directory(id).await.unwrap();

                server_data
                    .broadcast(
                        &dir.signature.shared_peers,
                        MessageToClient::SendDirectories(vec![dir.clone()]),
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
            let dir_id = Uuid::from_str(&directory_identifier)?;
            let file_id = Uuid::from_str(&file_identifier)?;

            let mut success_delete = false;
            server_data
                .config
                .mutate_file(dir_id, file_id, |file| {
                    if let ContentLocation::LocalPath(path) = &file.content_location {
                        if path.exists() {
                            let _ = std::fs::remove_file(path);
                        }
                    }

                    file.content_location = ContentLocation::NetworkOnly;
                    success_delete = true;
                })
                .await;

            if success_delete {
                let mut success_remove = false;
                server_data
                    .config
                    .mutate_dir(dir_id, |dir| {
                        dir.remove_files(
                            &server_data.server_handle.peer_id,
                            Utc::now(),
                            vec![file_id],
                        );

                        success_remove = true;
                    })
                    .await;

                if success_remove {
                    let dir = server_data.config.get_directory(dir_id).await;

                    if let Some(dir) = dir {
                        server_data
                            .broadcast(
                                &dir.signature.shared_peers,
                                MessageToClient::DeleteFile(
                                    server_data.server_handle.peer_id.clone(),
                                    dir.signature.clone(),
                                    file_id,
                                ),
                            )
                            .await;

                        let _ = server_data
                            .window_manager
                            .send(WindowRequest::UpdateDirectory(dir.clone()));
                    }
                }
            }

            Ok(())
        }

        WindowResponse::DownloadFile {
            directory_identifier,
            file_identifier,
        } => {
            let dir_id = Uuid::parse_str(&directory_identifier)?;
            let file_id = Uuid::parse_str(&file_identifier)?;

            let owners = server_data.config.get_owners(dir_id, file_id).await;
            let result = match owners {
                None => {
                    error!("File missing {}", file_id);
                    Err(DownloadError::FileMissing)
                }
                Some(owners) => {
                    let client = server_data.clients.iter().find(|(_, c)| {
                        if let Some(id) = &c.id {
                            return owners.contains(id);
                        }

                        false
                    });

                    match client {
                        None => {
                            error!("Clients to download from not found");
                            Err(DownloadError::NoClientsConnected)
                        }
                        Some((_, c)) => {
                            let download_id = Uuid::new_v4();
                            let download_path = server_data
                                .config
                                .generate_filepath(dir_id, file_id, download_id)
                                .await;

                            match download_path {
                                None => {
                                    error!("File missing {}", file_id);
                                    Err(DownloadError::FileMissing)
                                }
                                Some(path) => {
                                    c.sender
                                        .send(MessageToClient::StartDownload {
                                            download_id,
                                            file_identifier: file_id,
                                            directory_identifier: dir_id,
                                            destination: path,
                                        })
                                        .await?;

                                    Ok(())
                                }
                            }
                        }
                    }
                }
            };

            if let Err(e) = result {
                error!("{}", e);

                let _ = server_data
                    .window_manager
                    .send(WindowRequest::Error(BackendError {
                        error: e.to_string(),
                        title: "Could not start download".to_string(),
                    }));
            }

            Ok(())
        }

        WindowResponse::CancelDownload {
            download_identifier,
            peer,
        } => {
            let download_id = Uuid::parse_str(&download_identifier)?;
            let peers = vec![peer];
            server_data
                .broadcast(&peers, MessageToClient::CancelDownload { download_id })
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
    config: Arc<StoredConfig>,
) -> Result<()> {
    info!("Adding client with address {}", addr);

    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    let client_data = ClientData {
        server: server_handle,
        receiver,
        addr,
        config,
    };

    let pid = match &service_info {
        Some(service) => {
            let name = service.get_fullname();

            PeerId::parse(name)
        }
        None => None,
    };

    let join = tauri::async_runtime::spawn(client_loop(client_data, tcp, pid.clone()));

    let client = ClientHandle {
        id: pid,
        sender,
        join,
        service_info,
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