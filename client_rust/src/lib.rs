pub mod config;
pub mod s3_backend;
pub mod webdav_server;
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Object {
    pub id: u64,
    pub name: String,
    pub size: Option<u64>,
    pub last_modified: Option<i64>,
    pub etag: Option<String>,
    #[serde(rename = "type")]
    pub content_type: Option<String>,
    pub download_url: Option<String>,
    pub parent_id: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListObjectsResponse {
    pub results: Vec<Object>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StartUploadResponse {
    pub upload_id: String,
    pub key: String,
    pub chunk_size: u64,
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

#[derive(Clone)]
pub struct HamrahClient {
    client: Client,
    token: Option<String>,
    base_url: String,
    phone: Option<String>,
    password: Option<String>,
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
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            token: None,
            base_url: "https://abrehamrahi.ir".to_string(),
            phone: None,
            password: None,
        }
    }

    async fn relogin(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let phone = self.phone.clone().ok_or("no credentials stored")?;
        let password = self.password.clone().ok_or("no credentials stored")?;
        self.token = None;
        self.login(&phone, &password).await
    }

    pub async fn login(&mut self, phone: &str, password: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.phone = Some(phone.to_string());
        self.password = Some(password.to_string());
        let phone_num = if phone.starts_with('0') { &phone[1..] } else { phone };
        
        // Try to load session first
        if let Some(token) = self.load_session(phone_num).await {
            self.token = Some(token);
            // Verify token with a simple request
            if self.fetch_objects().await.is_ok() {
                println!("Reusing existing session for {}", phone_num);
                return Ok(());
            }
            self.token = None;
        }

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
             let token = access.to_string();
             self.token = Some(token.clone());
             self.save_session(phone_num, &token).await?;
        } else if let Some(access) = json_body.get("token").and_then(|t| t.get("access")).and_then(|v| v.as_str()) {
             let token = access.to_string();
             self.token = Some(token.clone());
             self.save_session(phone_num, &token).await?;
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

    async fn fetch_objects_url(&self, path: &str) -> Result<Vec<Object>, Box<dyn std::error::Error>> {
        let resp = self.authed_request(reqwest::Method::GET, path)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(format!("list failed: {}", resp.status()).into());
        }
        let data: ListObjectsResponse = resp.json().await?;
        Ok(data.results)
    }

    async fn fetch_objects(&self) -> Result<Vec<Object>, Box<dyn std::error::Error>> {
        self.fetch_objects_url("/api/v2/flat/list-objects/?is_trash=false&limit=1000").await
    }

    pub async fn list_objects(&mut self) -> Result<Vec<Object>, Box<dyn std::error::Error>> {
        let needs_refresh = self.fetch_objects().await.is_err();
        if needs_refresh {
            self.relogin().await?;
        }
        self.fetch_objects().await
    }

    pub async fn list_objects_by_parent(&mut self, parent_id: u64) -> Result<Vec<Object>, Box<dyn std::error::Error>> {
        let url = format!("/api/v2/flat/list-objects/?is_trash=false&limit=1000&parent_id={}", parent_id);
        log::info!("[list_objects_by_parent] fetching parent_id={}", parent_id);
        let first_err = match self.fetch_objects_url(&url).await {
            Ok(objects) => {
                log::info!("[list_objects_by_parent] got {} objects for parent_id={}", objects.len(), parent_id);
                return Ok(objects);
            }
            Err(e) => e.to_string(),
        };
        log::warn!("[list_objects_by_parent] first attempt failed: {}", first_err);
        if first_err.contains("401") || first_err.contains("token_not_valid") || first_err.contains("Token is expired") || first_err.contains("503") {
            self.relogin().await?;
        }
        let url2 = format!("/api/v2/flat/list-objects/?is_trash=false&limit=1000&parent_id={}", parent_id);
        self.fetch_objects_url(&url2).await
    }

    pub async fn upload_file<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let file_name = path.file_name().ok_or("Invalid filename")?.to_str().ok_or("Filename not unicode")?;
        let mut file = std::fs::File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        self.upload_bytes(file_name, buffer).await
    }

    pub async fn upload_bytes(&mut self, name: &str, data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        // Convert error to String before any await so the future stays Send
        let first_err = match self.upload_bytes_inner(name, data.clone()).await {
            Ok(()) => return Ok(()),
            Err(e) => e.to_string(),
        };
        if first_err.contains("401") || first_err.contains("token_not_valid") || first_err.contains("Token is expired") || first_err.contains("503") {
            eprintln!("[upload_bytes] token expired, re-logging in...");
            self.relogin().await?;
            self.upload_bytes_inner(name, data).await
        } else {
            Err(first_err.into())
        }
    }

    async fn upload_bytes_inner(&self, name: &str, data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        let size = data.len() as u64;

        let start_resp = self.authed_request(reqwest::Method::POST, "/api/v2/flat/start-upload/")
            .json(&json!({ "obj_size": size }))
            .send()
            .await?;

        let start_data: StartUploadResponse = start_resp.json().await?;
        let chunk_size = start_data.chunk_size as usize;

        // Upload all parts in parallel
        let upload_futures: Vec<_> = start_data.signed_urls.iter()
            .zip(data.chunks(chunk_size))
            .enumerate()
            .map(|(i, (url, chunk))| {
                let client = self.client.clone();
                let url = url.clone();
                let chunk = chunk.to_vec();
                async move {
                    let chunk_len = chunk.len();
                    let put_resp = client.put(&url)
                        .header("Content-Type", "application/octet-stream")
                        .body(chunk)
                        .send()
                        .await
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

                    if !put_resp.status().is_success() {
                        let msg = format!("PUT part {} failed: {}", i + 1, put_resp.text().await.unwrap_or_default());
                        return Err(Box::<dyn std::error::Error + Send + Sync>::from(msg));
                    }

                    let etag = put_resp.headers().get("ETag")
                        .ok_or("Missing ETag")?
                        .to_str()
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
                        .to_string();

                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>((i + 1, etag, chunk_len))
                }
            })
            .collect();

        let results = futures_util::future::join_all(upload_futures).await;
        let mut parts: Vec<(usize, String, usize)> = {
            let mut acc = Vec::new();
            for r in results {
                acc.push(r.map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?);
            }
            acc
        };
        parts.sort_by_key(|(part_num, _, _)| *part_num);
        let parts: Vec<_> = parts.into_iter()
            .map(|(n, etag, size)| json!({ "ETag": etag, "PartNumber": n, "size": size }))
            .collect();

        let complete_resp = self.authed_request(reqwest::Method::POST, "/api/v2/flat/complete-upload/")
            .json(&json!({
                "key": start_data.key,
                "name": name,
                "upload_id": start_data.upload_id,
                "parts": parts,
                "force_overwrite": true
            }))
            .send()
            .await?;

        if !complete_resp.status().is_success() {
            let status = complete_resp.status();
            let body = complete_resp.text().await.unwrap_or_default();
            return Err(format!("complete-upload failed ({status}): {body}").into());
        }

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

    pub async fn rename_object(&self, obj_id: u64, new_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = "/api/v2/rgw/rename-object/";
        let resp = self.authed_request(reqwest::Method::POST, url)
            .json(&json!({ "obj_id": obj_id, "name": new_name }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Rename failed: {}", resp.text().await?).into())
        }
    }

    pub async fn copy_object(&self, source_obj_id: u64, target_parent_id: Option<u64>, new_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = "/api/v5/rgw/copy-object/";
        let resp = self.authed_request(reqwest::Method::POST, url)
            .json(&json!({
                "source_obj_id": source_obj_id,
                "target_parent_id": target_parent_id,
                "new_name": new_name
            }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Copy failed: {}", resp.text().await?).into())
        }
    }

    pub async fn move_object(&self, source_obj_id: u64, target_parent_id: u64) -> Result<(), Box<dyn std::error::Error>> {
        let url = "/api/v2/rgw/move-object/";
        let resp = self.authed_request(reqwest::Method::POST, url)
            .json(&json!({
                "source_obj_id": source_obj_id,
                "target_parent_id": target_parent_id
            }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Move failed: {}", resp.text().await?).into())
        }
    }

    pub async fn create_folder(&self, name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = "/api/v2/flat/create-folder/";
        let resp = self.authed_request(reqwest::Method::POST, url)
            .json(&json!({ "name": name }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Create folder failed: {}", resp.text().await?).into())
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

    pub async fn download_object(&self, download_url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let resp = self.client.get(download_url)
            .header("Authorization", format!("Bearer {}", self.token.as_deref().unwrap_or("")))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Download failed ({status}): {body}").into());
        }

        Ok(resp.bytes().await?.to_vec())
    }

    async fn save_session(&self, phone: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
        let session_file = format!(".session_{}", phone);
        std::fs::write(session_file, token)?;
        Ok(())
    }

    async fn load_session(&self, phone: &str) -> Option<String> {
        let session_file = format!(".session_{}", phone);
        std::fs::read_to_string(session_file).ok()
    }
}
