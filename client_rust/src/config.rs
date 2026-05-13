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
pub struct McConfig {
    pub host: String,
    pub port: u16,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

impl McConfig {
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn alias_cmd(&self, alias: &str) -> String {
        format!(
            "mc alias set {} {} {} {}",
            alias, self.endpoint(), self.access_key, self.secret_key
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub accounts: HashMap<String, AccountConfig>,
    pub proxy: Option<String>,
    pub mc: Option<McConfig>,
}

impl AppConfig {
    pub fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        
        // Simple expansion of ${VAR}
        use regex::Regex;
        let re = Regex::new(r"\$\{([^}]+)\}").unwrap();
        let content = re.replace_all(&content, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_default()
        }).to_string();

        let mut config: Self = serde_yaml::from_str(&content)?;
        
        // Normalize phone numbers
        for acc in config.accounts.values_mut() {
            acc.phone = normalize_phone(&acc.phone);
        }

        Ok(config)
    }
}

fn normalize_phone(phone: &str) -> String {
    let mut p = phone.trim().to_string();
    
    // Remove + prefix
    if p.starts_with('+') {
        p = p[1..].to_string();
    }
    
    // Remove 00 prefix
    if p.starts_with("00") {
        p = p[2..].to_string();
    }
    
    // Remove 98 country code
    if p.starts_with("98") && p.len() > 10 {
        p = p[2..].to_string();
    }
    
    // Remove leading 0 if it's 11 digits (0912...)
    if p.starts_with('0') && p.len() == 11 {
        p = p[1..].to_string();
    }
    
    p
}
