use platform_dirs::AppDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tauri::async_runtime::Mutex;
use uuid::Uuid;

use crate::data::{PeerId, ShareDirectory};

const APP_FILES_LOCATION: &str = "fileshare";
const APP_CONFIG_LOCATION: &str = "config.json";
const APP_CACHE_LOCATION: &str = "cached_files.json";
const DEFAULT_DOWNLOAD_LOCATION: &str = "downloads";
const SAVE_INTERVAL_SECS: u64 = 300;

pub fn load_stored_data() -> StoredConfig {
    let app_dir =
        AppDirs::new(Some(APP_FILES_LOCATION), false).expect("to be able to create config files");

    let config_path = ensure_path(app_dir.config_dir, APP_CONFIG_LOCATION);
    let cache_path = ensure_path(app_dir.data_dir.clone(), APP_CACHE_LOCATION);

    let config_str = fs::read_to_string(&config_path).expect("to be able to read the config file");
    let mut config: AppConfig = serde_json::from_str(&config_str).unwrap_or_default();

    if config.peer_id.is_none() {
        config.peer_id = Some(PeerId::generate());
    }

    if !config.download_directory.exists() {
        let default_download_path = app_dir.data_dir.join(DEFAULT_DOWNLOAD_LOCATION);

        if !default_download_path.exists() {
            fs::create_dir(default_download_path.clone())
                .expect("should be able to create default download directory");
        }

        config.download_directory = default_download_path;
    }

    let cache_str = fs::read_to_string(&cache_path).expect("to be able to read cache file");
    let cache: HashMap<Uuid, ShareDirectory> = serde_json::from_str(&cache_str).unwrap_or_default();

    StoredConfig::new(config, cache)
}

pub fn write_stored_data(stored_config: &StoredConfig) {
    let app_dir =
        AppDirs::new(Some(APP_FILES_LOCATION), false).expect("to be able to create config files");

    let config_path = app_dir.config_dir.join(APP_CONFIG_LOCATION);
    let cache_path = app_dir.data_dir.join(APP_CACHE_LOCATION);

    let config_bytes = serde_json::to_vec_pretty(&*stored_config.app_config.blocking_lock());
    let cache_bytes = serde_json::to_vec_pretty(&*stored_config.cached_data.blocking_lock());

    let mut open_settings = OpenOptions::new();
    let open_settings = open_settings.write(true).truncate(true);

    if let Ok(config) = config_bytes {
        let file = open_settings.open(config_path);

        if let Ok(mut file) = file {
            if let Err(e) = file.write_all(&config) {
                error!("could not write config to file: {}", e);
            } else {
                info!("Successfully wrote config to file");
            }
        }
    }

    if let Ok(cache) = cache_bytes {
        let file = open_settings.open(cache_path);

        if let Ok(mut file) = file {
            if let Err(e) = file.write_all(&cache) {
                error!("could not write cache to file: {}", e);
            } else {
                info!("Successfully wrote cache to file");
            }
        }
    }
}

pub async fn write_stored_data_async(stored_config: &StoredConfig) {
    let app_dir =
        AppDirs::new(Some(APP_FILES_LOCATION), false).expect("to be able to create config files");

    let config_path = app_dir.config_dir.join(APP_CONFIG_LOCATION);
    let cache_path = app_dir.data_dir.join(APP_CACHE_LOCATION);

    let config_bytes = serde_json::to_vec_pretty(&*stored_config.app_config.lock().await);
    let cache_bytes = serde_json::to_vec_pretty(&*stored_config.cached_data.lock().await);

    let mut open_settings = tokio::fs::OpenOptions::new();
    let open_settings = open_settings.write(true).truncate(true);

    if let Ok(config) = config_bytes {
        let file = open_settings.open(config_path).await;

        if let Ok(mut file) = file {
            if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &config).await {
                error!("could not write config to file: {}", e);
            } else {
                info!("Successfully wrote config to file");
            }
        }
    }

    if let Ok(cache) = cache_bytes {
        let file = open_settings.open(cache_path).await;

        if let Ok(mut file) = file {
            if let Err(e) = tokio::io::AsyncWriteExt::write_all(&mut file, &cache).await {
                error!("could not write cache to file: {}", e);
            } else {
                info!("Successfully wrote cache to file");
            }
        }
    }
}

pub async fn save_config_loop(configs: Arc<StoredConfig>) {
    let mut job_interval = tokio::time::interval(Duration::from_secs(SAVE_INTERVAL_SECS));

    loop {
        let _ = job_interval.tick().await;

        write_stored_data_async(&configs).await;
    }
}

fn ensure_path<P>(path: PathBuf, subpath: P) -> PathBuf
where
    P: AsRef<Path>,
{
    fs::create_dir_all(&path).expect("should be able to create directory for stored data");

    let resulting_path = path.join(&subpath);
    if !resulting_path.exists() {
        File::create(&resulting_path).expect("should be able to create file for stored data");
    }

    resulting_path
}

#[derive(Serialize, Deserialize)]
pub struct AppConfig {
    pub peer_id: Option<PeerId>,
    pub hide_on_close: bool,
    pub download_directory: PathBuf,
    pub theme: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            peer_id: None,
            hide_on_close: false,
            download_directory: PathBuf::new(),
            theme: "dark".to_string(),
        }
    }
}

pub struct StoredConfig {
    pub app_config: Mutex<AppConfig>,
    pub cached_data: Mutex<HashMap<Uuid, ShareDirectory>>,
}

impl StoredConfig {
    pub fn new(app_config: AppConfig, cached_data: HashMap<Uuid, ShareDirectory>) -> Self {
        Self {
            app_config: Mutex::new(app_config),
            cached_data: Mutex::new(cached_data),
        }
    }
}