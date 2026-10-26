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

    /// Per-command Valkey timeout (`VALKEY_COMMAND_TIMEOUT_MS`, default 3 s).
    /// Commands that get no answer in time fail instead of waiting for Valkey.
    pub valkey_command_timeout: std::time::Duration,

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

    /// Lowest UDP port the SFU may bind for ICE/media (`VOICE_UDP_PORT_MIN`).
    /// Set together with `voice_udp_port_max`, or not at all (ephemeral ports).
    pub voice_udp_port_min: Option<u16>,

    /// Highest UDP port the SFU may bind for ICE/media (`VOICE_UDP_PORT_MAX`).
    pub voice_udp_port_max: Option<u16>,

    /// Allowed network interface names for ICE host candidates
    /// (`VOICE_ICE_INTERFACES`). `None` = all interfaces.
    pub voice_ice_interfaces: Option<Vec<String>>,

    /// IP family used for ICE candidates (`VOICE_ICE_IP_FAMILY`, default both).
    pub voice_ice_ip_family: IceIpFamily,

    /// Public base URL of this instance (e.g. "<https://kaiku.example.com>").
    /// Used to build absolute URLs handed to external services (incoming
    /// webhook execute URLs). Falls back to request Host headers when unset.
    pub public_base_url: Option<String>,

    /// Public URL of this server (`PUBLIC_URL`). OIDC providers always
    /// redirect to `{public_url}/auth/oidc/callback`; required for OIDC.
    pub public_url: Option<String>,

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

    /// FCM service account (env: `FCM_SERVICE_ACCOUNT_FILE` or
    /// `FCM_SERVICE_ACCOUNT_JSON`). `None` = FCM off.
    pub fcm_service_account: Option<crate::push::fcm::ServiceAccountKey>,

    /// FCM API base URL. Not an env var: always the Google endpoint outside
    /// tests, which point it at a local mock.
    pub fcm_api_base: String,

    /// Optional features turned on (env: `KAIKU_OPTIONAL_FEATURES`, comma
    /// list of `forum`, `announcements`, `events`, `screening`). Default: all off.
    pub optional_features: crate::features::OptionalFeatures,

    /// Unknown names found in `KAIKU_OPTIONAL_FEATURES`; logged as warnings at
    /// startup (config loads before logging is initialised).
    pub unknown_optional_features: Vec<String>,
}

/// Origins presented by the Tauri desktop webview (per platform/webview
/// flavour). They are added to every explicit CORS allow-list automatically.
pub const TAURI_ORIGINS: [&str; 3] = [
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
];

impl Config {
    /// Whether object-storage credentials are configured.
    ///
    /// Blank values count as unset, so an empty `AWS_ACCESS_KEY_ID=` from a
    /// compose template disables uploads instead of probing the default AWS
    /// endpoint at startup.
    #[must_use]
    pub fn has_s3_credentials(&self) -> bool {
        s3_credentials_present(self.s3_access_key.as_deref(), self.s3_secret_key.as_deref())
    }

