use async_trait::async_trait;
use s3s::dto::*;
use s3s::{S3Error, S3Result, S3, S3Request, S3Response};
use crate::HamrahClient;
use crate::Object as HamrahObject;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct MultipartUpload {
    name: String,
    parts: Vec<(i32, Vec<u8>)>, // (part_number, data)
}

const CACHE_TTL: Duration = Duration::from_secs(30);

struct CachedListing {
    objects: Vec<HamrahObject>,
    fetched_at: Instant,
}

impl CachedListing {
    fn is_fresh(&self) -> bool {
        self.fetched_at.elapsed() < CACHE_TTL
    }
}

type ClientMap = Arc<HashMap<String, Arc<Mutex<HamrahClient>>>>;

pub struct HamrahS3Backend {
    clients: ClientMap,
    cache: Arc<Mutex<HashMap<String, CachedListing>>>,
    multiparts: Arc<Mutex<HashMap<String, MultipartUpload>>>,
}

impl HamrahS3Backend {
    pub fn new(clients: HashMap<String, HamrahClient>) -> Self {
        let clients = Arc::new(
            clients.into_iter().map(|(k, v)| (k, Arc::new(Mutex::new(v)))).collect()
        );
        Self {
            clients,
            cache: Arc::new(Mutex::new(HashMap::new())),
            multiparts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_client(&self, bucket: &str) -> S3Result<Arc<Mutex<HamrahClient>>> {
        self.clients.get(bucket)
            .cloned()
            .ok_or_else(|| S3Error::with_message(s3s::S3ErrorCode::NoSuchBucket, "Bucket not found"))
    }

    async fn list_cached(&self, bucket: &str) -> Result<Vec<HamrahObject>, Box<dyn std::error::Error + Send + Sync>> {
        {
            let cache = self.cache.lock().await;
            if let Some(entry) = cache.get(bucket) {
                if entry.is_fresh() {
                    return Ok(entry.objects.clone());
                }
            }
        }

        let client = self.clients.get(bucket).cloned().ok_or("bucket not found")?;
        let objects = client.lock().await
            .list_objects().await
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.to_string().into() })?;

        self.cache.lock().await.insert(bucket.to_string(), CachedListing {
            objects: objects.clone(),
            fetched_at: Instant::now(),
        });
        Ok(objects)
    }

    async fn invalidate(&self, bucket: &str) {
        self.cache.lock().await.remove(bucket);
    }
}

#[async_trait]
impl S3 for HamrahS3Backend {
    async fn list_objects_v2(&self, req: S3Request<ListObjectsV2Input>) -> S3Result<S3Response<ListObjectsV2Output>> {
        let input = req.input;
        eprintln!("[list_objects_v2] bucket='{}' prefix={:?} delimiter={:?} max_keys={:?}", input.bucket, input.prefix, input.delimiter, input.max_keys);
        let objects = self.list_cached(&input.bucket).await
            .map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;

        let prefix = input.prefix.as_deref().unwrap_or("");
        let delimiter = input.delimiter.as_deref();

        let mut common_prefixes: Vec<String> = Vec::new();
        let mut contents: Vec<s3s::dto::Object> = Vec::new();

        for obj in objects {
            if !obj.name.starts_with(prefix) {
                continue;
            }
            let suffix = &obj.name[prefix.len()..];
            if let Some(delim) = delimiter {
                if let Some(pos) = suffix.find(delim) {
                    let cp = format!("{}{}{}", prefix, &suffix[..pos], delim);
                    if !common_prefixes.contains(&cp) {
                        common_prefixes.push(cp);
                    }
                    continue;
                }
            }
            let mut s3_obj = s3s::dto::Object::default();
            s3_obj.key = Some(obj.name);
            s3_obj.size = obj.size.map(|s| s as i64);
            s3_obj.e_tag = obj.etag.map(ETag::Strong);
            s3_obj.last_modified = obj.last_modified.map(|ts| {
                Timestamp::from(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64))
            });
            contents.push(s3_obj);
        }

        let cp_output: Vec<CommonPrefix> = common_prefixes.into_iter().map(|p| {
            let mut cp = CommonPrefix::default();
            cp.prefix = Some(p);
            cp
        }).collect();

