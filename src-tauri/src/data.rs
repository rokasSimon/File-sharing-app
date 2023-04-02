use std::{collections::HashMap, path::Path, sync::Arc};

use chrono::{DateTime, Local};
use serde::{Serialize, Deserialize};
use tauri::async_runtime::Mutex;

#[derive(Serialize, Deserialize)]
pub struct ShareDirectory {
    name: String,
    shared_files: HashMap<String, SharedFile>
}

#[derive(Serialize, Deserialize)]
pub struct SharedFile {
    name: String,
    content_hash: String,
    last_modified: DateTime<Local>,
    content_location: ContentLocation
}

#[derive(Serialize, Deserialize)]
pub enum ContentLocation {
    LocalPath(Box<Path>),
    NetworkPath(String)
}