use async_trait::async_trait;
use s3s::dto::*;
use s3s::{S3Error, S3Result, S3, S3Request, S3Response};
use crate::HamrahClient;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

pub struct HamrahS3Backend {
    clients: Arc<Mutex<HashMap<String, HamrahClient>>>,
}

impl HamrahS3Backend {
    pub fn new(clients: HashMap<String, HamrahClient>) -> Self {
        Self {
            clients: Arc::new(Mutex::new(clients)),
        }
    }
}

#[async_trait]
impl S3 for HamrahS3Backend {
    async fn list_objects_v2(&self, req: S3Request<ListObjectsV2Input>) -> S3Result<S3Response<ListObjectsV2Output>> {
        let input = req.input;
        let clients = self.clients.lock().await;
        let client = clients.get(&input.bucket).ok_or(S3Error::with_message(s3s::S3ErrorCode::NoSuchBucket, "Bucket not found"))?;
        
        let objects = client.list_objects().await.map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;
        
        let contents: Vec<Object> = objects.into_iter().map(|obj| {
            let mut s3_obj = Object::default();
            s3_obj.key = Some(obj.name);
            s3_obj.size = Some(0); 
            s3_obj
        }).collect();

        let mut output = ListObjectsV2Output::default();
        output.contents = Some(contents);
        output.key_count = Some(output.contents.as_ref().map(|c| c.len() as i32).unwrap_or(0));
        
        Ok(S3Response::new(output))
    }

    async fn put_object(&self, req: S3Request<PutObjectInput>) -> S3Result<S3Response<PutObjectOutput>> {
        let input = req.input;
        let clients = self.clients.lock().await;
        let client = clients.get(&input.bucket).ok_or(S3Error::with_message(s3s::S3ErrorCode::NoSuchBucket, "Bucket not found"))?;
        
        let key = input.key;
        let body = input.body.ok_or(S3Error::with_message(s3s::S3ErrorCode::InvalidRequest, "Missing body"))?;
        
        // Convert stream to bytes
        use futures_util::StreamExt;
        let mut data = Vec::new();
        let mut stream = body;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "Stream error"))?;
            data.extend_from_slice(&chunk);
        }

        // Save to temp file and upload
        let temp_path = std::env::temp_dir().join(&key);
        std::fs::write(&temp_path, data).map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "File write error"))?;
        
        client.upload_file(&temp_path).await.map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "Upload error"))?;
        
        std::fs::remove_file(temp_path).ok();

        Ok(S3Response::new(PutObjectOutput::default()))
    }

    async fn head_object(&self, req: S3Request<HeadObjectInput>) -> S3Result<S3Response<HeadObjectOutput>> {
        let input = req.input;
        let clients = self.clients.lock().await;
        let client = clients.get(&input.bucket).ok_or(S3Error::with_message(s3s::S3ErrorCode::NoSuchBucket, "Bucket not found"))?;
        
        let objects = client.list_objects().await.map_err(|_| S3Error::with_message(s3s::S3ErrorCode::InternalError, "API error"))?;
        
        if let Some(_obj) = objects.iter().find(|o| o.name == input.key) {
            Ok(S3Response::new(HeadObjectOutput::default()))
        } else {
            Err(S3Error::with_message(s3s::S3ErrorCode::NoSuchKey, "Key not found"))
        }
    }
}