    /// CORS origins the server actually allows.
    ///
    /// A wildcard configuration is returned unchanged (`["*"]`). An explicit
    /// list is extended with [`TAURI_ORIGINS`] (deduplicated), so self-hosters
    /// restricting origins do not have to know the desktop webview origins.
    #[must_use]
    pub fn effective_cors_origins(&self) -> Vec<String> {
        if self.cors_allowed_origins.iter().any(|o| o == "*") {
            return vec!["*".to_string()];
        }
        let mut origins: Vec<String> = Vec::new();
        for origin in self
            .cors_allowed_origins
            .iter()
            .map(String::as_str)
            .chain(TAURI_ORIGINS)
        {
            if !origins.iter().any(|o| o == origin) {
                origins.push(origin.to_string());
            }
        }
        origins
    }

    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let (voice_udp_port_min, voice_udp_port_max) = parse_voice_udp_port_range(
            env_opt("VOICE_UDP_PORT_MIN").as_deref(),
            env_opt("VOICE_UDP_PORT_MAX").as_deref(),
        )?;
        let voice_ice_interfaces =
            parse_voice_ice_interfaces(env_opt("VOICE_ICE_INTERFACES").as_deref());
        let voice_ice_ip_family =
            parse_voice_ice_ip_family(env_opt("VOICE_ICE_IP_FAMILY").as_deref())?;
        let fcm_service_account = crate::push::fcm::load_service_account(
            env_opt("FCM_SERVICE_ACCOUNT_FILE"),
            env_opt("FCM_SERVICE_ACCOUNT_JSON"),
        )?;
        let fcm_api_base = crate::push::fcm::FCM_API_BASE.to_string();
        let (optional_features, unknown_optional_features) =
            crate::features::OptionalFeatures::parse(
                &env::var("KAIKU_OPTIONAL_FEATURES").unwrap_or_default(),
            );
        let valkey_command_timeout =
            parse_valkey_command_timeout(env_opt("VALKEY_COMMAND_TIMEOUT_MS").as_deref())?;
        if let Some(key) = &fcm_service_account {
            // Refuse to start with a key the FCM client cannot use.
            crate::push::fcm::FcmClient::new(key.clone(), &fcm_api_base)?;
        }
        let config = Self {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            redis_url: env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into()),
            valkey_command_timeout,
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
            s3_endpoint: env_opt("S3_ENDPOINT"),
            s3_bucket: env::var("S3_BUCKET").unwrap_or_else(|_| "voicechat".into()),
            s3_public_url: env_opt("S3_PUBLIC_URL"),
            s3_presign_expiry: env::var("S3_PRESIGN_EXPIRY")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600) // 1 hour
                .max(1),
            s3_access_key: env_opt("AWS_ACCESS_KEY_ID"),
            s3_secret_key: env_opt("AWS_SECRET_ACCESS_KEY"),
            allowed_mime_types: env::var("ALLOWED_MIME_TYPES").ok().map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            }),
            oidc_issuer_url: env_opt("OIDC_ISSUER_URL"),
            oidc_client_id: env_opt("OIDC_CLIENT_ID"),
            oidc_client_secret: env_opt("OIDC_CLIENT_SECRET"),
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
            public_ip: env_opt("PUBLIC_IP"),
            voice_udp_port_min,
            voice_udp_port_max,
            voice_ice_interfaces,
            voice_ice_ip_family,
            public_base_url: env_opt("PUBLIC_BASE_URL"),
            public_url: env_opt("PUBLIC_URL"),
            mfa_encryption_key: env_opt("MFA_ENCRYPTION_KEY"),
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
            smtp_host: env_opt("SMTP_HOST"),
            smtp_port: env::var("SMTP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(587),
            smtp_username: env_opt("SMTP_USERNAME"),
            smtp_password: env_opt("SMTP_PASSWORD"),
            smtp_from: env_opt("SMTP_FROM"),
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
            grafana_url: env_opt("GRAFANA_URL"),
            tempo_url: env_opt("TEMPO_URL"),
            loki_url: env_opt("LOKI_URL"),
            prometheus_url: env_opt("PROMETHEUS_URL"),
            geoip_api_url: env::var("GEOIP_API_URL")
                .ok()
                .or_else(|| Some("http://ip-api.com/json/{ip}?fields=city,country".to_string())),
            fcm_service_account,
            fcm_api_base,
            optional_features,
            unknown_optional_features,
        };

        // SameSite=None requires the Secure flag — browsers reject the cookie otherwise
        anyhow::ensure!(
            config.cookie_same_site != "none" || config.cookie_secure,
            "COOKIE_SAMESITE=none requires COOKIE_SECURE=true; \
             browsers will reject the refresh cookie without the Secure flag"
        );

        // Sovereignty check: a self-hosted deployment that runs its own TURN
        // (coturn) but still points STUN at Google leaks every client's IP
        // discovery to a third party — defeating the point of self-hosting.
        // coturn serves STUN on the same host/port as TURN, so the operator
        // can almost always point STUN at it. Warn loudly rather than fail,
        // since some operators deliberately use a public STUN.
        if stun_leaks_with_self_hosted_turn(&config.stun_server, config.turn_server.as_deref()) {
            tracing::warn!(
                stun_server = %config.stun_server,
                "TURN is self-hosted but STUN still points at Google's public \
                 server — client IP discovery is leaking to a third party. Set \
                 STUN_SERVER to your coturn endpoint (e.g. stun:<your-domain>:3478)."
            );
        }

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
            valkey_command_timeout: crate::db::DEFAULT_VALKEY_COMMAND_TIMEOUT,
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
            voice_udp_port_min: None,
            voice_udp_port_max: None,
            voice_ice_interfaces: None,
            voice_ice_ip_family: IceIpFamily::Both,
            public_base_url: None,
            public_url: None,
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
            fcm_service_account: None,
            fcm_api_base: crate::push::fcm::FCM_API_BASE.into(),
            optional_features: crate::features::OptionalFeatures::default(),
            unknown_optional_features: Vec::new(),
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
    "a4f8e2d1b7c9036f5e8d4a2b1c7f9e3d6a8b5c2d4e7f1a3b9c6d8e2f5a7b4c1d";

