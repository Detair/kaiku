//! Server Configuration
//!
//! Loads configuration from environment variables.

use anyhow::{Context, Result};
use std::env;

/// Server configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Server bind address (e.g., "0.0.0.0:8080")
    pub bind_address: String,

    /// `PostgreSQL` connection URL
    pub database_url: String,

    /// Valkey/Redis connection URL (uses redis:// protocol)
    pub redis_url: String,

    /// JWT private key (PEM format, base64 encoded) for signing tokens
    pub jwt_private_key: String,

    /// JWT public key (PEM format, base64 encoded) for verifying tokens
    pub jwt_public_key: String,

    /// JWT access token expiry in seconds (default: 900 = 15 min)
    pub jwt_access_expiry: i64,

    /// JWT refresh token expiry in seconds (default: 604800 = 7 days)
    pub jwt_refresh_expiry: i64,

    /// S3-compatible storage endpoint
    pub s3_endpoint: Option<String>,

    /// S3 bucket name
    pub s3_bucket: String,

    /// S3 presigned URL expiry in seconds (default: 3600 = 1 hour)
    pub s3_presign_expiry: i64,

    /// Allowed MIME types for file uploads (comma-separated)
    pub allowed_mime_types: Option<Vec<String>>,

    /// OIDC issuer URL (optional)
    pub oidc_issuer_url: Option<String>,

    /// OIDC client ID (optional)
    pub oidc_client_id: Option<String>,

    /// OIDC client secret (optional)
    pub oidc_client_secret: Option<String>,

    /// Maximum file upload size in bytes (default: 50MB)
    ///
    /// Used by DefaultBodyLimit middleware as final safety net for all uploads.
    /// Should be ≥ all specific upload limits (avatar, emoji).
    pub max_upload_size: usize,

    /// Maximum avatar size in bytes (user profiles and DM groups, default: 5MB)
    ///
    /// Validated by upload handlers before processing.
    /// Must be ≤ max_upload_size to avoid middleware rejection.
    pub max_avatar_size: usize,

    /// Maximum emoji size in bytes (guild custom emojis, default: 256KB)
    ///
    /// Validated by upload handlers before processing.
    /// Must be ≤ max_upload_size to avoid middleware rejection.
    pub max_emoji_size: usize,

    /// WebRTC STUN server
    pub stun_server: String,

    /// WebRTC TURN server (optional)
    pub turn_server: Option<String>,

    /// WebRTC TURN username (optional)
    pub turn_username: Option<String>,

    /// WebRTC TURN credential (optional)
    pub turn_credential: Option<String>,

    /// MFA secret encryption key (32-byte hex string)
    pub mfa_encryption_key: Option<String>,

    /// Whether E2EE setup is required before using the app (default: false)
    pub require_e2ee_setup: bool,

    /// Allowed CORS origins (comma-separated, default: "*" for dev)
    /// Set to specific origins in production (e.g., "https://app.example.com")
    pub cors_allowed_origins: Vec<String>,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into()),
            jwt_private_key: env::var("JWT_PRIVATE_KEY").context("JWT_PRIVATE_KEY must be set (base64-encoded PEM)")?,
            jwt_public_key: env::var("JWT_PUBLIC_KEY").context("JWT_PUBLIC_KEY must be set (base64-encoded PEM)")?,
            jwt_access_expiry: env::var("JWT_ACCESS_EXPIRY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(900),
            jwt_refresh_expiry: env::var("JWT_REFRESH_EXPIRY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(604800),
            s3_endpoint: env::var("S3_ENDPOINT").ok(),
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "voicechat".into()),
            s3_presign_expiry: env::var("S3_PRESIGN_EXPIRY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600), // 1 hour
            allowed_mime_types: env::var("ALLOWED_MIME_TYPES").ok().map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            }),
            oidc_issuer_url: env::var("OIDC_ISSUER_URL").ok(),
            oidc_client_id: env::var("OIDC_CLIENT_ID").ok(),
            oidc_client_secret: env::var("OIDC_CLIENT_SECRET").ok(),
            max_upload_size: env::var("MAX_UPLOAD_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50 * 1024 * 1024), // 50MB
            max_avatar_size: env::var("MAX_AVATAR_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5 * 1024 * 1024), // 5MB
            max_emoji_size: env::var("MAX_EMOJI_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(256 * 1024), // 256KB
            stun_server: env::var("STUN_SERVER")
                .unwrap_or_else(|_| "stun:stun.l.google.com:19302".into()),
            turn_server: env::var("TURN_SERVER").ok(),
            turn_username: env::var("TURN_USERNAME").ok(),
            turn_credential: env::var("TURN_CREDENTIAL").ok(),
            mfa_encryption_key: env::var("MFA_ENCRYPTION_KEY").ok(),
            require_e2ee_setup: env::var("REQUIRE_E2EE_SETUP")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            cors_allowed_origins: env::var("CORS_ALLOWED_ORIGINS")
                .ok()
                .map(|s| {
                    s.split(',')
                        .map(|o| o.trim().to_string())
                        .filter(|o| !o.is_empty())
                        .collect()
                })
                .unwrap_or_else(|| vec!["*".to_string()]),
        })
    }

    /// Check if OIDC is configured.
    #[must_use]
    pub const fn has_oidc(&self) -> bool {
        self.oidc_issuer_url.is_some()
            && self.oidc_client_id.is_some()
            && self.oidc_client_secret.is_some()
    }

    /// Check if TURN is configured.
    #[must_use]
    pub const fn has_turn(&self) -> bool {
        self.turn_server.is_some()
    }

    /// Create a default configuration for testing.
    ///
    /// Uses Docker test containers:
    /// - `PostgreSQL`: `docker run -d --name canis-test-postgres -e POSTGRESQL_USERNAME=test -e POSTGRESQL_PASSWORD=test -e POSTGRESQL_DATABASE=test -p 5434:5432 bitnami/postgresql:latest`
    /// - Redis: `docker run -d --name canis-test-redis -e ALLOW_EMPTY_PASSWORD=yes -p 6380:6379 bitnami/redis:latest`
    ///
    /// Run migrations: `DATABASE_URL="postgresql://test:test@localhost:5434/test" sqlx migrate run --source server/migrations`
    #[must_use]
    pub fn default_for_test() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".into(),
            // Uses dev database - sqlx::test creates isolated DBs automatically
            database_url: "postgresql://voicechat:voicechat_dev@localhost:5433/voicechat".into(),
            // Uses dev Redis
            redis_url: "redis://localhost:6379".into(),
            // Test RSA key pair (2048-bit, generated for testing only)
            jwt_private_key: TEST_JWT_PRIVATE_KEY.into(),
            jwt_public_key: TEST_JWT_PUBLIC_KEY.into(),
            jwt_access_expiry: 900,
            jwt_refresh_expiry: 604800,
            s3_endpoint: None,
            s3_bucket: "test-bucket".into(),
            s3_presign_expiry: 3600,
            allowed_mime_types: None,
            max_upload_size: 50 * 1024 * 1024,
            max_avatar_size: 5 * 1024 * 1024,
            max_emoji_size: 256 * 1024,
            oidc_issuer_url: None,
            oidc_client_id: None,
            oidc_client_secret: None,
            stun_server: "stun:stun.l.google.com:19302".into(),
            turn_server: None,
            turn_username: None,
            turn_credential: None,
            mfa_encryption_key: None,
            require_e2ee_setup: false,
            cors_allowed_origins: vec!["*".to_string()],
        }
    }
}

// Test Ed25519 key pair - DO NOT USE IN PRODUCTION
// Generated with: openssl genpkey -algorithm Ed25519 -out ed25519_private.pem
//                 openssl pkey -in ed25519_private.pem -pubout -out ed25519_public.pem
// Then base64-encoded for storage in environment variables

/// Test private key (base64-encoded PEM) - Ed25519
const TEST_JWT_PRIVATE_KEY: &str = "LS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0tCk1DNENBUUF3QlFZREsyVndCQ0lFSUZuUDFodDNNcjlkOGJyYW4zV2IyTGFxSStqd2NnY0V4YXp2V0pQNWUrSG8KLS0tLS1FTkQgUFJJVkFURSBLRVktLS0tLQo=";

/// Test public key (base64-encoded PEM) - Ed25519
const TEST_JWT_PUBLIC_KEY: &str = "LS0tLS1CRUdJTiBQVUJMSUMgS0VZLS0tLS0KTUNvd0JRWURLMlZ3QXlFQW80TlJjVnQ2ajF3OHRCWUtxUEJzS0krNUZVREkwVGtJaHF4WWlud05TRlU9Ci0tLS0tRU5EIFBVQkxJQyBLRVktLS0tLQo=";
