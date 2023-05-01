use std::fmt::Display;

use serde::{Serialize, Deserialize};
use uuid::Uuid;

const INSTANCE_SEPARATOR: &str = ";";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PeerId {
    pub hostname: String,
    pub uuid: Uuid
}

impl PeerId {
    pub fn to_string(&self) -> String {
        let uuid_str = self.uuid.to_string();

        let parts = [
            self.hostname.as_str(),
            uuid_str.as_str()
        ];

        parts.join(INSTANCE_SEPARATOR)
    }

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

impl Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}