/// Read an optional env var, treating unset AND empty/whitespace as `None`.
///
/// Compose `.env` templates declare optional vars as `NAME=` (empty string);
/// `env::var().ok()` surfaces that as `Some("")`, which downstream code takes
/// as "configured" — e.g. OIDC env seeding created a broken provider with an
/// empty issuer URL on the beta server.
fn env_opt(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// Exclusive upper bound for `VALKEY_COMMAND_TIMEOUT_MS`: the WebSocket
/// pub/sub setup must fail on its own before its 5 s ready timeout.
const MAX_VALKEY_COMMAND_TIMEOUT_MS: u64 = 5000;

/// Parse `VALKEY_COMMAND_TIMEOUT_MS`: milliseconds in `1..5000`; unset means
/// [`crate::db::DEFAULT_VALKEY_COMMAND_TIMEOUT`]. Anything else refuses
/// startup (`0` would disable the timeout).
fn parse_valkey_command_timeout(raw: Option<&str>) -> Result<std::time::Duration> {
    let Some(raw) = raw.map(str::trim) else {
        return Ok(crate::db::DEFAULT_VALKEY_COMMAND_TIMEOUT);
    };
    match raw.parse::<u64>() {
        Ok(ms) if ms > 0 && ms < MAX_VALKEY_COMMAND_TIMEOUT_MS => {
            Ok(std::time::Duration::from_millis(ms))
        }
        _ => anyhow::bail!(
            "Invalid VALKEY_COMMAND_TIMEOUT_MS value '{raw}'. Must be a number of milliseconds \
             from 1 to {} (below the 5000 ms WebSocket ready timeout)",
            MAX_VALKEY_COMMAND_TIMEOUT_MS - 1
        ),
    }
}

/// IP family allowed for ICE candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IceIpFamily {
    V4,
    V6,
    #[default]
    Both,
}

/// Parse `VOICE_ICE_INTERFACES`: comma-separated names, trimmed, empty entries
/// dropped; an empty list means unset (all interfaces).
fn parse_voice_ice_interfaces(raw: Option<&str>) -> Option<Vec<String>> {
    let names: Vec<String> = raw?
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .collect();
    (!names.is_empty()).then_some(names)
}

/// Parse `VOICE_ICE_IP_FAMILY`: `v4` | `v6` | `both` (case-insensitive).
fn parse_voice_ice_ip_family(raw: Option<&str>) -> Result<IceIpFamily> {
    match raw.map(|v| v.trim().to_lowercase()).as_deref() {
        None | Some("both") => Ok(IceIpFamily::Both),
        Some("v4") => Ok(IceIpFamily::V4),
        Some("v6") => Ok(IceIpFamily::V6),
        Some(other) => anyhow::bail!(
            "Invalid VOICE_ICE_IP_FAMILY value '{other}'. Must be one of: v4, v6, both"
        ),
    }
}

/// Both S3 credential halves present and non-blank.
fn s3_credentials_present(access: Option<&str>, secret: Option<&str>) -> bool {
    let present = |v: Option<&str>| v.is_some_and(|v| !v.trim().is_empty());
    present(access) && present(secret)
}

#[cfg(test)]
mod s3_credentials_tests {
    use super::s3_credentials_present;

    #[test]
    fn blank_s3_credentials_count_as_unset() {
        assert!(!s3_credentials_present(None, None));
        assert!(!s3_credentials_present(Some(""), Some("")));
        assert!(!s3_credentials_present(Some("  "), Some("secret")));
        assert!(!s3_credentials_present(Some("key"), Some("")));
        assert!(!s3_credentials_present(Some("key"), None));
        assert!(s3_credentials_present(Some("key"), Some("secret")));
    }
}

#[cfg(test)]
mod voice_ice_filter_tests {
    use super::*;

