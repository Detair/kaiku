//! S3 Storage Client
//!
//! Handles S3-compatible storage for file uploads.
//! Supports any S3-compatible backend: AWS S3, `MinIO`, Backblaze B2, Cloudflare R2.

use aws_config::Region;
use aws_sdk_s3::{
    config::{
        Credentials, IdentityCache, SharedCredentialsProvider, StalledStreamProtectionConfig,
    },
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client,
};
use aws_smithy_async::rt::sleep::TokioSleep;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::info;

use crate::config::Config;

/// S3 client wrapper with configuration.
#[derive(Clone)]
pub struct S3Client {
    client: Client,
    bucket: String,
    presign_expiry: Duration,
}

/// S3-related errors.
#[derive(Debug, Error)]
pub enum S3Error {
    /// Failed to upload file.
    #[error("Failed to upload file: {0}")]
    Upload(String),

    /// Failed to download file.
    #[error("Failed to download file: {0}")]
    Download(String),

    /// Failed to generate presigned URL.
    #[error("Failed to generate presigned URL: {0}")]
    Presign(String),

    /// Failed to delete file.
    #[error("Failed to delete file: {0}")]
    Delete(String),

    /// S3 configuration error.
    #[error("S3 configuration error: {0}")]
    Config(String),
}

impl S3Client {
    /// Create a new S3 client from configuration.
    ///
    /// Supports custom endpoints for S3-compatible backends (`MinIO`, R2, B2).
    /// Uses path-style addressing when a custom endpoint is configured.
    pub async fn new(config: &Config) -> Result<Self, S3Error> {
        let region =
            Region::new(std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()));

        let mut s3_config_builder = aws_sdk_s3::Config::builder()
            .region(region)
            .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
            .identity_cache(IdentityCache::no_cache())
            // Provide tokio sleep implementation for timeouts
            .sleep_impl(Arc::new(TokioSleep::new()))
            .behavior_version_latest();

        // Configure credentials from environment
        if let (Ok(access_key), Ok(secret_key)) = (
            std::env::var("AWS_ACCESS_KEY_ID"),
            std::env::var("AWS_SECRET_ACCESS_KEY"),
        ) {
            let credentials = Credentials::new(
                access_key,
                secret_key,
                None, // session token
                None, // expiry
                "environment",
            );
            s3_config_builder =
                s3_config_builder.credentials_provider(SharedCredentialsProvider::new(credentials));
        }

        // Configure custom endpoint for S3-compatible backends
        if let Some(endpoint) = &config.s3_endpoint {
            s3_config_builder = s3_config_builder
                .endpoint_url(endpoint)
                .force_path_style(true); // Required for MinIO and most S3-compatible backends
        }

        let s3_config = s3_config_builder.build();
        let client = Client::from_conf(s3_config);

        info!(
            bucket = %config.s3_bucket,
            endpoint = ?config.s3_endpoint,
            "S3 client initialized"
        );

        Ok(Self {
            client,
            bucket: config.s3_bucket.clone(),
            presign_expiry: Duration::from_secs(config.s3_presign_expiry as u64),
        })
    }

    /// Upload a file to S3.
    ///
    /// # Arguments
    /// * `key` - The S3 object key (path)
    /// * `data` - File contents as bytes
    /// * `content_type` - MIME type of the file
    ///
    /// # Timeout
    /// Operations are protected by a 30-second timeout to prevent slow S3
    /// responses from blocking tokio threads and cascading failures.
    pub async fn upload(
        &self,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<(), S3Error> {
        let upload_future = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(data))
            .content_type(content_type)
            .send();

        tokio::time::timeout(Duration::from_secs(30), upload_future)
            .await
            .map_err(|_| S3Error::Upload("S3 upload timed out after 30 seconds".to_string()))?
            .map_err(|e| S3Error::Upload(e.to_string()))?;

        Ok(())
    }

    /// Generate a presigned URL for downloading a file.
    ///
    /// The URL is valid for the configured expiry duration.
    /// Protected by a 10-second timeout.
    pub async fn presign_get(&self, key: &str) -> Result<String, S3Error> {
        let presign_config = PresigningConfig::builder()
            .expires_in(self.presign_expiry)
            .build()
            .map_err(|e| S3Error::Presign(e.to_string()))?;

        let presign_future = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presign_config);

        let presigned = tokio::time::timeout(Duration::from_secs(10), presign_future)
            .await
            .map_err(|_| S3Error::Presign("S3 presign timed out after 10 seconds".to_string()))?
            .map_err(|e| S3Error::Presign(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Delete a file from S3.
    ///
    /// Protected by a 30-second timeout.
    pub async fn delete(&self, key: &str) -> Result<(), S3Error> {
        let delete_future = self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send();

        tokio::time::timeout(Duration::from_secs(30), delete_future)
            .await
            .map_err(|_| S3Error::Delete("S3 delete timed out after 30 seconds".to_string()))?
            .map_err(|e| S3Error::Delete(e.to_string()))?;

        Ok(())
    }

    /// Check if the bucket is accessible (health check).
    ///
    /// Protected by a 10-second timeout.
    pub async fn health_check(&self) -> Result<(), S3Error> {
        let health_future = self.client.head_bucket().bucket(&self.bucket).send();

        tokio::time::timeout(Duration::from_secs(10), health_future)
            .await
            .map_err(|_| S3Error::Config("S3 health check timed out after 10 seconds".to_string()))?
            .map_err(|e| S3Error::Config(format!("Bucket not accessible: {e}")))?;

        Ok(())
    }

    /// Get the raw object stream for a file (for proxying).
    ///
    /// Protected by a 30-second timeout for initial response.
    /// Note: Streaming the body itself may take longer for large files.
    pub async fn get_object_stream(&self, key: &str) -> Result<ByteStream, S3Error> {
        let get_future = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send();

        let output = tokio::time::timeout(Duration::from_secs(30), get_future)
            .await
            .map_err(|_| S3Error::Download("S3 download timed out after 30 seconds".to_string()))?
            .map_err(|e| S3Error::Download(e.to_string()))?;

        Ok(output.body)
    }

    /// Get the bucket name.
    #[must_use]
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
}
