#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod data;
mod network;
mod config;
mod peer_id;

use std::{sync::{Arc}, net::{SocketAddr, Ipv4Addr, SocketAddrV4}};

use tauri::{Manager, async_runtime::Mutex};
use tokio::{sync::{mpsc, oneshot, broadcast}, net::TcpListener};
use window_shadows::set_shadow;

use config::load_stored_data;
use network::{main_network_handler, to_network_thread, NetworkThreadSender, route_input_to_network_thread, server_handle::{ServerHandle, MessageToServer, server_loop}, tcp_listener::start_accept, mdns::start_mdns, get_ipv4_intf};

const NETWORK_THREAD_RECEIVER_SIZE: usize = 64;

fn main() {
    pretty_env_logger::init();

    let config = load_stored_data();
    let id = config.app_config.blocking_lock().peer_id.clone().expect("PeerID should be set on startup");
    let stored_data = Arc::new(config);

    let (webview_to_intermediary_sender, intermediary_receiver) = mpsc::channel::<String>(NETWORK_THREAD_RECEIVER_SIZE);
    let (intermediary_to_network_sender, network_receiver) = mpsc::channel::<String>(NETWORK_THREAD_RECEIVER_SIZE);

    let (tcp_addr_sender, tcp_addr_receiver) = oneshot::channel::<SocketAddr>();
    let intf_addr = get_ipv4_intf();
    let soc_addr = SocketAddrV4::new(intf_addr, 0).into();

    let (server_sender, server_receiver) = mpsc::channel::<MessageToServer>(NETWORK_THREAD_RECEIVER_SIZE);
    let server_handle = ServerHandle {
        channel: server_sender,
        config: stored_data,
        peer_id: id.clone(),
    };

    tauri::Builder::default()
        .manage(NetworkThreadSender::new(webview_to_intermediary_sender))
        .invoke_handler(tauri::generate_handler![to_network_thread])
        .setup(move |app| {
            let window = app.get_window("main").expect("To find main window");

            if let Err(e) = set_shadow(&window, true) {
                warn!("Could not set shadows: {}", e)
            }
            
            //let (broadcast_sender, broadcast_receiver) = broadcast::channel(64);
            
            tauri::async_runtime::spawn(start_accept(soc_addr, tcp_addr_sender, server_handle.clone()));
            tauri::async_runtime::spawn(start_mdns(tcp_addr_receiver, server_handle.clone(), id.clone(), intf_addr));
            tauri::async_runtime::spawn(server_loop(server_receiver, server_handle.clone()));
            tauri::async_runtime::spawn(route_input_to_network_thread(intermediary_receiver, intermediary_to_network_sender));

            let app_handle = app.handle();
            // tauri::async_runtime::spawn(
            //     main_network_handler(
            //         app_handle,
            //         stored_data,
            //         network_manager.clone(),
            //         network_receiver
            //     )
            // );

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}