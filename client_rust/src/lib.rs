pub mod config;
pub mod s3_backend;
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Object {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListObjectsResponse {
    pub results: Vec<Object>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StartUploadResponse {
    pub upload_id: String,
    pub key: String,
    pub signed_urls: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublicLinkResponse {
    pub id: u64,
    pub link: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Contact {
    pub id: u64,
    pub name: String,
    pub phone: String,
    #[serde(rename = "user")]
    pub user_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListContactsResponse {
    pub results: Vec<Contact>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SharePermission {
    pub access: u32, // 1: Read, 3: Write
    pub user: u64,   // The user_id of the contact
}

pub struct HamrahClient {
    client: Client,
    token: Option<String>,
    base_url: String,
}

impl HamrahClient {
    pub fn new(proxy_url: Option<&str>) -> Self {
        let mut client_builder = Client::builder();

        if let Some(url) = proxy_url {
            let proxy = Proxy::all(url).expect("Invalid proxy URL");
            client_builder = client_builder.proxy(proxy);
        }
        
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36".parse().unwrap());
        headers.insert("Accept", "application/json, text/plain, */*".parse().unwrap());
        headers.insert("Accept-Language", "en-US,en;q=0.9,fa;q=0.8".parse().unwrap());
        headers.insert("Origin", "https://abrehamrahi.ir".parse().unwrap());
        headers.insert("Referer", "https://abrehamrahi.ir/auth/login".parse().unwrap());

        let client = client_builder
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            token: None,
            base_url: "https://abrehamrahi.ir".to_string(),
        }
    }

    pub async fn login(&mut self, phone: &str, password: &str) -> Result<(), Box<dyn std::error::Error>> {
        let phone_num = if phone.starts_with('0') { &phone[1..] } else { phone };
        
        let payload = json!({
            "phone": phone_num,
            "prefix": "+98",
            "country": "IR",
            "password": password
        });

        let url = format!("{}/api/v6/profile/auth/login/", self.base_url);
        let resp = self.client.post(&url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(format!("Login failed: {}", resp.text().await?).into());
        }

        let headers = resp.headers().clone();
        let body_text = resp.text().await?;
        let json_body: serde_json::Value = serde_json::from_str(&body_text)?;
        
        if let Some(access) = json_body.get("access").and_then(|v| v.as_str()) {
             self.token = Some(access.to_string());
        }

        let set_cookies = headers.get_all("Set-Cookie");
        for header_val in set_cookies {
            let cookie_str = header_val.to_str().unwrap_or("");
            if cookie_str.contains("ABREHAMRAHI_AUTH_TOKEN=") || cookie_str.contains("access_token=") {
                let parts: Vec<&str> = cookie_str.split(';').collect();
                for part in parts {
                    let part = part.trim();
                    if part.starts_with("ABREHAMRAHI_AUTH_TOKEN=") {
                        let val = &part["ABREHAMRAHI_AUTH_TOKEN=".len()..];
                        if let Ok(decoded) = urlencoding::decode(val) {
                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(&decoded) {
                                if let Some(access) = json_val.get("access").and_then(|v| v.as_str()) {
                                    self.token = Some(access.to_string());
                                }
                            }
                        }
                    } else if part.starts_with("access_token=") {
                        let val = &part["access_token=".len()..];
                        self.token = Some(val.to_string());
                    }
                }
            }
        }

        if self.token.is_none() {
            return Err("Could not find access token in response".into());
        }

        Ok(())
    }

    fn authed_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut rb = self.client.request(method, &url);
        if let Some(ref token) = self.token {
            rb = rb.header("Authorization", format!("Bearer {}", token));
        }
        rb
    }

    pub async fn list_objects(&self) -> Result<Vec<Object>, Box<dyn std::error::Error>> {
        let resp = self.authed_request(reqwest::Method::GET, "/api/v2/flat/list-objects/?is_trash=false&limit=1000")
            .send()
            .await?;
        
        let data: ListObjectsResponse = resp.json().await?;
        Ok(data.results)
    }

    pub async fn upload_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let file_name = path.file_name().ok_or("Invalid filename")?.to_str().ok_or("Filename not unicode")?;
        let mut file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        let size = metadata.len();

        let start_resp = self.authed_request(reqwest::Method::POST, "/api/v2/flat/start-upload/")
            .json(&json!({ "obj_size": size }))
            .send()
            .await?;
        
        let start_data: StartUploadResponse = start_resp.json().await?;
        let upload_url = start_data.signed_urls.first().ok_or("No signed upload URL provided")?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let put_resp = self.client.put(upload_url)
            .header("Content-Type", "application/octet-stream")
            .body(buffer)
            .send()
            .await?;

        if !put_resp.status().is_success() {
            return Err(format!("PUT chunk failed: {}", put_resp.text().await?).into());
        }

        let etag = put_resp.headers().get("ETag")
            .ok_or("Missing ETag header")?
            .to_str()?;

        let _complete_resp = self.authed_request(reqwest::Method::POST, "/api/v2/flat/complete-upload/")
            .json(&json!({
                "key": start_data.key,
                "name": file_name,
                "upload_id": start_data.upload_id,
                "parts": [{
                    "ETag": etag,
                    "PartNumber": 1,
                    "size": size
                }],
                "force_overwrite": false
            }))
            .send()
            .await?;

        Ok(())
    }

    pub async fn create_public_link(&self, obj_id: u64, duration: u32, limit: u32) -> Result<PublicLinkResponse, Box<dyn std::error::Error>> {
        let resp = self.authed_request(reqwest::Method::POST, "/api/v2/sharing/public-link/create/")
            .json(&json!({
                "obj_id": obj_id,
                "duration": duration,
                "expiration_count": limit
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(format!("Create link failed: {}", resp.text().await?).into());
        }

        let body = resp.text().await?;
        let data: PublicLinkResponse = serde_json::from_str(&body)?;
        Ok(data)
    }

    pub async fn update_link(&self, link_id: u64, duration: u32, limit: u32) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("/api/v2/sharing/public-link/edit/{}/", link_id);
        let resp = self.authed_request(reqwest::Method::PATCH, &url)
            .json(&json!({
                "duration": duration,
                "expiration_count": limit
            }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Update link failed: {}", resp.text().await?).into())
        }
    }

    pub async fn delete_link(&self, link_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("/api/v2/sharing/public-link/delete/{}/", link_id);
        let resp = self.authed_request(reqwest::Method::DELETE, &url)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Delete link failed: {}", resp.text().await?).into())
        }
    }

    pub async fn delete_file(&self, obj_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        let url = "/api/v2/rgw/trash-objects/";
        let resp = self.authed_request(reqwest::Method::DELETE, url)
            .json(&json!({ "obj_ids": [obj_id] }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Delete failed: {}", resp.text().await?).into())
        }
    }

    pub async fn add_contact(&self, name: &str, phone: &str) -> Result<(), Box<dyn std::error::Error>> {
        let phone_num = if phone.starts_with('0') { &phone[1..] } else { phone };
        let url = "/api/v6/profile/contact/create-contact/";
        let resp = self.authed_request(reqwest::Method::POST, url)
            .json(&json!({
                "phone": phone_num,
                "name": name,
                "prefix": "+98"
            }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Add contact failed: {}", resp.text().await?).into())
        }
    }

    pub async fn list_contacts(&self) -> Result<Vec<Contact>, Box<dyn std::error::Error>> {
        let url = "/api/v6/profile/contact/list-contact/?limit=1000";
        let resp = self.authed_request(reqwest::Method::GET, url)
            .send()
            .await?;

        let data: ListContactsResponse = resp.json().await?;
        Ok(data.results)
    }

    pub async fn share_file(&self, obj_id: u64, permissions: Vec<SharePermission>) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("/api/v2/sharing/set-permission/{}/", obj_id);
        let resp = self.authed_request(reqwest::Method::POST, &url)
            .json(&permissions)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Share file failed: {}", resp.text().await?).into())
        }
    }
}