    #[test]
    fn interfaces_are_trimmed_and_empty_means_unset() {
        assert_eq!(parse_voice_ice_interfaces(None), None);
        assert_eq!(parse_voice_ice_interfaces(Some(" , ,")), None);
        assert_eq!(
            parse_voice_ice_interfaces(Some(" eth0, ,wt0 ")),
            Some(vec!["eth0".to_string(), "wt0".to_string()])
        );
    }

    #[test]
    fn ip_family_parses_case_insensitively() {
        assert_eq!(parse_voice_ice_ip_family(None).unwrap(), IceIpFamily::Both);
        assert_eq!(
            parse_voice_ice_ip_family(Some("V4")).unwrap(),
            IceIpFamily::V4
        );
        assert_eq!(
            parse_voice_ice_ip_family(Some("v6")).unwrap(),
            IceIpFamily::V6
        );
        assert_eq!(
            parse_voice_ice_ip_family(Some("Both")).unwrap(),
            IceIpFamily::Both
        );
    }

    #[test]
    fn ip_family_rejects_invalid_value() {
        let err = parse_voice_ice_ip_family(Some("ipv4"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("VOICE_ICE_IP_FAMILY") && err.contains("v4, v6, both"),
            "{err}"
        );
    }
}

const VOICE_UDP_RANGE_ERROR: &str =
    "VOICE_UDP_PORT_MIN and VOICE_UDP_PORT_MAX must be set together and min <= max";

/// Parse the optional SFU UDP port range. Both bounds or neither; `min <= max`.
/// Pure over its inputs so it is testable without touching process env.
fn parse_voice_udp_port_range(
    min: Option<&str>,
    max: Option<&str>,
) -> Result<(Option<u16>, Option<u16>)> {
    match (min, max) {
        (None, None) => Ok((None, None)),
        (Some(min), Some(max)) => {
            let min: u16 = min.trim().parse().context(VOICE_UDP_RANGE_ERROR)?;
            let max: u16 = max.trim().parse().context(VOICE_UDP_RANGE_ERROR)?;
            anyhow::ensure!(
                min >= 1,
                "{VOICE_UDP_RANGE_ERROR} (ports must be within 1-65535)"
            );
            anyhow::ensure!(min <= max, VOICE_UDP_RANGE_ERROR);
            Ok((Some(min), Some(max)))
        }
        _ => anyhow::bail!(VOICE_UDP_RANGE_ERROR),
    }
}

#[cfg(test)]
mod voice_udp_range_tests {
    use super::parse_voice_udp_port_range;

    #[test]
    fn config_rejects_half_set_udp_range() {
        for (min, max) in [(Some("50000"), None), (None, Some("50010"))] {
            let err = parse_voice_udp_port_range(min, max).unwrap_err();
            assert_eq!(
                err.to_string(),
                "VOICE_UDP_PORT_MIN and VOICE_UDP_PORT_MAX must be set together and min <= max"
            );
        }
    }

    #[test]
    fn config_rejects_inverted_udp_range() {
        let err = parse_voice_udp_port_range(Some("50010"), Some("50000")).unwrap_err();
        assert_eq!(
            err.to_string(),
            "VOICE_UDP_PORT_MIN and VOICE_UDP_PORT_MAX must be set together and min <= max"
        );
    }

    #[test]
    fn udp_range_rejects_port_zero() {
        let err = parse_voice_udp_port_range(Some("0"), Some("50010")).unwrap_err();
        assert!(err.to_string().contains("1-65535"), "{err}");
        assert!(parse_voice_udp_port_range(Some("0"), Some("0")).is_err());
    }

    #[test]
    fn udp_range_accepts_valid_and_unset() {
        assert_eq!(
            parse_voice_udp_port_range(None, None).unwrap(),
            (None, None)
        );
        assert_eq!(
            parse_voice_udp_port_range(Some("50000"), Some("50000")).unwrap(),
            (Some(50000), Some(50000))
        );
    }
}

#[cfg(test)]
mod env_opt_tests {
    use super::env_opt;

