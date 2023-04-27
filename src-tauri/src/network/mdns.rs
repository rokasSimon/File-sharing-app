use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::{mpsc, oneshot};

use crate::peer_id::PeerId;

use super::server_handle::{MessageToServer, ServerHandle};

pub const SERVICE_TYPE: &str = "_ktu_fileshare._tcp.local.";
pub const MDNS_UPDATE_TIME: u64 = 120;
pub const RECONNECT_TIME: i64 = 15;

const MDNS_PORT: u16 = 61000;

#[derive(Debug)]
pub enum MessageToMdns {
    RemoveService(ServiceInfo),
    ConnectedService(ServiceInfo),
}

pub struct ResolvedServiceInfo {
    pub service_info: ServiceInfo,
    pub status: ServiceStatus,
}

pub enum ServiceStatus {
    Disconnected(DateTime<Utc>),
    Connected,
}

pub struct MdnsHandle {
    pub info: ServiceInfo,
}

pub async fn start_mdns(
    mut server_recv: mpsc::Receiver<MessageToMdns>,
    addr_recv: oneshot::Receiver<SocketAddr>,
    server_handle: ServerHandle,
    peer_id: PeerId,
    intf_addr: Ipv4Addr,
) -> Result<()> {
    let local_addr = addr_recv.await?;
    let ip = local_addr.ip();

    let _ = match ip {
        IpAddr::V4(ipv4) => ipv4,
        IpAddr::V6(_) => panic!("IPv6 not supported"),
    };

    let my_name = &peer_id.to_string();
    let mut host_name = my_name.clone();
    host_name.push_str(".local.");

    let addr = [intf_addr];

    let properties = HashMap::from([("port".to_owned(), local_addr.port().to_string())]);

    let mdns = ServiceDaemon::new().expect("should be able to create mDNS daemon");
    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &my_name,
        &host_name,
        &addr[..],
        MDNS_PORT,
        Some(properties),
    )
    .expect("should be able to create service info");

    let service_receiver = mdns.browse(SERVICE_TYPE).expect("should start mDNS browse");

    let reconnect_time = chrono::Duration::seconds(RECONNECT_TIME);
    let mut reregister_interval = tokio::time::interval(Duration::from_secs(MDNS_UPDATE_TIME));
    let mut resolved_services: HashMap<String, ResolvedServiceInfo> = HashMap::new();

    loop {
        tokio::select! {
            event = service_receiver.recv_async() => {
                match event {
                    Ok(ev) => handle_mdns_event(&ev, &server_handle, &host_name).await,
                    Err(err) => error!("Event received was error: {}", err)
                }
            }
            Some(server_action) = server_recv.recv() => {
                match server_action {

                    MessageToMdns::RemoveService(service_to_remove) => {
                        if let Some(mut service) = resolved_services.get_mut(service_to_remove.get_fullname()) {
                            let current_time = Utc::now();
                            info!("Disconnecting service at {}", current_time);
                            service.status = ServiceStatus::Disconnected(current_time);
                        }
                    }

                    MessageToMdns::ConnectedService(service_connected) => {
                        info!("Connected service {}", service_connected.get_fullname());
                        resolved_services.insert(service_connected.get_fullname().to_owned(), ResolvedServiceInfo {
                            service_info: service_connected,
                            status: ServiceStatus::Connected
                        });
                    }
                }
            }
            _ = reregister_interval.tick() => {
                let res = mdns.register(service_info.clone());

                match res {
                    Ok(()) => info!("Registered service"),
                    Err(e) => error!("{}", e)
                };

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

async fn handle_mdns_event(event: &ServiceEvent, server_handle: &ServerHandle, my_hostname: &str) {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            info!("Resolved service {:?}", service);

            if service.get_hostname() != my_hostname {
                info!("Adding service: {:?}", service);

                let _ = server_handle
                    .channel
                    .send(MessageToServer::ServiceFound(service.clone()))
                    .await;
            }
        }
        ServiceEvent::ServiceRemoved(service_type, fullname) => {
            warn!("Removed service {} {}", service_type, fullname);
        }
        _ => (),
    }
}
