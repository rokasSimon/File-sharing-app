use anyhow::{bail, Result};

use platform_dirs::AppDirs;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, hash_map::Entry},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration, str::FromStr,
};
use tauri::async_runtime::Mutex;
use uuid::Uuid;

use crate::data::{PeerId, ShareDirectory, SharedFile, ContentLocation};

const APP_FILES_LOCATION: &str = "fileshare";
const APP_CONFIG_LOCATION: &str = "config.json";
const APP_CACHE_LOCATION: &str = "cached_files.json";
const DEFAULT_DOWNLOAD_LOCATION: &str = "downloads";
const SAVE_INTERVAL_SECS: u64 = 300;

pub fn load_stored_data() -> (StoredConfig, PeerId) {
    let app_dir =
        AppDirs::new(Some(APP_FILES_LOCATION), false).expect("to be able to create config files");

    let config_path = ensure_path(app_dir.config_dir, APP_CONFIG_LOCATION);
    let cache_path = ensure_path(app_dir.data_dir.clone(), APP_CACHE_LOCATION);

    let config_str = fs::read_to_string(&config_path).expect("to be able to read the config file");
    let mut config: AppConfig = serde_json::from_str(&config_str).unwrap_or_default();

    if config.peer_id.is_none() {
        config.peer_id = Some(PeerId::generate());
    }
    let peer_id = config.peer_id.clone().unwrap();

    if !config.download_directory.exists() {
        let default_download_path = app_dir.data_dir.join(DEFAULT_DOWNLOAD_LOCATION);

        if !default_download_path.exists() {
            fs::create_dir(default_download_path.clone())
                .expect("should be able to create default download directory");
        }

        config.download_directory = default_download_path;
    }

    let cache_str = fs::read_to_string(cache_path).expect("to be able to read cache file");
    let cache: HashMap<Uuid, ShareDirectory> = serde_json::from_str(&cache_str).unwrap_or_default();

    (StoredConfig::new(config, cache), peer_id)
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub minimize_on_close: bool,
    pub theme: String,
    pub download_directory: String,
}

pub struct StoredConfig {
    app_config: Mutex<AppConfig>,
    cached_data: Mutex<HashMap<Uuid, ShareDirectory>>,
}

impl StoredConfig {
    pub fn new(app_config: AppConfig, cached_data: HashMap<Uuid, ShareDirectory>) -> Self {
        Self {
            app_config: Mutex::new(app_config),
            cached_data: Mutex::new(cached_data),
        }
    }

    pub async fn get_settings(&self) -> Settings {
        let app_conf = self.app_config.lock().await;

        Settings {
            minimize_on_close: app_conf.hide_on_close,
            theme: app_conf.theme.clone(),
            download_directory: app_conf.download_directory.to_str().unwrap_or_default().to_string(),
        }
    }

    pub async fn set_settings(&self, new_settings: Settings) -> Result<()> {
        let mut app_conf = self.app_config.lock().await;

        app_conf.download_directory = PathBuf::from_str(&new_settings.download_directory)?;
        app_conf.hide_on_close = new_settings.minimize_on_close;
        app_conf.theme = new_settings.theme;

        Ok(())
    }

    pub async fn get_directories(&self) -> Vec<ShareDirectory> {
        let directories = self.cached_data.lock().await;

        directories.values().cloned().collect()
    }

    pub async fn get_directory(&self, dir_id: Uuid) -> Option<ShareDirectory> {
        let directories = self.cached_data.lock().await;

        directories.get(&dir_id).map(|dir| dir.clone())
    }

    pub async fn get_filepath(&self, dir_id: Uuid, file_id: Uuid) -> Option<PathBuf> {
        let directories = self.cached_data.lock().await;

        match directories.get(&dir_id) {
            None => None,
            Some(dir) => {
                let file = dir.shared_files.get(&file_id);

                match file {
                    None => None,
                    Some(file) => {
                        match &file.content_location {
                            ContentLocation::NetworkOnly => None,
                            ContentLocation::LocalPath(path) => Some(path.clone())
                        }
                    }
                }
            }
        }
    }