    #[test]
    fn unset_empty_and_whitespace_are_none() {
        // Env mutation is process-global; keep all cases in one test to
        // avoid parallel-test interference on the same variable.
        let var = "KAIKU_TEST_ENV_OPT";
        std::env::remove_var(var);
        assert_eq!(env_opt(var), None);
        std::env::set_var(var, "");
        assert_eq!(env_opt(var), None);
        std::env::set_var(var, "   ");
        assert_eq!(env_opt(var), None);
        std::env::set_var(var, "https://issuer.example.com");
        assert_eq!(env_opt(var), Some("https://issuer.example.com".to_owned()));
        std::env::remove_var(var);
    }
}

/// Returns true when a self-hosted TURN server is configured but STUN still
/// points at Google's public server — a sovereignty leak (client IP
/// discovery goes to a third party). Pure helper so the condition is unit
/// tested without a tracing subscriber.
fn stun_leaks_with_self_hosted_turn(stun_server: &str, turn_server: Option<&str>) -> bool {
    turn_server.is_some() && stun_server.contains("stun.l.google.com")
}

#[cfg(test)]
mod stun_sovereignty_tests {
    use super::stun_leaks_with_self_hosted_turn;

    #[test]
    fn google_stun_with_self_hosted_turn_leaks() {
        assert!(stun_leaks_with_self_hosted_turn(
            "stun:stun.l.google.com:19302",
            Some("turn:chat.example.com:3478"),
        ));
    }

    #[test]
    fn self_hosted_stun_does_not_leak() {
        assert!(!stun_leaks_with_self_hosted_turn(
            "stun:chat.example.com:3478",
            Some("turn:chat.example.com:3478"),
        ));
    }

    #[test]
    fn google_stun_without_turn_is_not_flagged() {
        // No self-hosted TURN configured — using public STUN is a deliberate
        // choice, not a leak to warn about.
        assert!(!stun_leaks_with_self_hosted_turn(
            "stun:stun.l.google.com:19302",
            None,
        ));
    }
}

#[cfg(test)]
mod cors_origin_tests {
    use super::{Config, TAURI_ORIGINS};

    fn config_with(origins: &[&str]) -> Config {
        let mut config = Config::default_for_test();
        config.cors_allowed_origins = origins.iter().map(|o| (*o).to_string()).collect();
        config
    }

    #[test]
    fn effective_origins_add_tauri_to_explicit_list() {
        let origins = config_with(&["https://a"]).effective_cors_origins();
        assert!(origins.contains(&"https://a".to_string()));
        for o in TAURI_ORIGINS {
            assert!(origins.contains(&o.to_string()), "missing {o}");
        }
        assert_eq!(origins.len(), 1 + TAURI_ORIGINS.len());
    }

    #[test]
    fn effective_origins_deduplicate_tauri_entries() {
        let origins = config_with(&["tauri://localhost", "https://a"]).effective_cors_origins();
        assert_eq!(origins.len(), 1 + TAURI_ORIGINS.len());
    }

    #[test]
    fn effective_origins_keep_wildcard() {
        assert_eq!(config_with(&["*"]).effective_cors_origins(), vec!["*"]);
    }
}

#[cfg(test)]
mod valkey_command_timeout_tests {
    use std::time::Duration;

    use super::{parse_valkey_command_timeout, Config};

    #[test]
    fn defaults_to_three_seconds() {
        assert_eq!(
            parse_valkey_command_timeout(None).unwrap(),
            Duration::from_secs(3)
        );
        assert_eq!(
            Config::default_for_test().valkey_command_timeout,
            Duration::from_secs(3)
        );
    }

    #[test]
    fn accepts_milliseconds() {
        assert_eq!(
            parse_valkey_command_timeout(Some(" 1500 ")).unwrap(),
            Duration::from_millis(1500)
        );
    }

    #[test]
    fn rejects_values_not_below_the_websocket_ready_timeout() {
        assert_eq!(
            parse_valkey_command_timeout(Some("4999")).unwrap(),
            Duration::from_millis(4999)
        );
        for raw in ["5000", "30000"] {
            let err = parse_valkey_command_timeout(Some(raw))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("VALKEY_COMMAND_TIMEOUT_MS") && err.contains("5000"),
                "{raw}: {err}"
            );
        }
    }

    #[test]
    fn rejects_invalid_values_at_startup() {
        for raw in ["0", "-1", "3s", "abc", "1.5"] {
            let err = parse_valkey_command_timeout(Some(raw))
                .unwrap_err()
                .to_string();
            assert!(err.contains("VALKEY_COMMAND_TIMEOUT_MS"), "{raw}: {err}");
        }
    }
}
