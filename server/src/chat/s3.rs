//! S3 Storage Client
//!
//! Handles S3-compatible storage for file uploads.
//! Supports any S3-compatible backend: AWS S3, MinIO, Backblaze B2, Cloudflare R2.

use aws_config::Region;
use aws_sdk_s3::{
    config::{Credentials, IdentityCache, SharedCredentialsProvider, StalledStreamProtectionConfig},
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client,
};
use std::time::Duration;
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
    /// Supports custom endpoints for S3-compatible backends (MinIO, R2, B2).
    /// Uses path-style addressing when a custom endpoint is configured.
    pub async fn new(config: &Config) -> Result<Self, S3Error> {
        let region = Region::new(
            std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
        );

        let mut s3_config_builder = aws_sdk_s3::Config::builder()
            .region(region)
            .stalled_stream_protection(StalledStreamProtectionConfig::disabled())
            .identity_cache(IdentityCache::no_cache());

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
    pub async fn upload(&self, key: &str, data: Vec<u8>, content_type: &str) -> Result<(), S3Error> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(ByteStream::from(data))
            .content_type(content_type)
            .send()
            .await
            .map_err(|e| S3Error::Upload(e.to_string()))?;

        Ok(())
    }

    /// Generate a presigned URL for downloading a file.
    ///
    /// The URL is valid for the configured expiry duration.
    pub async fn presign_get(&self, key: &str) -> Result<String, S3Error> {
        let presign_config = PresigningConfig::builder()
            .expires_in(self.presign_expiry)
            .build()
            .map_err(|e| S3Error::Presign(e.to_string()))?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presign_config)
            .await
            .map_err(|e| S3Error::Presign(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Delete a file from S3.
    pub async fn delete(&self, key: &str) -> Result<(), S3Error> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| S3Error::Delete(e.to_string()))?;

        Ok(())
    }

    /// Check if the bucket is accessible (health check).
    pub async fn health_check(&self) -> Result<(), S3Error> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|e| S3Error::Config(format!("Bucket not accessible: {}", e)))?;

        Ok(())
    }

    /// Get the bucket name.
    pub fn bucket(&self) -> &str {
        &self.bucket
    }
}
