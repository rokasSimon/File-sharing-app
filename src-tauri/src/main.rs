#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

pub mod client;
pub mod config;
pub mod data;
pub mod listen;
pub mod mdns;
pub mod server;
pub mod window;

use std::sync::Arc;

use config::{load_stored_data, save_config_loop, write_stored_data};
use listen::start_accept;
use mdns::{start_mdns, MessageToMdns};
use server::{server_loop, MessageToServer, ServerHandle};
use tauri::{async_runtime::Mutex, CustomMenuItem, Manager, SystemTray, SystemTrayMenu};
use tokio::sync::mpsc;
use window::{
    commands::{get_settings, network_command, open_file, save_settings, Window},
    MainWindowManager, WindowResponse,
};
use window_shadows::set_shadow;

const THREAD_CHANNEL_SIZE: usize = 64;
const MAIN_WINDOW_LABEL: &str = "main";

fn main() {
    pretty_env_logger::init();

    let (conf, id) = load_stored_data();
    let stored_data = Arc::new(conf);

    let (network_sender, network_receiver) = mpsc::channel::<WindowResponse>(THREAD_CHANNEL_SIZE);
    let (mdns_sender, mdns_receiver) = mpsc::channel::<MessageToMdns>(THREAD_CHANNEL_SIZE);
    let (server_sender, server_receiver) = mpsc::channel::<MessageToServer>(THREAD_CHANNEL_SIZE);

    let server_handle = ServerHandle {
        channel: server_sender,
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
                if id.as_str() == "exit" {
                    let window = app.get_window(MAIN_WINDOW_LABEL);

                    if let Some(window) = window {
                        let res = window.close();

                        if let Err(e) = res {
                            error!("Could not close main window{}", e);
                        }
                    }
                }
            }
            tauri::SystemTrayEvent::LeftClick { .. } => {
                let window = app.get_window(MAIN_WINDOW_LABEL);

                if let Some(window) = window {
                    let result = window.show();

                    if let Err(e) = result {
                        error!("Could not show window: {}", e);
                    }
                }
            }
            _ => (),
        })
        .system_tray(system_tray)
        .manage(Window {
            server: Mutex::new(network_sender),
        })
        .manage(settings_config)
        .on_window_event(move |event| match event.event() {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                let settings = tauri::async_runtime::block_on(window_config.get_settings());

                if settings.minimize_on_close {
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
        .invoke_handler(tauri::generate_handler![
            network_command,
            open_file,
            save_settings,
            get_settings
        ])
        .setup(move |app| {
            let window = app
                .get_window(MAIN_WINDOW_LABEL)
                .expect("To find main window");

            if let Err(e) = set_shadow(&window, true) {
                warn!("Could not set shadows: {}", e)
            }

            tauri::async_runtime::spawn(start_accept(mdns_sender.clone(), server_handle.clone()));
            tauri::async_runtime::spawn(start_mdns(
                mdns_receiver,
                server_handle.clone(),
                id.clone(),
            ));

            let app_handle = app.handle();
            let window_manager = MainWindowManager {
                app_handle,
                window_label: MAIN_WINDOW_LABEL,
            };
            tauri::async_runtime::spawn(server_loop(
                window_manager,
                server_receiver,
                network_receiver,
                mdns_sender,
                server_handle.clone(),
                stored_data.clone(),
            ));

            tauri::async_runtime::spawn(save_config_loop(loop_config));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
