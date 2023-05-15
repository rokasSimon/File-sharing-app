use std::{net::{Ipv4Addr}, time::Duration};

use anyhow::Result;
use if_addrs::IfAddr;
use tokio::{net::TcpListener, sync::{mpsc}};

use crate::{server::{MessageToServer, ServerHandle}, mdns::MessageToMdns};

pub async fn start_accept(
    send_addr: mpsc::Sender<MessageToMdns>,
    server_handle: ServerHandle,
) -> Result<()> {
    loop {
        let intf = get_ipv4_intf();
        if let Some(addr) = intf {
            let bind_res = TcpListener::bind((addr, 0)).await;

            if let Ok(tcp_listener) = bind_res {
                while let Ok((tcp, ip)) = tcp_listener.accept().await {
                    info!("Accepted connection from {}", ip);
    
                    let msg = MessageToServer::ConnectionAccepted(tcp, ip);
                    let _ = server_handle.channel.send(msg).await;
                }
            }
        }

        let _ = tokio::time::interval(Duration::from_secs(5)).tick().await;
    }
}

fn get_ipv4_intf() -> Option<Ipv4Addr> {
    if_addrs::get_if_addrs()
        .expect("should be able to get IP interfaces")
        .into_iter()
        .filter_map(|intf| {
            if intf.is_loopback() {
                None
            } else {
                match intf.addr {
                    IfAddr::V4(ifv4) => Some(ifv4),
                    _ => None,
                }
            }
        })
        .map(|intf| intf.ip)
        .next()
}