        let mut output = ListObjectsV2Output::default();
        output.name = Some(input.bucket);
        output.prefix = input.prefix;
        output.delimiter = input.delimiter;
        output.key_count = Some((contents.len() + cp_output.len()) as i32);
        output.contents = if contents.is_empty() { None } else { Some(contents) };
        output.common_prefixes = if cp_output.is_empty() { None } else { Some(cp_output) };
        output.is_truncated = Some(false);
        Ok(S3Response::new(output))
    }

    async fn put_object(&self, req: S3Request<PutObjectInput>) -> S3Result<S3Response<PutObjectOutput>> {
        let input = req.input;
        let key = input.key.clone();
        let bucket = input.bucket.clone();

        // Directory markers (trailing slash, empty body) — Hamrah is flat, just ack them
        if key.ends_with('/') {
            return Ok(S3Response::new(PutObjectOutput::default()));
        }

        let body = input.body.ok_or(S3Error::with_message(s3s::S3ErrorCode::InvalidRequest, "Missing body"))?;

        use futures_util::StreamExt;
        let mut data = Vec::new();
        let mut stream = body;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "Stream error"))?;
            data.extend_from_slice(&chunk);
        }

        // Hamrah doesn't allow slashes in names — use only the basename
        let name = key.rsplit('/').next().unwrap_or(&key).to_string();
        let client = self.get_client(&bucket)?;
        client.lock().await
            .upload_bytes(&name, data).await
            .map_err(|e| { eprintln!("[put_object] upload error: {e}"); S3Error::with_message(s3s::S3ErrorCode::InternalError, e.to_string()) })?;

        self.invalidate(&bucket).await;

        Ok(S3Response::new(PutObjectOutput::default()))
    }

    async fn get_bucket_location(&self, _req: S3Request<GetBucketLocationInput>) -> S3Result<S3Response<GetBucketLocationOutput>> {
        Ok(S3Response::new(GetBucketLocationOutput::default()))
    }

    async fn head_bucket(&self, req: S3Request<HeadBucketInput>) -> S3Result<S3Response<HeadBucketOutput>> {
        if self.clients.contains_key(&req.input.bucket) {
            Ok(S3Response::new(HeadBucketOutput::default()))
        } else {
            Err(S3Error::with_message(s3s::S3ErrorCode::NoSuchBucket, "Bucket not found"))
        }
    }

    async fn head_object(&self, req: S3Request<HeadObjectInput>) -> S3Result<S3Response<HeadObjectOutput>> {
        let input = req.input;
        eprintln!("[head_object] bucket='{}' key='{}'", input.bucket, input.key);
        let objects = self.list_cached(&input.bucket).await
            .map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;

        let obj = objects.iter().find(|o| o.name == input.key)
            .ok_or_else(|| {
                eprintln!("[head_object] key='{}' not found (have {} objects)", input.key, objects.len());
                S3Error::with_message(s3s::S3ErrorCode::NoSuchKey, "Key not found")
            })?;

        let mut out = HeadObjectOutput::default();
        out.content_length = obj.size.map(|s| s as i64);
        out.e_tag = obj.etag.clone().map(ETag::Strong);
        out.last_modified = obj.last_modified.map(|ts| {
            Timestamp::from(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64))
        });
        out.content_type = obj.content_type.clone();
        Ok(S3Response::new(out))
    }

    async fn get_object(&self, req: S3Request<GetObjectInput>) -> S3Result<S3Response<GetObjectOutput>> {
        let input = req.input;
        let objects = self.list_cached(&input.bucket).await
            .map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;

        let obj = objects.iter().find(|o| o.name == input.key)
            .ok_or_else(|| S3Error::with_message(s3s::S3ErrorCode::NoSuchKey, "Key not found"))?
            .clone();

        let dl_url = obj.download_url.clone()
            .ok_or_else(|| S3Error::with_message(s3s::S3ErrorCode::InternalError, "No download_url for object"))?;

        let client = self.get_client(&input.bucket)?;
        let data = client.lock().await
            .download_object(&dl_url).await
            .map_err(|e| { eprintln!("[get_object] download error: {e}"); S3Error::with_message(s3s::S3ErrorCode::InternalError, e.to_string()) })?;

        let mut out = GetObjectOutput::default();
        out.body = Some(StreamingBlob::from(s3s::Body::from(bytes::Bytes::from(data))));
        out.content_length = obj.size.map(|s| s as i64);
        out.e_tag = obj.etag.map(ETag::Strong);
        out.last_modified = obj.last_modified.map(|ts| {
            Timestamp::from(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64))
        });
        out.content_type = obj.content_type;
        Ok(S3Response::new(out))
    }

    async fn delete_object(&self, req: S3Request<DeleteObjectInput>) -> S3Result<S3Response<DeleteObjectOutput>> {
        let input = req.input;
        let objects = self.list_cached(&input.bucket).await
            .map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;

        let obj_id = objects.iter().find(|o| o.name == input.key)
            .ok_or_else(|| S3Error::with_message(s3s::S3ErrorCode::NoSuchKey, "Key not found"))?.id;

        let client = self.get_client(&input.bucket)?;
        client.lock().await
            .delete_file(obj_id).await
            .map_err(|e| S3Error::with_message(s3s::S3ErrorCode::InternalError, e.to_string()))?;

        self.invalidate(&input.bucket).await;
        Ok(S3Response::new(DeleteObjectOutput::default()))
    }

    async fn create_multipart_upload(&self, req: S3Request<CreateMultipartUploadInput>) -> S3Result<S3Response<CreateMultipartUploadOutput>> {
        let input = req.input;
        let upload_id = format!("{}-{}", input.bucket, uuid());
        self.multiparts.lock().await.insert(upload_id.clone(), MultipartUpload {
            name: input.key,
            parts: Vec::new(),
        });
        let mut out = CreateMultipartUploadOutput::default();
        out.upload_id = Some(upload_id);
        out.bucket = Some(input.bucket);
        Ok(S3Response::new(out))
    }

    async fn upload_part(&self, req: S3Request<UploadPartInput>) -> S3Result<S3Response<UploadPartOutput>> {
        let input = req.input;
        let upload_id = input.upload_id;
        let part_number = input.part_number;
        let body = input.body.ok_or(S3Error::with_message(s3s::S3ErrorCode::InvalidRequest, "Missing body"))?;

        use futures_util::StreamExt;
        let mut data = Vec::new();
        let mut stream = body;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "Stream error"))?;
            data.extend_from_slice(&chunk);
        }

        let etag = format!("{:x}", md5::compute(&data));
        {
            let mut mp = self.multiparts.lock().await;
            let upload = mp.get_mut(&upload_id).ok_or(S3Error::with_message(s3s::S3ErrorCode::NoSuchUpload, "Upload not found"))?;
            upload.parts.push((part_number, data));
        }

        let mut out = UploadPartOutput::default();
        out.e_tag = Some(ETag::Strong(etag));
        Ok(S3Response::new(out))
    }

    async fn complete_multipart_upload(&self, req: S3Request<CompleteMultipartUploadInput>) -> S3Result<S3Response<CompleteMultipartUploadOutput>> {
        let input = req.input;
        let upload_id = input.upload_id;
        let bucket = input.bucket;

        let (name, mut parts) = {
            let mut mp = self.multiparts.lock().await;
            let upload = mp.remove(&upload_id).ok_or(S3Error::with_message(s3s::S3ErrorCode::NoSuchUpload, "Upload not found"))?;
            (upload.name, upload.parts)
        };

        parts.sort_by_key(|(n, _)| *n);
        let data: Vec<u8> = parts.into_iter().flat_map(|(_, d)| d).collect();

        // Hamrah doesn't allow slashes in names — use only the basename
        let basename = name.rsplit('/').next().unwrap_or(&name).to_string();
        let client = self.get_client(&bucket)?;
        client.lock().await
            .upload_bytes(&basename, data).await
            .map_err(|e| { eprintln!("[complete_multipart] upload error: {e}"); S3Error::with_message(s3s::S3ErrorCode::InternalError, e.to_string()) })?;

        self.invalidate(&bucket).await;

        let mut out = CompleteMultipartUploadOutput::default();
        out.bucket = Some(bucket);
        out.key = Some(name);
        Ok(S3Response::new(out))
    }

    async fn delete_objects(&self, req: S3Request<DeleteObjectsInput>) -> S3Result<S3Response<DeleteObjectsOutput>> {
        let input = req.input;
        let bucket = input.bucket;
        let objects = self.list_cached(&bucket).await
            .map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;

        let mut deleted = Vec::new();
        let mut errors_out: Vec<s3s::dto::Error> = Vec::new();

        for obj_id_ref in input.delete.objects {
            let key = obj_id_ref.key;
            if let Some(obj) = objects.iter().find(|o| o.name == key) {
                let id = obj.id;
                let result = {
                    let client = self.get_client(&bucket)?;
                    let mut guard = client.lock().await;
                    guard.delete_file(id).await
                };
                match result {
                    Ok(_) => {
                        let mut d = DeletedObject::default();
                        d.key = Some(key);
                        deleted.push(d);
                    }
                    Err(e) => {
                        let mut err = s3s::dto::Error::default();
                        err.key = Some(key);
                        err.message = Some(e.to_string());
                        errors_out.push(err);
                    }
                }
            } else {
                // S3 semantics: deleting non-existent key is a no-op success
                let mut d = DeletedObject::default();
                d.key = Some(key);
                deleted.push(d);
            }
        }

        self.invalidate(&bucket).await;

        let mut out = DeleteObjectsOutput::default();
        out.deleted = if deleted.is_empty() { None } else { Some(deleted) };
        out.errors = if errors_out.is_empty() { None } else { Some(errors_out) };
        Ok(S3Response::new(out))
    }

    async fn abort_multipart_upload(&self, req: S3Request<AbortMultipartUploadInput>) -> S3Result<S3Response<AbortMultipartUploadOutput>> {
        let upload_id = req.input.upload_id;
        self.multiparts.lock().await.remove(&upload_id);
        Ok(S3Response::new(AbortMultipartUploadOutput::default()))
    }
}

fn uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{:x}{:x}", t.as_secs(), t.subsec_nanos())
}
