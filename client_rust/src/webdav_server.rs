use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode, header, Method},
    response::IntoResponse,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::time::Instant;
use crate::HamrahClient;
use crate::Object as HamrahObject;
use crate::s3_backend::{encode_key, decode_key};
use http_body_util::BodyExt;

pub struct WebDavState {
    pub clients: Arc<HashMap<String, Arc<Mutex<HamrahClient>>>>,
    // Cache never expires automatically; entries are replaced on explicit invalidation.
    // Key: account name for root listing, "account/parent_id" for folder children.
    pub cache: Arc<Mutex<HashMap<String, Vec<HamrahObject>>>>,
}

impl WebDavState {
    pub fn new(clients: HashMap<String, HamrahClient>) -> Self {
        let clients = Arc::new(
            clients.into_iter().map(|(k, v)| (k, Arc::new(Mutex::new(v)))).collect()
        );
        Self { clients, cache: Arc::new(Mutex::new(HashMap::new())) }
    }
}

/// Pre-warm all caches at startup so the first Finder request is instant.
pub async fn prewarm_caches(state: Arc<WebDavState>) {
    for (account, client) in state.clients.iter() {
        log::info!("[prewarm] fetching account listing for {}", account);
        let objects = match client.lock().await.list_objects().await {
            Ok(o) => o,
            Err(e) => { log::warn!("[prewarm] account {} failed: {}", account, e); continue; }
        };
        let real_folder_ids: Vec<u64> = objects.iter()
            .filter(|o| is_folder(o))
            .map(|o| o.id)
            .collect();
        state.cache.lock().await.insert(account.clone(), objects);
        log::info!("[prewarm] account {} cached, {} real folders to warm", account, real_folder_ids.len());
        for fid in real_folder_ids {
            log::info!("[prewarm] fetching folder parent_id={}", fid);
            let result = client.lock().await.list_objects_by_parent(fid).await;
            match result {
                Ok(children) => {
                    let key = format!("{}/{}", account, fid);
                    state.cache.lock().await.insert(key, children);
                    log::info!("[prewarm] folder {} cached", fid);
                }
                Err(e) => log::warn!("[prewarm] folder {} failed: {}", fid, e),
            }
        }
    }
    log::info!("[prewarm] all caches ready");
}

