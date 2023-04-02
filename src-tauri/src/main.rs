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

use std::sync::{Arc};

use tauri::{Manager, async_runtime::Mutex};
use tokio::sync::mpsc;
use window_shadows::set_shadow;

use config::load_stored_data;
use network::{main_network_handler, to_network_thread, NetworkThreadSender, route_input_to_network_thread, accept_tcp_connections};

const NETWORK_THREAD_RECEIVER_SIZE: usize = 1;

fn main() {
    pretty_env_logger::init();

    let config = load_stored_data();
    let stored_data = Arc::new(config);

    let (webview_to_intermediary_sender, intermediary_receiver) = mpsc::channel::<String>(NETWORK_THREAD_RECEIVER_SIZE);
    let (intermediary_to_network_sender, network_receiver) = mpsc::channel::<String>(NETWORK_THREAD_RECEIVER_SIZE);

    let connections_handle = Arc::new(Mutex::new(vec![]));

    tauri::Builder::default()
        .manage(NetworkThreadSender::new(webview_to_intermediary_sender))
        .invoke_handler(tauri::generate_handler![to_network_thread])
        .setup(move |app| {
            let window = app.get_window("main").expect("To find main window");

            if let Err(e) = set_shadow(&window, true) {
                warn!("Could not set shadows: {}", e)
            }

            tauri::async_runtime::spawn(route_input_to_network_thread(intermediary_receiver, intermediary_to_network_sender));
            tauri::async_runtime::spawn(accept_tcp_connections(connections_handle.clone()));

            let app_handle = app.handle();
            tauri::async_runtime::spawn(
                main_network_handler(
                    app_handle,
                    stored_data,
                    connections_handle.clone(),
                    network_receiver
                )
            );

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
