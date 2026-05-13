use async_trait::async_trait;
use s3_server::dto::*;
use s3_server::{S3Error, S3Result, S3};
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
    async fn list_objects_v2(&self, input: ListObjectsV2Input) -> S3Result<ListObjectsV2Output> {
        let clients = self.clients.lock().await;
        let client = clients.get(&input.bucket).ok_or(S3Error::new(S3ErrorCode::NoSuchBucket))?;
        
        let objects = client.list_objects().await.map_err(|_| S3Error::new(S3ErrorCode::InternalError))?;
        
        let contents: Vec<Object> = objects.into_iter().map(|obj| {
            let mut s3_obj = Object::default();
            s3_obj.key = Some(obj.name);
            s3_obj.size = Some(0); 
            s3_obj
        }).collect();

        let mut output = ListObjectsV2Output::default();
        output.contents = Some(contents);
        output.key_count = Some(output.contents.as_ref().map(|c| c.len() as i32).unwrap_or(0));
        
        Ok(output)
    }

    async fn put_object(&self, input: PutObjectInput) -> S3Result<PutObjectOutput> {
        let clients = self.clients.lock().await;
        let client = clients.get(&input.bucket).ok_or(S3Error::new(S3ErrorCode::NoSuchBucket))?;
        
        let key = input.key;
        let body = input.body.ok_or(S3Error::new(S3ErrorCode::InvalidRequest))?;
        
        // Convert stream to bytes
        use futures_util::StreamExt;
        let mut data = Vec::new();
        let mut stream = body;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| S3Error::new(S3ErrorCode::InternalError))?;
            data.extend_from_slice(&chunk);
        }

        // Save to temp file and upload
        let temp_path = std::env::temp_dir().join(&key);
        std::fs::write(&temp_path, data).map_err(|_| S3Error::new(S3ErrorCode::InternalError))?;
        
        client.upload_file(&temp_path).await.map_err(|_| S3Error::new(S3ErrorCode::InternalError))?;
        
        std::fs::remove_file(temp_path).ok();

        Ok(PutObjectOutput::default())
    }

    async fn head_object(&self, input: HeadObjectInput) -> S3Result<HeadObjectOutput> {
        let clients = self.clients.lock().await;
        let client = clients.get(&input.bucket).ok_or(S3Error::new(S3ErrorCode::NoSuchBucket))?;
        
        let objects = client.list_objects().await.map_err(|_| S3Error::new(S3ErrorCode::InternalError))?;
        
        if let Some(_obj) = objects.iter().find(|o| o.name == input.key) {
            Ok(HeadObjectOutput::default())
        } else {
            Err(S3Error::new(S3ErrorCode::NoSuchKey))
        }
    }
}
