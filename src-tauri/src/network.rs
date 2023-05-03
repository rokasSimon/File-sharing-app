mod client;
mod codec;
pub mod mdns;
pub mod server;
pub mod tcp_listener;

use std::net::{IpAddr, Ipv4Addr};

use anyhow::Result;
use if_addrs::IfAddr;
use tauri::async_runtime::Mutex;
use tokio::sync::mpsc;

use self::server::WindowRequest;

pub type ClientConnectionId = IpAddr;

pub struct NetworkThreadSender {
    pub inner: Mutex<mpsc::Sender<WindowRequest>>,
}

#[tauri::command]
pub async fn network_command(
    message: WindowRequest,
    state: tauri::State<'_, NetworkThreadSender>,
) -> Result<(), String> {
    info!("{:?}", message);

    let sender = state.inner.lock().await;
    sender.send(message).await.map_err(|e| e.to_string())
}

pub fn get_ipv4_intf() -> Ipv4Addr {
    let intf_addr: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
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
        .collect();

    let intf = intf_addr
        .iter()
        .next()
        .expect("should have at least 1 ipv4 interface");

    *intf
}
