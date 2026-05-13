use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountConfig {
    pub phone: String,
    pub password: String,
    pub webdav_user: Option<String>,
    pub webdav_pass: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub accounts: HashMap<String, AccountConfig>,
    pub proxy: Option<String>,
    pub s3_port: Option<u16>,
}

impl AppConfig {
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let f = std::fs::File::open(path)?;
        let config: Self = serde_yaml::from_reader(f)?;
        Ok(config)
    }
}