struct DavResource {
    href: String,
    display_name: String,
    is_collection: bool,
    size: Option<u64>,
    last_modified: Option<i64>,
    etag: Option<String>,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn format_rfc1123(timestamp: i64) -> String {
    use chrono::{DateTime, Utc};
    DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(|| Utc::now())
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
}

fn encode_href_path(path: &str) -> String {
    path.split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn build_multistatus(resources: &[DavResource]) -> String {
    let mut xml = String::from(r#"<?xml version="1.0" encoding="utf-8"?>"#);
    xml.push_str(r#"<D:multistatus xmlns:D="DAV:">"#);
    for res in resources {
        xml.push_str("<D:response>");
        xml.push_str(&format!("<D:href>{}</D:href>", xml_escape(&encode_href_path(&res.href))));
        xml.push_str("<D:propstat><D:prop>");
        if let Some(size) = res.size {
            xml.push_str(&format!("<D:getcontentlength>{}</D:getcontentlength>", size));
        }
        if let Some(ts) = res.last_modified {
            xml.push_str(&format!("<D:getlastmodified>{}</D:getlastmodified>", format_rfc1123(ts)));
        }
        if let Some(ref etag) = res.etag {
            xml.push_str(&format!("<D:getetag>{}</D:getetag>", xml_escape(etag)));
        }
        xml.push_str("<D:resourcetype>");
        if res.is_collection {
            xml.push_str("<D:collection/>");
        }
        xml.push_str("</D:resourcetype>");
        xml.push_str(&format!("<D:displayname>{}</D:displayname>", xml_escape(&res.display_name)));
        xml.push_str(r#"</D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>"#);
        xml.push_str("</D:response>");
    }
    xml.push_str("</D:multistatus>");
    xml
}

fn parse_path(path: &str) -> (Option<&str>, &str) {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return (None, "");
    }
    match path.split_once('/') {
        Some((account, rest)) => (Some(account), rest),
        None => (Some(path), ""),
    }
}

fn extract_path_from_destination(dest: &str) -> &str {
    if dest.starts_with("http://") {
        if let Some(pos) = dest[7..].find('/') {
            &dest[7 + pos..]
        } else {
            "/"
        }
    } else if dest.starts_with("https://") {
        if let Some(pos) = dest[8..].find('/') {
            &dest[8 + pos..]
        } else {
            "/"
        }
    } else {
        dest
    }
}

fn is_folder(obj: &HamrahObject) -> bool {
    obj.content_type.as_deref() == Some("folder")
}

async fn list_account_objects(state: Arc<WebDavState>, account: String) -> Result<Vec<HamrahObject>, StatusCode> {
    // Serve from cache if present (no TTL — cache is only invalidated by S3 mutations or prewarm)
    if let Some(objects) = state.cache.lock().await.get(&account) {
        return Ok(objects.clone());
    }
    // Cache miss: fetch and store
    let client = state.clients.get(&account)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    let objects = client.lock().await.list_objects().await
        .map_err(|e| {
            log::error!("[webdav] list_objects error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    state.cache.lock().await.insert(account, objects.clone());
    Ok(objects)
}

pub async fn invalidate_cache(state: Arc<WebDavState>, account: String) {
    let mut cache = state.cache.lock().await;
    // Remove root listing and all folder sub-listings for this account
    cache.retain(|k, _| !k.starts_with(&account));
}

async fn list_folder_objects(state: Arc<WebDavState>, account: String, parent_id: u64) -> Result<Vec<HamrahObject>, StatusCode> {
    let cache_key = format!("{}/{}", account, parent_id);
    if let Some(objects) = state.cache.lock().await.get(&cache_key) {
        return Ok(objects.clone());
    }
    let client = state.clients.get(&account)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    let fetch = async {
        client.lock().await.list_objects_by_parent(parent_id).await
    };
    let objects = tokio::time::timeout(std::time::Duration::from_secs(20), fetch).await
        .map_err(|_| {
            log::error!("[webdav] list_objects_by_parent timeout for parent_id={}", parent_id);
            StatusCode::GATEWAY_TIMEOUT
        })?
        .map_err(|e| {
            log::error!("[webdav] list_objects_by_parent error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    state.cache.lock().await.insert(cache_key, objects.clone());
    Ok(objects)
}

// --- Handlers ---

pub async fn handle_options() -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header("DAV", "1, 2")
        .header("Allow", "OPTIONS, GET, HEAD, PROPFIND, LOCK, UNLOCK")
        .body(Body::empty())
        .unwrap()
}

pub async fn handle_propfind(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let raw_path = req.uri().path().to_string();
    let decoded_path = urlencoding::decode(&raw_path)
        .unwrap_or_else(|_| raw_path.clone().into())
        .into_owned();
    let depth = req.headers()
        .get("Depth")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("1");

    let (maybe_account, subpath) = parse_path(&decoded_path);

    match maybe_account {
        None => {
            let mut resources = vec![DavResource {
                href: "/".to_string(),
                display_name: "".to_string(),
                is_collection: true,
                size: None,
                last_modified: None,
                etag: None,
            }];
            if depth != "0" {
                for name in state.clients.keys() {
                    resources.push(DavResource {
                        href: format!("/{}/", name),
                        display_name: name.clone(),
                        is_collection: true,
                        size: None,
                        last_modified: None,
                        etag: None,
                    });
                }
            }
            let xml = build_multistatus(&resources);
            return Response::builder()
                .status(StatusCode::MULTI_STATUS)
                .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                .body(Body::from(xml))
                .unwrap();
        }
        Some(account) => {
            let prefix = if subpath.is_empty() {
                ""
            } else if subpath.ends_with('/') {
                &subpath[..subpath.len()-1]
            } else {
                subpath
            };

            let objects = match list_account_objects(state.clone(), account.to_string()).await {
                Ok(o) => o,
                Err(e) => return e.into_response(),
            };

            // Check if the path's FIRST component is a real Hamrah folder (type == "folder").
            // e.g. prefix="Manwe/rustic-test" → real_folder="Manwe", virtual_sub="rustic-test"
            //      prefix="Manwe"             → real_folder="Manwe", virtual_sub=""
            let (real_folder_id, virtual_sub): (Option<u64>, String) = if !prefix.is_empty() {
                let first = prefix.split('/').next().unwrap_or("");
                if let Some(fo) = objects.iter().find(|o| is_folder(o) && decode_key(&o.name) == first) {
                    let rest = prefix[first.len()..].trim_start_matches('/').to_string();
                    (Some(fo.id), rest)
                } else {
                    (None, String::new())
                }
            } else {
                (None, String::new())
            };

            if !subpath.is_empty() && !subpath.ends_with('/') {
                // Could be a file or a Hamrah folder referenced without trailing slash
                if let Some(folder_obj) = objects.iter().find(|o| is_folder(o) && decode_key(&o.name) == subpath) {
                    // Treat as a collection — redirect PROPFIND to the folder view
                    let folder_href = format!("{}/", raw_path);
                    let resource = DavResource {
                        href: folder_href,
                        display_name: decode_key(&folder_obj.name)
                            .rsplit_once('/')
                            .map(|(_, n)| n.to_string())
                            .unwrap_or_else(|| decode_key(&folder_obj.name)),
                        is_collection: true,
                        size: None,
                        last_modified: None,
                        etag: None,
                    };
                    let xml = build_multistatus(&[resource]);
                    return Response::builder()
                        .status(StatusCode::MULTI_STATUS)
                        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                        .body(Body::from(xml))
                        .unwrap();
                }
                let encoded = encode_key(subpath);
                if let Some(obj) = objects.iter().find(|o| o.name == encoded) {
                    let resource = DavResource {
                        href: raw_path.clone(),
                        display_name: decode_key(&obj.name)
                            .rsplit_once('/')
                            .map(|(_, n)| n.to_string())
                            .unwrap_or_else(|| decode_key(&obj.name)),
                        is_collection: false,
                        size: obj.size,
                        last_modified: obj.last_modified,
                        etag: obj.etag.clone(),
                    };
                    let xml = build_multistatus(&[resource]);
                    return Response::builder()
                        .status(StatusCode::MULTI_STATUS)
                        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                        .body(Body::from(xml))
                        .unwrap();
                }
                // File not found in flat listing and not a real folder — 404
                // (real-folder subtree lookups are not supported for single-file PROPFIND)
                if real_folder_id.is_none() {
                    return StatusCode::NOT_FOUND.into_response();
                }
            }

            // For virtual directory paths (no real folder backing), verify the path
            // actually exists — i.e. at least one object has this prefix.
            // Without this check every made-up path returns 207, confusing macOS.
            if real_folder_id.is_none() && !prefix.is_empty() {
                let search_prefix = format!("{}/", prefix);
                let exists = objects.iter().any(|o| {
                    !is_folder(o) && decode_key(&o.name).starts_with(&search_prefix)
                });
                if !exists {
                    return StatusCode::NOT_FOUND.into_response();
                }
            }

            let folder_href = if raw_path.ends_with('/') {
                raw_path.clone()
            } else {
                format!("{}/", raw_path)
            };
            let mut resources = vec![DavResource {
                href: folder_href,
                display_name: if prefix.is_empty() {
                    account.to_string()
                } else {
                    prefix.rsplit_once('/')
                        .map(|(_, n)| n.to_string())
                        .unwrap_or_else(|| prefix.to_string())
                },
                is_collection: true,
                size: None,
                last_modified: None,
                etag: None,
            }];

            if depth != "0" {
                // If inside a real Hamrah folder, fetch its children and apply virtual_sub filtering
                if let Some(folder_id) = real_folder_id {
                    let children = match list_folder_objects(state.clone(), account.to_string(), folder_id).await {
                        Ok(c) => c,
                        Err(e) => return e.into_response(),
                    };
                    // virtual_sub is the path within the real folder.
                    // e.g. Manwe/ → virtual_sub="", Manwe/rustic-test/ → virtual_sub="rustic-test"
                    let search_prefix = if virtual_sub.is_empty() { String::new() } else { format!("{}/", virtual_sub) };
                    let mut seen = std::collections::HashSet::new();
                    for obj in &children {
                        if obj.id == folder_id { continue; } // skip parent folder itself
                        if is_folder(obj) {
                            // Real sub-folders only appear at the top level of a real folder
                            if virtual_sub.is_empty() {
                                let child_name = decode_key(&obj.name);
                                if seen.insert(child_name.clone()) {
                                    let href = format!("/{}/{}/{}/", account, prefix, child_name);
                                    resources.push(DavResource {
                                        href,
                                        display_name: child_name,
                                        is_collection: true,
                                        size: None,
                                        last_modified: None,
                                        etag: None,
                                    });
                                }
                            }
                            continue;
                        }
                        // Apply %2F virtual directory grouping filtered by search_prefix
                        let decoded = decode_key(&obj.name);
                        if !search_prefix.is_empty() && !decoded.starts_with(&search_prefix) {
                            continue;
                        }
                        let suffix = &decoded[search_prefix.len()..];
                        if suffix.is_empty() { continue; }
                        if let Some(slash_pos) = suffix.find('/') {
                            let dir_name = &suffix[..slash_pos];
                            if seen.insert(dir_name.to_string()) {
                                let href = format!("/{}/{}/{}/", account, prefix, dir_name);
                                resources.push(DavResource {
                                    href,
                                    display_name: dir_name.to_string(),
                                    is_collection: true,
                                    size: None,
                                    last_modified: None,
                                    etag: None,
                                });
                            }
                        } else if seen.insert(suffix.to_string()) {
                            // Deduplicate — Hamrah allows same-name objects, WebDAV hrefs must be unique
                            let href = format!("/{}/{}/{}", account, prefix, suffix);
                            resources.push(DavResource {
                                href,
                                display_name: suffix.to_string(),
                                is_collection: false,
                                size: obj.size,
                                last_modified: obj.last_modified,
                                etag: obj.etag.clone(),
                            });
                        }
                    }
                } else {
                    // Standard flat listing at root or virtual path level
                    let search_prefix = if prefix.is_empty() { "".to_string() } else { format!("{}/", prefix) };
                    let mut seen_dirs = std::collections::HashSet::new();

                    // Pre-warm folder caches in the background when listing the account root.
                    for obj in &objects {
                        // Real Hamrah folders at root level — show as collections
                        if is_folder(obj) && prefix.is_empty() {
                            let folder_name = decode_key(&obj.name);
                            if seen_dirs.insert(folder_name.clone()) {
                                let href = format!("/{}/{}/", account, folder_name);
                                resources.push(DavResource {
                                    href,
                                    display_name: folder_name,
                                    is_collection: true,
                                    size: None,
                                    last_modified: None,
                                    etag: None,
                                });
                            }
                            continue;
                        }

                        let decoded = decode_key(&obj.name);
                        if !decoded.starts_with(&*search_prefix) {
                            continue;
                        }
                        let suffix = &decoded[search_prefix.len()..];
                        if suffix.is_empty() {
                            continue;
                        }

                        if let Some(slash_pos) = suffix.find('/') {
                            let dir_name = &suffix[..slash_pos];
                            if seen_dirs.insert(dir_name.to_string()) {
                                let href = format!("/{}/{}{}/", account, search_prefix, dir_name);
                                resources.push(DavResource {
                                    href,
                                    display_name: dir_name.to_string(),
                                    is_collection: true,
                                    size: None,
                                    last_modified: None,
                                    etag: None,
                                });
                            }
                        } else {
                            let href = format!("/{}/{}{}", account, search_prefix, suffix);
                            resources.push(DavResource {
                                href,
                                display_name: suffix.to_string(),
                                is_collection: false,
                                size: obj.size,
                                last_modified: obj.last_modified,
                                etag: obj.etag.clone(),
                            });
                        }
                    }
                }
            }

            let xml = build_multistatus(&resources);
            Response::builder()
                .status(StatusCode::MULTI_STATUS)
                .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
                .body(Body::from(xml))
                .unwrap()
        }
    }
}

pub async fn handle_get_head(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let is_head = req.method() == Method::HEAD;

    let decoded_path = urlencoding::decode(&path)
        .unwrap_or_else(|_| path.clone().into())
        .into_owned();
    let (maybe_account, subpath) = parse_path(&decoded_path);
    let account = match maybe_account {
        Some(a) => a,
        None => return StatusCode::FORBIDDEN.into_response(),
    };

    if subpath.is_empty() || subpath.ends_with('/') {
        return StatusCode::FORBIDDEN.into_response();
    }

    let objects = match list_account_objects(state.clone(), account.to_string()).await {
        Ok(o) => o,
        Err(e) => return e.into_response(),
    };

    let encoded = encode_key(subpath);
    let obj = match objects.iter().find(|o| o.name == encoded) {
        Some(o) => o.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(size) = obj.size {
        builder = builder.header(header::CONTENT_LENGTH, size);
    }
    if let Some(ref etag) = obj.etag {
        builder = builder.header(header::ETAG, etag.clone());
    }
    if let Some(ts) = obj.last_modified {
        builder = builder.header(header::LAST_MODIFIED, format_rfc1123(ts));
    }
    if let Some(ref ct) = obj.content_type {
        builder = builder.header(header::CONTENT_TYPE, ct.clone());
    }

    if is_head {
        return builder.body(Body::empty()).unwrap();
    }

    let dl_url = match obj.download_url {
        Some(url) => url,
        None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let client = match state.clients.get(account) {
        Some(c) => c.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let data = match client.lock().await.download_object(&dl_url).await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[webdav] download error: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    builder.body(Body::from(data)).unwrap()
}

pub async fn handle_put(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let decoded_path = urlencoding::decode(&path)
        .unwrap_or_else(|_| path.clone().into())
        .into_owned();
    let (maybe_account, subpath) = parse_path(&decoded_path);
    let account = match maybe_account {
        Some(a) => a,
        None => return StatusCode::FORBIDDEN.into_response(),
    };

    if subpath.is_empty() || subpath.ends_with('/') {
        return StatusCode::FORBIDDEN.into_response();
    }

    let client = match state.clients.get(account) {
        Some(c) => c.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let body = req.into_body();
    let collected = match body.collect().await {
        Ok(c) => c.to_bytes(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let data = collected.to_vec();

    let name = encode_key(subpath);
    let account_owned = account.to_string();
    let result: Result<(), String> = client.lock().await.upload_bytes(&name, data).await.map_err(|e| e.to_string());
    match result {
        Ok(()) => {
            invalidate_cache(state, account_owned).await;
            StatusCode::CREATED.into_response()
        }
        Err(e) => {
            eprintln!("[webdav] upload error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn handle_delete(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let decoded_path = urlencoding::decode(&path)
        .unwrap_or_else(|_| path.clone().into())
        .into_owned();
    let (maybe_account, subpath) = parse_path(&decoded_path);
    let account = match maybe_account {
        Some(a) => a,
        None => return StatusCode::FORBIDDEN.into_response(),
    };

    if subpath.is_empty() {
        return StatusCode::FORBIDDEN.into_response();
    }
    // Strip trailing slash — macOS may send DELETE /file/ if it mistook the file for a dir
    let subpath = subpath.trim_end_matches('/');

    let objects = match list_account_objects(state.clone(), account.to_string()).await {
        Ok(o) => o,
        Err(e) => return e.into_response(),
    };

    let encoded = encode_key(subpath);
    let obj_id = match objects.iter().find(|o| o.name == encoded) {
        Some(o) => o.id,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let client = match state.clients.get(account) {
        Some(c) => c.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let account_owned = account.to_string();
    let result: Result<(), String> = client.lock().await.delete_file(obj_id).await.map_err(|e| e.to_string());
    match result {
        Ok(()) => {
            invalidate_cache(state, account_owned).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            eprintln!("[webdav] delete error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn handle_mkcol(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let decoded_path = urlencoding::decode(&path)
        .unwrap_or_else(|_| path.clone().into())
        .into_owned();
    let (maybe_account, subpath) = parse_path(&decoded_path);

    match maybe_account {
        None => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        Some(account) => {
            if !state.clients.contains_key(account) {
                return StatusCode::NOT_FOUND.into_response();
            }
            if subpath.is_empty() {
                return StatusCode::METHOD_NOT_ALLOWED.into_response();
            }
            StatusCode::CREATED.into_response()
        }
    }
}

pub async fn handle_move(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let destination = req.headers()
        .get("Destination")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let decoded_path = urlencoding::decode(&path)
        .unwrap_or_else(|_| path.clone().into())
        .into_owned();
    let dest_path = extract_path_from_destination(destination);
    let decoded_dest = urlencoding::decode(dest_path)
        .unwrap_or_else(|_| dest_path.into())
        .into_owned();

    let (src_account, src_subpath) = parse_path(&decoded_path);
    let (dst_account, dst_subpath) = parse_path(&decoded_dest);

    let src_account = match src_account {
        Some(a) => a,
        None => return StatusCode::FORBIDDEN.into_response(),
    };

    if src_account != dst_account.unwrap_or("") {
        return StatusCode::FORBIDDEN.into_response();
    }

    if src_subpath.is_empty() || src_subpath.ends_with('/') {
        return StatusCode::FORBIDDEN.into_response();
    }
    if dst_subpath.is_empty() || dst_subpath.ends_with('/') {
        return StatusCode::FORBIDDEN.into_response();
    }

    let objects = match list_account_objects(state.clone(), src_account.to_string()).await {
        Ok(o) => o,
        Err(e) => return e.into_response(),
    };

    let src_encoded = encode_key(src_subpath);
    let obj_id = match objects.iter().find(|o| o.name == src_encoded) {
        Some(o) => o.id,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let client = match state.clients.get(src_account) {
        Some(c) => c.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let account_owned = src_account.to_string();
    let new_name = encode_key(dst_subpath);
    let result: Result<(), String> = client.lock().await.rename_object(obj_id, &new_name).await.map_err(|e| e.to_string());
    match result {
        Ok(()) => {
            invalidate_cache(state, account_owned).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            eprintln!("[webdav] move/rename error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn handle_copy(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let path = req.uri().path().to_string();
    let destination = req.headers()
        .get("Destination")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let decoded_path = urlencoding::decode(&path)
        .unwrap_or_else(|_| path.clone().into())
        .into_owned();
    let dest_path = extract_path_from_destination(destination);
    let decoded_dest = urlencoding::decode(dest_path)
        .unwrap_or_else(|_| dest_path.into())
        .into_owned();

    let (src_account, src_subpath) = parse_path(&decoded_path);
    let (dst_account, dst_subpath) = parse_path(&decoded_dest);

    let src_account = match src_account {
        Some(a) => a,
        None => return StatusCode::FORBIDDEN.into_response(),
    };

    if src_account != dst_account.unwrap_or("") {
        return StatusCode::FORBIDDEN.into_response();
    }

    if src_subpath.is_empty() || src_subpath.ends_with('/') {
        return StatusCode::FORBIDDEN.into_response();
    }
    if dst_subpath.is_empty() || dst_subpath.ends_with('/') {
        return StatusCode::FORBIDDEN.into_response();
    }

    let objects = match list_account_objects(state.clone(), src_account.to_string()).await {
        Ok(o) => o,
        Err(e) => return e.into_response(),
    };

    let src_encoded = encode_key(src_subpath);
    let obj_id = match objects.iter().find(|o| o.name == src_encoded) {
        Some(o) => o.id,
        None => return StatusCode::NOT_FOUND.into_response(),
    };

    let client = match state.clients.get(src_account) {
        Some(c) => c.clone(),
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let account_owned = src_account.to_string();
    let new_name = encode_key(dst_subpath);
    let result: Result<(), String> = client.lock().await.copy_object(obj_id, None, &new_name).await.map_err(|e| e.to_string());
    match result {
        Ok(()) => {
            invalidate_cache(state, account_owned).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            eprintln!("[webdav] copy error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn handle_lock(req: Request<Body>) -> Response<Body> {
    let href = req.uri().path().to_string();
    let unix_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let thread_id = format!("{:?}", std::thread::current().id());
    // Build a simple deterministic token from time + thread id hash
    let token = format!("{:x}-{:x}", unix_secs, {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        thread_id.hash(&mut h);
        unix_secs.hash(&mut h);
        h.finish()
    });
    let lock_token = format!("urn:uuid:{}", token);
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<D:prop xmlns:D="DAV:">
  <D:lockdiscovery>
    <D:activelock>
      <D:locktype><D:write/></D:locktype>
      <D:lockscope><D:exclusive/></D:lockscope>
      <D:depth>0</D:depth>
      <D:timeout>Second-3600</D:timeout>
      <D:locktoken><D:href>{lock_token}</D:href></D:locktoken>
      <D:lockroot><D:href>{href}</D:href></D:lockroot>
    </D:activelock>
  </D:lockdiscovery>
</D:prop>"#,
        lock_token = lock_token,
        href = xml_escape(&href),
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .header("Lock-Token", format!("<{}>", lock_token))
        .body(Body::from(xml))
        .unwrap()
}

pub async fn handle_unlock(_req: Request<Body>) -> Response<Body> {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap()
}

pub async fn handle_webdav_request(
    State(state): State<Arc<WebDavState>>,
    req: Request<Body>,
) -> Response<Body> {
    let method = req.method().to_string();
    let path = req.uri().to_string();
    let depth = req.headers().get("Depth").and_then(|v| v.to_str().ok()).unwrap_or("-");
    let dest = req.headers().get("Destination").and_then(|v| v.to_str().ok()).unwrap_or("");
    let ua = req.headers().get("User-Agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    log::info!("WebDAV {} {} depth={} dest={} ua={}", method, path, depth, dest, ua);

    let resp = match method.as_str() {
        "OPTIONS"  => handle_options().await,
        "PROPFIND" => handle_propfind(State(state), req).await,
        "GET" | "HEAD" => handle_get_head(State(state), req).await,
        // LOCK/UNLOCK stubs — macOS probes these even for read-only mounts
        "LOCK"   => handle_lock(req).await,
        "UNLOCK" => handle_unlock(req).await,
        // Read-only: reject all mutations
        "PUT" | "DELETE" | "MKCOL" | "MOVE" | "COPY" | "PROPPATCH" =>
            StatusCode::METHOD_NOT_ALLOWED.into_response(),
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    };
    log::info!("WebDAV {} {} → {}", method, path, resp.status());
    resp
}
