pub mod mdns;
pub mod server_handle;
mod client_handle;
pub mod tcp_listener;
mod codec;

use std::net::{SocketAddr, Ipv4Addr, IpAddr};
use std::sync::Arc;
use std::time::Duration;

use if_addrs::IfAddr;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::net::{TcpStream, TcpListener};
use tokio::{sync::mpsc};
use tauri::{AppHandle, Manager, async_runtime::Mutex};

use anyhow::{Result};

use crate::data::{ShareDirectory};
use crate::config::{AppConfig, StoredConfig};

pub async fn main_network_handler(
    app_handle: AppHandle,
    stored_data: Arc<StoredConfig>,
    client_receiver: mpsc::Receiver<String>
) {
    
}

pub async fn route_input_to_network_thread(
    mut webview_to_network_receiver: mpsc::Receiver<String>,
    to_network_sender: mpsc::Sender<String>
) -> Result<()> {

    while let Some(input) = webview_to_network_receiver.recv().await {
        to_network_sender.send(input).await?;
    }

    Ok(())
}

// pub struct ConnectedPeer {
//     id: Option<PeerId>,
//     address: SocketAddr,
//     socket: TcpStream,

// }

// pub enum Peer {
//     Connected(ConnectedPeer),
//     Remembered(PeerId)
// }

pub struct NetworkThreadSender {
    pub inner: Mutex<mpsc::Sender<String>>
}

impl NetworkThreadSender {
    pub fn new(sender: mpsc::Sender<String>) -> Self {
        Self {
            inner: Mutex::new(sender)
        }
    }
}

#[tauri::command]
pub async fn to_network_thread(
    message: String,
    state: tauri::State<'_, NetworkThreadSender>
) -> Result<(), String> {

    info!("{}", message);

    let sender = state.inner.lock().await;
    sender
        .send(message)
        .await
        .map_err(|e| e.to_string())
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

    let intf = intf_addr.iter().next().expect("should have at least 1 ipv4 interface");

    *intf
}