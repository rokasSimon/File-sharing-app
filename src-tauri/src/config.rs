const APP_FILES_LOCATION: &str = "fileshare";
const APP_CONFIG_LOCATION: &str = "config.json";
const APP_CACHE_LOCATION: &str = "cached_files.json";

use std::{
    fs::{self, File}
};
use platform_dirs::AppDirs;
use serde::{Deserialize, Serialize};
use tauri::async_runtime::Mutex;

use crate::{data::{ShareDirectory}, peer_id::PeerId};

pub fn load_stored_data() -> StoredConfig {
    let app_dir =
        AppDirs::new(Some(APP_FILES_LOCATION), false).expect("to be able to create config files");

    fs::create_dir_all(&app_dir.config_dir).expect("to have created config directories");
    fs::create_dir_all(&app_dir.cache_dir).expect("to have created cache directory");

    let config_path = app_dir.config_dir.join(APP_CONFIG_LOCATION);
    let cache_path = app_dir.cache_dir.join(APP_CACHE_LOCATION);

    if !config_path.exists() {
        File::create(&config_path).expect("to be able to create config file");
    }

    if !&cache_path.exists() {
        File::create(&cache_path).expect("to be able to create cache file");
    }

    let config_str = fs::read_to_string(&config_path).expect("to be able to read the config file");
    let mut config: AppConfig = serde_json::from_str(&config_str).unwrap_or_default();

    if config.peer_id.is_none() {
        config.peer_id = Some(PeerId::generate());
    }

    let cache_str = fs::read_to_string(&cache_path).expect("to be able to read cache file");
    let cache: Vec<ShareDirectory> = serde_json::from_str(&cache_str).unwrap_or_default();

    StoredConfig::new(config, cache)
}

#[derive(Serialize, Deserialize)]
pub struct AppConfig {
    pub peer_id: Option<PeerId>
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { 
            peer_id: None
        }
    }
}

pub struct StoredConfig {
    pub app_config: Mutex<AppConfig>,
    pub cached_data: Mutex<Vec<ShareDirectory>>
}

impl StoredConfig {
    pub fn new(app_config: AppConfig, cached_data: Vec<ShareDirectory>) -> Self {
        Self {
            app_config: Mutex::new(app_config),
            cached_data: Mutex::new(cached_data)
        }
    }
}