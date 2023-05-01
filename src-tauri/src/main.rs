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

use tauri::{async_runtime::Mutex, Manager, SystemTrayMenu, CustomMenuItem, SystemTray};
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
    config::{load_stored_data, save_config_loop, write_stored_data},
    network::server_handle::WindowRequest,
    window::{open_file, save_settings, get_settings},
};

const THREAD_CHANNEL_SIZE: usize = 64;

fn main() {
    pretty_env_logger::init();

    let conf = load_stored_data();
    let id = conf
        .app_config
        .blocking_lock()
        .peer_id
        .clone()
        .expect("PeerID should be set on startup");
    let stored_data = Arc::new(conf);

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

    let exit = CustomMenuItem::new("exit".to_string(), "Exit");
    let system_tray_menu = SystemTrayMenu::new().add_item(exit);
    let system_tray = SystemTray::new().with_menu(system_tray_menu);

    let exit_config = stored_data.clone();
    let window_config = stored_data.clone();
    let loop_config = stored_data.clone();
    let settings_config = stored_data.clone();
    tauri::Builder::default()
        .on_system_tray_event(|app, event| match event {
            tauri::SystemTrayEvent::MenuItemClick { id, .. } => {
                match id.as_str() {
                    "exit" => {
                        let window = app.get_window("main");

                        if let Some(window) = window {
                            let res = window.close();

                            if let Err(e) = res {
                                error!("Could not close main window{}", e);
                            }
                        }
                    },
                    _ => ()
                }
            }
            tauri::SystemTrayEvent::LeftClick { .. } => {
                let window = app.get_window("main");

                if let Some(window) = window {
                    let result = window.show();

                    if let Err(e) = result {
                        error!("Could not show window: {}", e);
                    }
                }
            }
            _ => ()
        })
        .system_tray(system_tray)
        .manage(NetworkThreadSender {
            inner: Mutex::new(network_sender),
        })
        .manage(settings_config)
        .on_window_event(move |event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let config_lock = window_config.app_config.blocking_lock();

                if config_lock.hide_on_close {
                    info!("Trying to prevent close");
                    api.prevent_close();
                    let hide_result = event.window().hide();

                    if let Err(e) = hide_result {
                        error!("Could not hide window: {}", e);
                    }
                }
            }
            tauri::WindowEvent::Destroyed => {
                write_stored_data(&exit_config);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![network_command, open_file, save_settings, get_settings])
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

            tauri::async_runtime::spawn(save_config_loop(loop_config));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}