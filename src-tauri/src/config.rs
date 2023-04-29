use platform_dirs::AppDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf}, io::Write, collections::HashMap,
};
use tauri::async_runtime::Mutex;

use crate::{data::ShareDirectory, peer_id::PeerId};

const APP_FILES_LOCATION: &str = "fileshare";
const APP_CONFIG_LOCATION: &str = "config.json";
const APP_CACHE_LOCATION: &str = "cached_files.json";
const DEFAULT_DOWNLOAD_LOCATION: &str = "downloads";

pub fn load_stored_data() -> StoredConfig {
    let app_dir =
        AppDirs::new(Some(APP_FILES_LOCATION), false).expect("to be able to create config files");

    let config_path = ensure_path(app_dir.config_dir, APP_CONFIG_LOCATION);
    let cache_path = ensure_path(app_dir.data_dir.clone(), APP_CACHE_LOCATION);
    let download_path = ensure_path(app_dir.data_dir, DEFAULT_DOWNLOAD_LOCATION);

    let config_str = fs::read_to_string(&config_path).expect("to be able to read the config file");
    let mut config: AppConfig = serde_json::from_str(&config_str).unwrap_or_default();

    if config.peer_id.is_none() {
        config.peer_id = Some(PeerId::generate());
    }

    if !config.download_directory.exists() {
        config.download_directory = download_path;
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            peer_id: None,
            hide_on_close: false,
            download_directory: PathBuf::new(),
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
