#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

mod config;
mod data;
mod network;
mod peer_id;
mod window;

use std::{
    net::{SocketAddr, SocketAddrV4},
    sync::Arc,
};

use tauri::{async_runtime::Mutex, Manager};
use tokio::sync::{mpsc, oneshot};
use window_shadows::set_shadow;

use network::{
    get_ipv4_intf,
    mdns::{start_mdns, MessageToMdns},
    network_command,
    server_handle::{server_loop, MessageToServer, ServerHandle},
    tcp_listener::start_accept,
    NetworkThreadSender,
};

use crate::{
    config::{load_stored_data, write_stored_data}, network::server_handle::WindowRequest, window::open_file
};

const THREAD_CHANNEL_SIZE: usize = 64;

fn main() {
    pretty_env_logger::init();

    let config = load_stored_data();
    let id = config
        .app_config
        .blocking_lock()
        .peer_id
        .clone()
        .expect("PeerID should be set on startup");
    let stored_data = Arc::new(config);

    let (network_sender, network_receiver) = mpsc::channel::<WindowRequest>(THREAD_CHANNEL_SIZE);
    let (mdns_sender, mdns_receiver) = mpsc::channel::<MessageToMdns>(THREAD_CHANNEL_SIZE);

    let (tcp_addr_sender, tcp_addr_receiver) = oneshot::channel::<SocketAddr>();
    let intf_addr = get_ipv4_intf();
    let soc_addr = SocketAddrV4::new(intf_addr, 0).into();

    let (server_sender, server_receiver) = mpsc::channel::<MessageToServer>(THREAD_CHANNEL_SIZE);
    let server_handle = ServerHandle {
        channel: server_sender,
        config: stored_data.clone(),
        peer_id: id.clone(),
    };

    tauri::Builder::default()
        .manage(NetworkThreadSender {
            inner: Mutex::new(network_sender),
        })
        .on_window_event(move |event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let config = stored_data.clone();
                let config_lock = config.app_config.blocking_lock();

                if config_lock.hide_on_close {
                    api.prevent_close();
                }
            }
            tauri::WindowEvent::Destroyed => {
                let config = stored_data.clone();

                write_stored_data(&config);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![network_command, open_file])
        .setup(move |app| {
            let window = app.get_window("main").expect("To find main window");

            if let Err(e) = set_shadow(&window, true) {
                warn!("Could not set shadows: {}", e)
            }

            tauri::async_runtime::spawn(start_accept(
                soc_addr,
                tcp_addr_sender,
                server_handle.clone(),
            ));
            tauri::async_runtime::spawn(start_mdns(
                mdns_receiver,
                tcp_addr_receiver,
                server_handle.clone(),
                id.clone(),
                intf_addr,
            ));

            let app_handle = app.handle();
            tauri::async_runtime::spawn(server_loop(
                app_handle,
                server_receiver,
                network_receiver,
                mdns_sender,
                server_handle.clone(),
            ));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    info!("Exiting app...");
}
