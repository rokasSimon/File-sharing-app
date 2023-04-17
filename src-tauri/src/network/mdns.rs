use std::{
    net::{IpAddr, SocketAddr, SocketAddrV4}, time::Duration,
};

use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::{sync::{oneshot, mpsc}, net::TcpStream};

use crate::peer_id::PeerId;

use super::{server_handle::{MessageToServer, ServerHandle}};

pub const SERVICE_TYPE: &str = "_ktu_fileshare._tcp.local.";
pub const MDNS_UPDATE_TIME: u64 = 5;

pub struct MdnsHandle {
    pub info: ServiceInfo,
}

pub async fn start_mdns(
    addr_recv: oneshot::Receiver<SocketAddr>,
    server_handle: ServerHandle,
    peer_id: Option<PeerId>
) -> Result<()> {
    let local_addr = addr_recv.await?;
    let ip = local_addr.ip();
    let host_name = ip.to_string() + ".local.";

    let ip = match ip {
        IpAddr::V4(ipv4) => ipv4,
        IpAddr::V6(_) => panic!("IPv6 not supported"),
    };

    let peer_id = match peer_id {
        Some(peer) => peer,
        None => PeerId::generate(),
    };

    let mdns = ServiceDaemon::new().expect("should be able to create mDNS daemon");
    let service_info = ServiceInfo::new(
        SERVICE_TYPE,
        &peer_id.to_string(),
        &host_name,
        ip,
        local_addr.port(),
        None,
    )
    .expect("should be able to create service info");

    let service_receiver = mdns.browse(SERVICE_TYPE).expect("should start mDNS browse");
    let mut interval = tokio::time::interval(Duration::from_secs(MDNS_UPDATE_TIME));

    loop {
        tokio::select! {
            event = service_receiver.recv_async() => {
                match event {
                    Ok(ev) => handle_mdns_event(&ev, &server_handle).await,
                    Err(err) => error!("Event received was error: {}", err)
                }
            }
            _ = interval.tick() => {
                let res = mdns.register(service_info.clone());

                match res {
                    Ok(()) => (),
                    Err(e) => error!("{}", e)
                }
            }
        }
    }
}

async fn handle_mdns_event(event: &ServiceEvent, server_handle: &ServerHandle) {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            warn!("Resolved service {:?}", service);

            let _ = server_handle.channel.send(MessageToServer::ServiceFound(service.clone())).await;
        }
        ServiceEvent::ServiceRemoved(service_type, fullname) => {
            warn!("Removed service {} {}", service_type, fullname);
        }
        _ => ()
    }
}