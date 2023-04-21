use std::net::SocketAddr;

use tokio::{net::TcpListener, sync::{oneshot}};
use anyhow::Result;

use super::{server_handle::{ServerHandle, MessageToServer}};

pub async fn start_accept(
    bind: SocketAddr,
    send_addr: oneshot::Sender<SocketAddr>,
    server_handle: ServerHandle
) -> Result<()> {
    let tcp_listener = TcpListener::bind(bind).await?;
    let local_addr = tcp_listener.local_addr()?;
    send_addr.send(local_addr).expect("to send bound port to mDNS");

    loop {
        let (tcp, ip) = tcp_listener.accept().await?;

        warn!("Accepted connection from {}", ip);

        let msg = MessageToServer::ConnectionAccepted(tcp, ip);

        let _ = server_handle.channel.send(msg).await;
    }
}