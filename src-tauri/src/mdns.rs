use std::{net::{SocketAddrV4}, collections::HashMap, time::Duration};

use anyhow::Result;
use chrono::{DateTime, Utc};
use mdns_sd::{ServiceInfo, ServiceEvent, ServiceDaemon};
use tokio::sync::{mpsc};

use crate::{server::{ServerHandle, MessageToServer}, data::PeerId};

pub const SERVICE_TYPE: &str = "_ktu_fileshare._tcp.local.";
pub const MDNS_UPDATE_TIME: u64 = 15;
pub const RECONNECT_TIME: i64 = 15;

#[derive(Debug)]
pub enum MessageToMdns {
    RemoveService(ServiceInfo),
    ConnectedService(ServiceInfo),
    SwitchedNetwork(SocketAddrV4)
}

pub struct ResolvedServiceInfo {
    pub service_info: ServiceInfo,
    pub status: ServiceStatus,
}

pub enum ServiceStatus {
    Disconnected(DateTime<Utc>),
    Connected,
}

pub async fn start_mdns(
    mut recv: mpsc::Receiver<MessageToMdns>,
    server_handle: ServerHandle,
    peer_id: PeerId,
) -> Result<()> {
    let mut fullname: Option<String> = None;
    let mut my_hostname: Option<String> = None;
    let mdns = ServiceDaemon::new().expect("should be able to create mDNS daemon");

    let service_receiver = mdns.browse(SERVICE_TYPE).expect("should start mDNS browse");

    let reconnect_time = chrono::Duration::seconds(RECONNECT_TIME);
    let mut reconnect_interval = tokio::time::interval(Duration::from_secs(MDNS_UPDATE_TIME));
    let mut resolved_services: HashMap<String, ResolvedServiceInfo> = HashMap::new();

    loop {
        tokio::select! {
            event = service_receiver.recv_async() => {
                match event {
                    Ok(ev) => handle_mdns_event(&ev, &server_handle, &my_hostname, &mut resolved_services).await,
                    Err(err) => error!("Event received was error: {}", err)
                }
            }
            Some(server_action) = recv.recv() => {
                match server_action {

                    MessageToMdns::RemoveService(service_to_remove) => {
                        if let Some(mut service) = resolved_services.get_mut(service_to_remove.get_fullname()) {
                            let current_time = Utc::now();
                            info!("Disconnecting service at {}", current_time);
                            service.status = ServiceStatus::Disconnected(current_time);
                        }
                    }

                    MessageToMdns::ConnectedService(service_connected) => {
                        let service = resolved_services.get_mut(service_connected.get_fullname());

                        match service {
                            None => {
                                info!("Connected service {}", service_connected.get_fullname());
                                resolved_services.insert(service_connected.get_fullname().to_owned(), ResolvedServiceInfo {
                                    service_info: service_connected,
                                    status: ServiceStatus::Connected
                                });
                            },
                            Some(service) => {
                                service.status = ServiceStatus::Connected;
                            }
                        }
                    }

                    MessageToMdns::SwitchedNetwork(new_addr) => {
                        let ip = new_addr.ip();
                        let port = new_addr.port();
                        let my_name = peer_id.to_string();
                        let host_name = my_name.clone() + ".local.";

                        let service = ServiceInfo::new(
                            SERVICE_TYPE, &my_name, &host_name, ip, port, None
                        ).unwrap();

                        if let Some(previous_service) = fullname {
                            let _ = mdns.unregister(&previous_service);
                        }

                        my_hostname = Some(service.get_hostname().to_string());
                        fullname = Some(service.get_fullname().to_string());
                        
                        let _ = mdns.register(service);
                    }
                }
            }
            _ = reconnect_interval.tick() => {
                for (_, rsv) in resolved_services.iter() {
                    match rsv.status {
                        ServiceStatus::Connected => (),
                        ServiceStatus::Disconnected(disconnect_time) => {
                            let current_time = Utc::now();
                            let time_diff = current_time - disconnect_time;

                            if time_diff >= reconnect_time {
                                let _ = server_handle.channel.send(MessageToServer::ServiceFound(rsv.service_info.clone())).await;
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn handle_mdns_event(
    event: &ServiceEvent,
    server_handle: &ServerHandle,
    my_hostname: &Option<String>,
    resolved_services: &mut HashMap<String, ResolvedServiceInfo>,
) {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            info!("Resolved service {:?}", service);

            if let Some(hostname) = my_hostname {
                if service.get_hostname() == hostname {
                    return;
                }
            }

            let existing_service = resolved_services.get(service.get_fullname());
            match existing_service {
                Some(_) => return,
                None => {
                    info!("Adding service: {:?}", service);

                    let _ = server_handle
                        .channel
                        .send(MessageToServer::ServiceFound(service.clone()))
                        .await;
                }
            }
        }
        _ => (),
    }
}
