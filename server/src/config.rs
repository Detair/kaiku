//! Server Configuration
//!
//! Loads configuration from environment variables.

use std::env;

use anyhow::{Context, Result};

/// Observability and telemetry configuration.
#[derive(Debug, Clone)]
pub struct ObservabilityConfig {
    /// Whether observability/telemetry is enabled (env: `OBSERVABILITY_ENABLED`, default: false)
    pub enabled: bool,

    /// OpenTelemetry OTLP exporter endpoint (env: `OTEL_EXPORTER_OTLP_ENDPOINT`, default: `"http://localhost:4317"`)
    pub otlp_endpoint: String,

    /// Service name for telemetry (env: `OTEL_SERVICE_NAME`, default: `"vc-server"`)
    pub service_name: String,

    /// Trace sampling ratio (0.0-1.0) (env: `OTEL_TRACES_SAMPLER_ARG`, default: 0.1)
    pub trace_sample_ratio: f64,

    /// Log level filter (env: `RUST_LOG`, default: `"vc_server=info"`)
    pub log_level: String,
}

impl ObservabilityConfig {
    /// Load observability configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            enabled: env::var("OBSERVABILITY_ENABLED")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            otlp_endpoint: env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".into()),
            service_name: env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "vc-server".into()),
            trace_sample_ratio: env::var("OTEL_TRACES_SAMPLER_ARG")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.1),
            log_level: env::var("RUST_LOG").unwrap_or_else(|_| "vc_server=info".into()),
        }
    }
}

