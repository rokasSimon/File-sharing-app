mod mdns;
mod peer_id;

use std::collections::HashMap;
use std::net::{SocketAddr, Ipv4Addr, IpAddr};
use std::sync::Arc;
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use tokio::net::{TcpStream, TcpListener};
use tokio::{sync::mpsc};
use tauri::{AppHandle, Manager, async_runtime::Mutex};

use anyhow::{Result, Ok};

use crate::data::{ShareDirectory};
use crate::config::{AppConfig, StoredConfig};

use self::mdns::{MdnsDaemon, SERVICE_TYPE};
use self::peer_id::{PeerId};

pub async fn main_network_handler(
    app_handle: AppHandle,
    stored_data: Arc<StoredConfig>,
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
    network_manager: Arc<NetworkManager>
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let tcp_listener = &network_manager.tcp_listener;
    let connections = &network_manager.peers;

    while let Ok((tcp_stream, socket_addr)) = tcp_listener.lock().await.accept().await {
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

pub struct NetworkManager {
    tcp_listener: Mutex<TcpListener>,
    peers: Mutex<Vec<ConnectedPeer>>,
    mdns: Mutex<MdnsDaemon>
}

impl NetworkManager {
    pub async fn new(peer_id: Option<PeerId>) -> Self {
        let ip4 = Ipv4Addr::new(127, 0, 0, 1);
        let address = SocketAddr::new(std::net::IpAddr::V4(ip4), 0);
        let tcp_listener = TcpListener::bind(address).await.expect("should be able to bind to localhost");

        let local_addr = tcp_listener.local_addr().expect("should be able to access bound address");
        let ip = local_addr.ip();
        let host_name = ip.to_string() + ".local.";
        
        let ip = match ip {
            IpAddr::V4(ipv4) => ipv4,
            IpAddr::V6(ipv6) => panic!("IPv6 not supported")
        };

        let properties = HashMap::from([
            ("port".to_owned(), local_addr.port().to_string())
        ]);

        let peer_id = match peer_id {
            Some(peer) => peer,
            None => PeerId::generate()
        };

        let mdns = ServiceDaemon::new().expect("should be able to create mDNS daemon");
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &peer_id.to_string(),
            &host_name,
            ip,
            local_addr.port(), 
            Some(properties))
        .expect("should be able to create service info");

        let mdns_daemon = MdnsDaemon::new(mdns, service, Mutex::new(vec![]));
        let peers = vec![];

        Self {
            tcp_listener: Mutex::new(tcp_listener),
            peers: Mutex::new(peers),
            mdns: Mutex::new(mdns_daemon)
        }
    }

    pub async fn start_mdns(&mut self) -> Result<()> {
        let mut mdns = self.mdns.lock().await;

        let result = mdns.start_query()?;

        Ok(())
    }

    pub async fn loop_mdns(&mut self) {
        while let Ok(mdns) = self.mdns.lock().await.
    }
}