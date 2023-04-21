use std::{
    net::{IpAddr, SocketAddr, SocketAddrV4, Ipv4Addr}, time::Duration, collections::HashMap,
};

use anyhow::Result;
use if_addrs::{IfAddr, Ifv4Addr};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::{sync::{oneshot, mpsc}, net::TcpStream};

use crate::peer_id::PeerId;

use super::{server_handle::{MessageToServer, ServerHandle}};

pub const SERVICE_TYPE: &str = "_ktu_fileshare._tcp.local.";
pub const MDNS_UPDATE_TIME: u64 = 120;

const MDNS_PORT: u16 = 61000;

pub struct MdnsHandle {
    pub info: ServiceInfo,
}

pub async fn start_mdns(
    addr_recv: oneshot::Receiver<SocketAddr>,
    server_handle: ServerHandle,
    peer_id: PeerId,
    intf_addr: Ipv4Addr
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

    let properties = HashMap::from([
        ("port".to_owned(), local_addr.port().to_string())
    ]);

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

    let _ = mdns.register(service_info.clone());

    let service_receiver = mdns.browse(SERVICE_TYPE).expect("should start mDNS browse");
    let mut interval = tokio::time::interval(Duration::from_secs(MDNS_UPDATE_TIME));

    loop {
        tokio::select! {
            event = service_receiver.recv_async() => {
                match event {
                    Ok(ev) => handle_mdns_event(&ev, &server_handle, &host_name).await,
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

async fn handle_mdns_event(event: &ServiceEvent, server_handle: &ServerHandle, my_hostname: &str) {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            warn!("Resolved service {:?}", service);

            if service.get_hostname() != my_hostname {
                warn!("Adding service: {:?}", service);

                let _ = server_handle.channel.send(MessageToServer::ServiceFound(service.clone())).await;
            }
        }
        ServiceEvent::ServiceRemoved(service_type, fullname) => {
            warn!("Removed service {} {}", service_type, fullname);
        }
        _ => ()
    }
}