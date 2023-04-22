use std::{collections::HashMap, path::Path};

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct ShareDirectory {
    name: String,
    shared_files: HashMap<String, SharedFile>
}

#[derive(Serialize, Deserialize)]
pub struct SharedFile {
    name: String,
    content_hash: String,
    last_modified: DateTime<Utc>,
    content_location: ContentLocation
}

#[derive(Serialize, Deserialize)]
pub enum ContentLocation {
    LocalPath(Box<Path>),
    NetworkPath(String)
}