/// Server configuration loaded from environment variables.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
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

    /// Public S3 base URL for client-facing file URLs (e.g., avatar images).
    /// When set, file URLs use this instead of the internal S3 endpoint.
    /// Example: `https://kaiku.pmind.de/s3`
    pub s3_public_url: Option<String>,

    /// S3 access key ID (optional, falls back to `AWS_ACCESS_KEY_ID` env var)
    pub s3_access_key: Option<String>,

    /// S3 secret access key (optional, falls back to `AWS_SECRET_ACCESS_KEY` env var)
    pub s3_secret_key: Option<String>,

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
    /// Used by `DefaultBodyLimit` middleware as final safety net for all uploads.
    /// Should be ≥ all specific upload limits (avatar, emoji).
    pub max_upload_size: usize,

    /// Maximum avatar size in bytes (user profiles and DM groups, default: 5MB)
    ///
    /// Validated by upload handlers before processing.
    /// Must be ≤ `max_upload_size` to avoid middleware rejection.
    pub max_avatar_size: usize,

    /// Maximum emoji size in bytes (guild custom emojis, default: 256KB)
    ///
    /// Validated by upload handlers before processing.
    /// Must be ≤ `max_upload_size` to avoid middleware rejection.
    pub max_emoji_size: usize,

    /// WebRTC STUN server
    pub stun_server: String,

    /// WebRTC TURN server (optional)
    pub turn_server: Option<String>,

    /// WebRTC TURN username (optional)
    pub turn_username: Option<String>,

    /// WebRTC TURN credential (optional)
    pub turn_credential: Option<String>,

    /// Shared secret for TURN HMAC credential generation (coturn `use-auth-secret` mode).
    /// When set, time-limited credentials are generated per-request instead of using static ones.
    pub turn_shared_secret: Option<String>,

    /// TURN credential TTL in seconds (default: 3600 = 1 hour).
    pub turn_credential_ttl: u64,

    /// Public IP for WebRTC NAT traversal.
    /// Required when the SFU runs behind NAT/Docker so ICE candidates
    /// advertise the reachable IP instead of the container-internal IP.
    pub public_ip: Option<String>,

    /// MFA secret encryption key (32-byte hex string)
    pub mfa_encryption_key: Option<String>,

    /// Whether E2EE setup is required before using the app (default: false)
    pub require_e2ee_setup: bool,

    /// Whether to fail open (allow actions) when Redis block checks fail (default: false)
    ///
    /// When false (recommended): Block checks fail-closed, rejecting DMs/calls if Redis is
    /// unavailable. When true: Block checks fail-open, allowing actions if Redis is
    /// unavailable (prioritizes availability).
    pub block_check_fail_open: bool,

    /// Allowed CORS origins (comma-separated, default: "*" for dev)
    /// Set to specific origins in production (e.g., "<https://app.example.com>")
    pub cors_allowed_origins: Vec<String>,

    /// Whether to set the Secure flag on auth cookies (default: true in release, false in debug).
    /// Override via `COOKIE_SECURE` env var.
    pub cookie_secure: bool,

    /// Optional domain for auth cookies. If unset, cookies are scoped to the request host.
    /// Override via `COOKIE_DOMAIN` env var.
    pub cookie_domain: Option<String>,

    /// `SameSite` policy for auth cookies: `lax`, `strict`, or `none` (default: `lax`).
    /// Use `strict` for same-origin deployments, `none` for cross-origin (requires Secure).
    /// Override via `COOKIE_SAMESITE` env var.
    pub cookie_same_site: String,

    /// SMTP server hostname (optional, enables password reset emails)
    pub smtp_host: Option<String>,

    /// SMTP server port (default: 587)
    pub smtp_port: u16,

    /// SMTP username (required if SMTP is enabled)
    pub smtp_username: Option<String>,

    /// SMTP password (required if SMTP is enabled)
    pub smtp_password: Option<String>,

    /// SMTP sender address (e.g., "noreply@example.com")
    pub smtp_from: Option<String>,

    /// SMTP TLS mode: "starttls" (default), "tls", or "none"
    pub smtp_tls: String,

    /// Whether to enable API documentation (Swagger UI) at /api/docs
    ///
    /// Defaults to `true` in debug builds, `false` in release builds.
    /// Override via `ENABLE_API_DOCS` env var ("true"/"1" to enable, "false"/"0" to disable).
    pub enable_api_docs: bool,

    /// Whether to enable the guild discovery endpoint for browsing public guilds.
    ///
    /// Defaults to `true`. Override via `ENABLE_GUILD_DISCOVERY` env var.
    pub enable_guild_discovery: bool,

    // ========================================================================
    // Resource Limits
    // ========================================================================
    /// Maximum number of guilds a single user can own (default: 100)
    pub max_guilds_per_user: i64,

    /// Maximum number of members per guild (default: 1000)
    pub max_members_per_guild: i64,

    /// Maximum number of channels per guild (default: 200)
    pub max_channels_per_guild: i64,

    /// Maximum number of roles per guild (default: 50)
    pub max_roles_per_guild: i64,

    /// Maximum number of custom emojis per guild (default: 50)
    pub max_emojis_per_guild: i64,

    /// Maximum number of bot installations per guild (default: 10)
    pub max_bots_per_guild: i64,

    /// Maximum number of webhooks per bot application (default: 5)
    pub max_webhooks_per_app: i64,

    /// Maximum number of personal workspaces per user (default: 20)
    pub max_workspaces_per_user: i64,

    /// Maximum number of entries per workspace (default: 50)
    pub max_entries_per_workspace: i64,

    /// Maximum number of pages per guild (default: 10)
    pub max_pages_per_guild: i64,

    /// Maximum number of revisions per page (default: 25)
    pub max_revisions_per_page: i64,

    /// Observability and telemetry configuration
    pub observability: ObservabilityConfig,

    // ========================================================================
    // Command Center
    // ========================================================================
    /// Deployment environment label (e.g., "production", "staging", "development")
    pub environment: String,

    /// Grafana dashboard URL (optional)
    pub grafana_url: Option<String>,

    /// Tempo tracing UI URL (optional)
    pub tempo_url: Option<String>,

    /// Loki log viewer URL (optional)
    pub loki_url: Option<String>,

    /// Prometheus UI URL (optional)
    pub prometheus_url: Option<String>,

    /// Geo-IP API URL template (optional). Use `{ip}` as placeholder for the IP address.
    /// Default: `http://ip-api.com/json/{ip}?fields=city,country`
    pub geoip_api_url: Option<String>,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let config = Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into()),
            jwt_private_key: env::var("JWT_PRIVATE_KEY")
                .context("JWT_PRIVATE_KEY must be set (base64-encoded PEM)")?,
            jwt_public_key: env::var("JWT_PUBLIC_KEY")
                .context("JWT_PUBLIC_KEY must be set (base64-encoded PEM)")?,
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
            s3_public_url: env::var("S3_PUBLIC_URL").ok(),
            s3_presign_expiry: env::var("S3_PRESIGN_EXPIRY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600) // 1 hour
                .max(1),
            s3_access_key: env::var("AWS_ACCESS_KEY_ID").ok(),
            s3_secret_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
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
            turn_server: env::var("TURN_SERVER").ok().filter(|s| !s.is_empty()),
            turn_username: env::var("TURN_USERNAME").ok().filter(|s| !s.is_empty()),
            turn_credential: env::var("TURN_CREDENTIAL").ok().filter(|s| !s.is_empty()),
            turn_shared_secret: env::var("TURN_SHARED_SECRET")
                .ok()
                .filter(|s| !s.is_empty()),
            turn_credential_ttl: env::var("TURN_CREDENTIAL_TTL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
            public_ip: env::var("PUBLIC_IP").ok(),
            mfa_encryption_key: env::var("MFA_ENCRYPTION_KEY").ok(),
            require_e2ee_setup: env::var("REQUIRE_E2EE_SETUP")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(false),
            block_check_fail_open: env::var("BLOCK_CHECK_FAIL_OPEN")
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
            cookie_secure: env::var("COOKIE_SECURE")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(!cfg!(debug_assertions)),
            cookie_domain: env::var("COOKIE_DOMAIN").ok().filter(|d| !d.is_empty()),
            cookie_same_site: {
                let value = env::var("COOKIE_SAMESITE")
                    .unwrap_or_else(|_| "lax".to_string())
                    .to_lowercase();
                anyhow::ensure!(
                    matches!(value.as_str(), "lax" | "strict" | "none"),
                    "Invalid COOKIE_SAMESITE value '{value}'. Must be one of: lax, strict, none"
                );
                value
            },
            smtp_host: env::var("SMTP_HOST").ok(),
            smtp_port: env::var("SMTP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(587),
            smtp_username: env::var("SMTP_USERNAME").ok(),
            smtp_password: env::var("SMTP_PASSWORD").ok(),
            smtp_from: env::var("SMTP_FROM").ok(),
            smtp_tls: env::var("SMTP_TLS").unwrap_or_else(|_| "starttls".into()),
            enable_api_docs: env::var("ENABLE_API_DOCS")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(cfg!(debug_assertions)),
            enable_guild_discovery: env::var("ENABLE_GUILD_DISCOVERY")
                .ok()
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(true),
            max_guilds_per_user: env::var("MAX_GUILDS_PER_USER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100)
                .max(1),
            max_members_per_guild: env::var("MAX_MEMBERS_PER_GUILD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000)
                .max(1),
            max_channels_per_guild: env::var("MAX_CHANNELS_PER_GUILD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(200)
                .max(1),
            max_roles_per_guild: env::var("MAX_ROLES_PER_GUILD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50)
                .max(1),
            max_emojis_per_guild: env::var("MAX_EMOJIS_PER_GUILD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50)
                .max(1),
            max_bots_per_guild: env::var("MAX_BOTS_PER_GUILD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10)
                .max(1),
            max_webhooks_per_app: env::var("MAX_WEBHOOKS_PER_APP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5)
                .max(1),
            max_workspaces_per_user: env::var("MAX_WORKSPACES_PER_USER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20)
                .max(1),
            max_entries_per_workspace: env::var("MAX_ENTRIES_PER_WORKSPACE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50)
                .max(1),
            max_pages_per_guild: env::var("MAX_PAGES_PER_GUILD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10)
                .max(1),
            max_revisions_per_page: env::var("MAX_REVISIONS_PER_PAGE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(25)
                .max(1),
            observability: ObservabilityConfig::from_env(),
            environment: env::var("KAIKU_ENV").unwrap_or_else(|_| "production".into()),
            grafana_url: env::var("GRAFANA_URL").ok(),
            tempo_url: env::var("TEMPO_URL").ok(),
            loki_url: env::var("LOKI_URL").ok(),
            prometheus_url: env::var("PROMETHEUS_URL").ok(),
            geoip_api_url: env::var("GEOIP_API_URL")
                .ok()
                .or_else(|| Some("http://ip-api.com/json/{ip}?fields=city,country".to_string())),
        };

        // SameSite=None requires the Secure flag — browsers reject the cookie otherwise
        anyhow::ensure!(
            config.cookie_same_site != "none" || config.cookie_secure,
            "COOKIE_SAMESITE=none requires COOKIE_SECURE=true; \
             browsers will reject the refresh cookie without the Secure flag"
        );

        Ok(config)
    }

    /// Check if OIDC is configured.
    #[must_use]
    pub const fn has_oidc(&self) -> bool {
        self.oidc_issuer_url.is_some()
            && self.oidc_client_id.is_some()
            && self.oidc_client_secret.is_some()
    }

    /// Check if SMTP is configured for sending emails (password reset, etc.).
    #[must_use]
    pub const fn has_smtp(&self) -> bool {
        self.smtp_host.is_some()
            && self.smtp_username.is_some()
            && self.smtp_password.is_some()
            && self.smtp_from.is_some()
    }

    /// Check if TURN is configured.
    #[must_use]
    pub const fn has_turn(&self) -> bool {
        self.turn_server.is_some()
    }

    /// Create a default configuration for testing.
    ///
    /// Respects `DATABASE_URL` and `REDIS_URL` environment variables (for CI),
    /// falling back to local dev defaults.
    #[must_use]
    pub fn default_for_test() -> Self {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://voicechat:voicechat_dev@localhost:5433/voicechat".into()
        });
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into());

        Self {
            bind_address: "127.0.0.1:8080".into(),
            database_url,
            redis_url,
            // Test RSA key pair (2048-bit, generated for testing only)
            jwt_private_key: TEST_JWT_PRIVATE_KEY.into(),
            jwt_public_key: TEST_JWT_PUBLIC_KEY.into(),
            jwt_access_expiry: 900,
            jwt_refresh_expiry: 604800,
            s3_endpoint: None,
            s3_bucket: "test-bucket".into(),
            s3_public_url: None,
            s3_presign_expiry: 3600,
            s3_access_key: None,
            s3_secret_key: None,
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
            turn_shared_secret: None,
            turn_credential_ttl: 3600,
            public_ip: None,
            mfa_encryption_key: Some(TEST_MFA_ENCRYPTION_KEY.into()),
            require_e2ee_setup: false,
            block_check_fail_open: false,
            cors_allowed_origins: vec!["*".to_string()],
            cookie_secure: false,
            cookie_domain: None,
            cookie_same_site: "lax".to_string(),
            smtp_host: None,
            smtp_port: 587,
            smtp_username: None,
            smtp_password: None,
            smtp_from: None,
            smtp_tls: "starttls".into(),
            enable_api_docs: true,
            enable_guild_discovery: true,
            max_guilds_per_user: 100,
            max_members_per_guild: 1000,
            max_channels_per_guild: 200,
            max_roles_per_guild: 50,
            max_emojis_per_guild: 50,
            max_bots_per_guild: 10,
            max_webhooks_per_app: 5,
            max_workspaces_per_user: 20,
            max_entries_per_workspace: 50,
            max_pages_per_guild: 10,
            max_revisions_per_page: 25,
            observability: ObservabilityConfig {
                enabled: false,
                otlp_endpoint: "http://localhost:4317".into(),
                service_name: "vc-server".into(),
                trace_sample_ratio: 0.1,
                log_level: "vc_server=info".into(),
            },
            environment: "test".into(),
            grafana_url: None,
            tempo_url: None,
            loki_url: None,
            prometheus_url: None,
            geoip_api_url: None,
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

/// Test MFA encryption key (32-byte hex) - DO NOT USE IN PRODUCTION
/// Generated with: openssl rand -hex 32
const TEST_MFA_ENCRYPTION_KEY: &str =
    "a4f8e2d1b7c9036f5e8d4a2b1c7f9e3d6a8b5c2d4e7f1a3b9c6d8e2f5a7b4c";
