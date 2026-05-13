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
        let mut content = std::fs::read_to_string(path)?;
        
        // Simple expansion of ${VAR}
        for (key, value) in std::env::vars() {
            let target = format!("${{{}}}", key);
            content = content.replace(&target, &value);
        }

        let config: Self = serde_yaml::from_str(&content)?;
        Ok(config)
    }
}