    pub async fn mutate_dir<F>(&self, dir_id: Uuid, f: F) where F: FnOnce(&mut ShareDirectory) {
        let mut directories = self.cached_data.lock().await;
        let dir = directories.get_mut(&dir_id);

        if let Some(dir) = dir {
            f(dir);
        }
    }

    pub async fn mutate_file<F>(&self, dir_id: Uuid, file_id: Uuid, f: F) where F: FnOnce(&mut SharedFile) {
        let mut directories = self.cached_data.lock().await;
        
        if let Some(dir) = directories.get_mut(&dir_id) {
            if let Some(file) = dir.shared_files.get_mut(&file_id) {
                f(file);
            }
        }
    }

    pub async fn add_directory(&self, dir: ShareDirectory) {
        let mut directories = self.cached_data.lock().await;

        directories.insert(dir.signature.identifier, dir);
    }

    pub async fn remove_directory(&self, dir_id: Uuid) -> Option<ShareDirectory> {
        let mut directories = self.cached_data.lock().await;

        directories.remove(&dir_id)
    }

    pub async fn generate_filepath(&self, dir_id: Uuid, file_id: Uuid, download_id: Uuid) -> Option<PathBuf> {
        let directories = self.cached_data.lock().await;
        let config = self.app_config.lock().await;
        let dir = directories.get(&dir_id);

        match dir {
            None => None,
            Some(dir) => {
                let file = dir.shared_files.get(&file_id);

                match file {
                    None => None,
                    Some(file) => {
                        let download_dir = config.download_directory.clone();
                        let file_path = download_dir.join(file.name.clone());

                        if file_path.exists() {
                            Some(file_path.join(download_id.to_string()))
                        } else {
                            Some(file_path)
                        }
                    }
                }
            }
        }
    }

    pub async fn get_owners(&self, dir_id: Uuid, file_id: Uuid) -> Option<Vec<PeerId>> {
        let directories = self.cached_data.lock().await;
        let dir = directories.get(&dir_id);

        match dir {
            None => None,
            Some(dir) => {
                let file = dir.shared_files.get(&file_id);

                file.map(|file| file.owned_peers.clone())
            }
        }
    }

    pub async fn shared_directory(&self, dir: ShareDirectory) -> Result<()> {
        let mut directories = self.cached_data.lock().await;

        if let Entry::Vacant(e) = directories.entry(dir.signature.identifier) {
            e.insert(dir);

            return Ok(());
        }

        bail!("Directory already shared");
    }

    pub async fn synchronize(&self, dirs: Vec<ShareDirectory>, host: &PeerId) -> Vec<ShareDirectory> {
        let mut owned_dirs = self.cached_data.lock().await;

        for dir in dirs {
            let od = owned_dirs.get_mut(&dir.signature.identifier);

            match od {
                Some(matched_dir) => {
                    if dir.signature.last_modified > matched_dir.signature.last_modified {
                        matched_dir.signature.shared_peers = dir.signature.shared_peers;

                        if !matched_dir.signature.shared_peers.contains(host) {
                            matched_dir
                                .signature
                                .shared_peers
                                .push(host.clone());
                        }

                        let mut files_to_delete: Vec<Uuid> = vec![];
                        for (file_id, file) in matched_dir.shared_files.iter_mut() {
                            if !dir.shared_files.contains_key(file_id) {
                                if !file.owned_peers.contains(host) {
                                    files_to_delete.push(*file_id);
                                }
                            }
                        }

                        let mut files_to_add = vec![];
                        for (file_id, file) in dir.shared_files.iter() {
                            match matched_dir.shared_files.get_mut(file_id) {
                                None => files_to_add.push(file.clone()),
                                Some(matched_file) => {
                                    matched_file.owned_peers = file.owned_peers.clone();
                                }
                            }
                        }

                        matched_dir
                            .shared_files
                            .retain(|file_id, _| !files_to_delete.contains(file_id));

                        for file in files_to_add {
                            matched_dir.shared_files.insert(file.identifier, file);
                        }
                    }
                }
                None => {
                    owned_dirs.insert(dir.signature.identifier, dir);
                }
            }
        }

        owned_dirs.values().cloned().collect()
    }
}
