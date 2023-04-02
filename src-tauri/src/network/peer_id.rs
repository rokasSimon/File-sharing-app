use std::{net::SocketAddr};

use serde::{Serialize, Deserialize};
use tokio::net::TcpStream;
use uuid::Uuid;

const INSTANCE_SEPARATOR: &str = ";";

#[derive(Serialize, Deserialize)]
pub struct PeerId {
    hostname: String,
    uuid: Uuid
}

impl PeerId {
    pub fn parse(instance: &str) -> Option<Self> {
        let (hostname, uuid_str) = instance.split_once(INSTANCE_SEPARATOR)?;
        let uuid = Uuid::parse_str(uuid_str).ok()?;
        
        Some(Self {
            hostname: hostname.to_owned(),
            uuid
        })
    }

    pub fn generate() -> Self {
        let os_hostname = hostname::get().unwrap().into_string();
        let hostname = match os_hostname {
            Ok(h) => h,
            Err(_) => "generic_hostname".to_owned()
        };

        let uuid = Uuid::new_v4();

        Self {
            hostname,
            uuid
        }
    }
}