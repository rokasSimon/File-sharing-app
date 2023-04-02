mod mdns;
mod peer_id;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use mdns_sd::ServiceDaemon;
use tokio::net::{TcpStream};
use tokio::{net::TcpListener, sync::mpsc};
use tauri::{AppHandle, Manager, async_runtime::Mutex};

use crate::data::{ShareDirectory};
use crate::config::{AppConfig, StoredConfig};

use self::peer_id::{PeerId};

const SERVICE_TYPE: &str = "_ktu_fileshare._tcp.local.";

pub async fn main_network_handler(
    app_handle: AppHandle,
    stored_data: Arc<StoredConfig>,
    connections: Arc<Mutex<Vec<ConnectedPeer>>>,
    client_receiver: mpsc::Receiver<String>
) {
    
    
}

pub async fn route_input_to_network_thread(
    mut webview_to_network_receiver: mpsc::Receiver<String>,
    to_network_sender: mpsc::Sender<String>
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {

    while let Some(input) = webview_to_network_receiver.recv().await {
        to_network_sender.send(input).await?;
    }

    Ok(())
}

pub async fn accept_tcp_connections(
    connections: Arc<Mutex<Vec<ConnectedPeer>>>
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tcp_listener = TcpListener::bind("127.0.0.1:0").await.expect("should be able to bind TCP listener to address");

    while let Ok((tcp_stream, socket_addr)) = tcp_listener.accept().await {
        let mut connection_lock = connections.lock().await;

        // connection_lock.retain(async |connection| {
        //     let mut buf = [0];
        //     let peek_result = connection.socket.peek(&mut buf).await;

        //     match peek_result {
        //         Ok(_) => true,
        //         Err(_) => false
        //     }
        // });

        connection_lock.push(ConnectedPeer {
            id: None,
            address: socket_addr,
            socket: tcp_stream
        });
    }

    Ok(())
}

pub async fn query_mdns_periodically(

) {
    let mdns_daemon = ServiceDaemon::new().expect("should be able to create MDNS daemon thread");
    let service_browser = mdns_daemon.browse(SERVICE_TYPE).expect("should be able to create MDNS service browser");

    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        use mdns_sd::ServiceEvent;

        interval.tick().await;

        if let Ok(event) = service_browser.recv_async().await {
            match event {
                ServiceEvent::ServiceFound(service_type, name) => (),
                ServiceEvent::ServiceRemoved(service_type, name) => (),
                ServiceEvent::ServiceResolved(service_info) => (),
                _ => ()
            }
        }
    }
}

pub struct ConnectedPeer {
    id: Option<PeerId>,
    address: SocketAddr,
    socket: TcpStream
}

pub enum Peer {
    Connected(ConnectedPeer),
    Remembered(PeerId)
}

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