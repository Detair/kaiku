//! API Router and Application State
//!
//! Central routing configuration and shared state. Feature-specific handlers
//! live in their own top-level modules; this file wires them together.

use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, FromRef, State};
use axum::http::{Request, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::Response;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use fred::interfaces::ClientLike;
use serde::Serialize;
use sqlx::PgPool;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::oidc::OidcProviderManager;
use crate::chat::S3Client;
use crate::config::Config;
use crate::email::EmailService;
use crate::moderation::filter_cache::FilterCache;
use crate::ratelimit::{
    rate_limit_by_ip, rate_limit_by_user, with_category, RateLimitCategory, RateLimiter,
};
use crate::voice::{ScreenShareLimiter, SfuServer};
use crate::{
    admin, auth, bots, chat, connectivity, crypto, discovery, favorites, governance, guild,
    incoming_webhooks, moderation, pages, preferences, search, settings, setup, social, voice,
    webhooks, workspaces, ws,
};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool
    pub db: PgPool,
    /// Redis client
    pub redis: fred::clients::Client,
    /// Server configuration
    pub config: Arc<Config>,
    /// S3 client for file storage (optional)
    pub s3: Option<S3Client>,
    /// SFU server for voice channels
    pub sfu: Arc<SfuServer>,
    /// Rate limiter (optional, uses Redis)
    pub rate_limiter: Option<RateLimiter>,
    /// Screen share limit manager (uses Redis Lua script)
    pub screen_share_limiter: Option<ScreenShareLimiter>,
    /// Email service (optional, requires SMTP configuration)
    pub email: Option<Arc<EmailService>>,
    /// OIDC provider manager (optional, requires MFA encryption key)
    pub oidc_manager: Option<Arc<OidcProviderManager>>,
    /// Per-guild content filter engine cache
    pub filter_cache: Arc<FilterCache>,
    /// Shared HTTP client for outbound requests (geo-IP lookups, etc.)
    pub http_client: reqwest::Client,
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

/// Configuration for creating a new [`AppState`].
pub struct AppStateConfig {
    pub db: PgPool,
    pub redis: fred::clients::Client,
    pub config: Config,
    pub s3: Option<S3Client>,
    pub sfu: SfuServer,
    pub rate_limiter: Option<RateLimiter>,
    pub screen_share_limiter: Option<ScreenShareLimiter>,
    pub email: Option<EmailService>,
    pub oidc_manager: Option<OidcProviderManager>,
    pub http_client: reqwest::Client,
}

impl AppState {
    /// Create new application state.
    #[must_use]
    pub fn new(cfg: AppStateConfig) -> Self {
        Self {
            db: cfg.db,
            redis: cfg.redis,
            config: Arc::new(cfg.config),
            s3: cfg.s3,
            sfu: Arc::new(cfg.sfu),
            rate_limiter: cfg.rate_limiter,
            screen_share_limiter: cfg.screen_share_limiter,
            email: cfg.email.map(Arc::new),
            oidc_manager: cfg.oidc_manager.map(Arc::new),
            filter_cache: Arc::new(FilterCache::new()),
            http_client: cfg.http_client,
        }
    }

    /// Check if S3 storage is configured and available.
    #[must_use]
    pub const fn has_s3(&self) -> bool {
        self.s3.is_some()
    }
}

/// Create the main application router.
pub fn create_router(state: AppState) -> Router {
    // Configure CORS based on allowed origins
    let cors = {
        use axum::http::{header, HeaderName, Method};

        let allowed_methods = [
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ];
        let allowed_headers = [
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-request-id"),
        ];

        if state.config.cors_allowed_origins.iter().any(|o| o == "*") {
            // Wildcard `*` mirrors the request Origin, but credentials MUST be
            // disabled in that mode. Mirroring the Origin *with* credentials
            // would let any website make credentialed cross-origin calls (e.g.
            // POST /auth/refresh with the victim's HttpOnly cookie) and read the
            // response — a full session-hijack primitive. Same-origin requests
            // (the actual app) are unaffected: CORS never gates same-origin, so
            // cookies still flow there. Cross-origin callers simply get no
            // credentials. Configure CORS_ALLOWED_ORIGINS explicitly to allow
            // trusted cross-origin credentialed access.
            tracing::warn!(
                "CORS_ALLOWED_ORIGINS='*' mirrors the Origin header with credentials DISABLED; \
                 set explicit origins to enable credentialed cross-origin requests"
            );
            CorsLayer::new()
                .allow_origin(AllowOrigin::mirror_request())
                .allow_methods(allowed_methods)
                .allow_headers(allowed_headers)
                .allow_credentials(false)
        } else {
            // Production mode: restrict to configured origins
            let origins: Vec<_> = state
                .config
                .cors_allowed_origins
                .iter()
                .filter_map(|o| {
                    if let Ok(origin) = o.parse() {
                        Some(origin)
                    } else {
                        tracing::warn!(origin = %o, "Invalid CORS origin in configuration, skipping");
                        None
                    }
                })
                .collect();

            if origins.is_empty() {
                tracing::error!(
                    "No valid CORS origins configured! All cross-origin requests will fail."
                );
            }

            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods(allowed_methods)
                .allow_headers(allowed_headers)
                .allow_credentials(true)
        }
    };

    // Get max upload size from config (default 50MB)
    let max_upload_size = state.config.max_upload_size;

    // Social routes with Social rate limit category (20 req/60s)
    let social_routes = social::router()
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Social)));

    // Discovery join route with Social rate limit (20 req/60s) — frictionless join needs tighter
    // limit
    let discovery_join_routes = Router::new()
        .nest("/api/discover", discovery::protected_router())
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Social)));

    // Other API routes with Write rate limit category (30 req/60s)
    let api_routes = Router::new()
        .nest("/api/channels", chat::channels_router())
        .nest("/api/messages", chat::messages_router())
        .nest("/api/forum", chat::forum_router())
        .nest("/api/guilds", guild::router())
        .nest(
            "/api/guilds/{id}/filters",
            moderation::filter_handlers::router(),
        )
        .nest("/api/invites", guild::invite_router())
        .nest("/api/pages", pages::platform_pages_router())
        .nest("/api/dm", chat::dm_router())
        .nest("/api/dm", voice::call_handlers::call_router())
        .nest("/api/voice", voice::router())
        .route(
            "/api/me/data-export",
            get(governance::handlers::get_export_status),
        )
        .route(
            "/api/me/data-export/download",
            get(governance::handlers::download_export),
        )
        .route(
            "/api/me/delete-account/cancel",
            post(governance::handlers::cancel_deletion),
        )
        .nest("/api/me/connection", connectivity::router())
        .nest("/api/me/preferences", preferences::router())
        .route(
            "/api/me/pins",
            get(chat::pins::list_pins).post(chat::pins::create_pin),
        )
        .route("/api/me/pins/reorder", put(chat::pins::reorder_pins))
        .route(
            "/api/me/pins/{id}",
            put(chat::pins::update_pin).delete(chat::pins::delete_pin),
        )
        .nest("/api/me/favorites", favorites::router())
        .nest("/api/me/workspaces", workspaces::router())
        .nest("/api/me/push-subscriptions", crate::push::router())
        .route("/api/me/unread", get(chat::unread::get_unread_aggregate))
        .route("/api/me/read-all", post(chat::unread::mark_all_read))
        .nest("/api/keys", crypto::router())
        .nest("/api/users/{user_id}/keys", crypto::user_keys_router())
        // Bot management routes (bots + slash commands + gateway intents)
        .nest("/api/applications", bots::router())
        // Incoming (Discord-compatible) webhook management
        .merge(incoming_webhooks::management_router())
        // Webhooks
        .route(
            "/api/applications/{app_id}/webhooks",
            get(webhooks::handlers::list_webhooks).post(webhooks::handlers::create_webhook),
        )
        .route(
            "/api/applications/{app_id}/webhooks/{wh_id}",
            get(webhooks::handlers::get_webhook)
                .patch(webhooks::handlers::update_webhook)
                .delete(webhooks::handlers::delete_webhook),
        )
        .route(
            "/api/applications/{app_id}/webhooks/{wh_id}/test",
            post(webhooks::handlers::test_webhook),
        )
        .route(
            "/api/applications/{app_id}/webhooks/{wh_id}/deliveries",
            get(webhooks::handlers::list_deliveries),
        )
        // Message reactions
        .route(
            "/api/channels/{channel_id}/messages/{message_id}/reactions",
            get(chat::reactions::get_reactions).put(chat::reactions::add_reaction),
        )
        .route(
            "/api/channels/{channel_id}/messages/{message_id}/reactions/{emoji}",
            delete(chat::reactions::remove_reaction),
        )
        // Channel pins
        .route(
            "/api/channels/{channel_id}/pins",
            get(chat::channel_pins::list_channel_pins),
        )
        .route(
            "/api/channels/{channel_id}/messages/{message_id}/pin",
            put(chat::channel_pins::pin_message).delete(chat::channel_pins::unpin_message),
        )
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Write)));

    // Search routes with dedicated Search rate limit category (15 req/60s)
    let search_routes = Router::new()
        .route(
            "/api/guilds/{id}/search",
            get(guild::search::search_messages),
        )
        .route("/api/dm/search", get(chat::dm_search::search_dm_messages))
        .nest("/api/search", search::router())
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Search)));

    // Data governance routes with DataGovernance rate limit (2 req/60s for mutations)
    let governance_routes = Router::new()
        .route(
            "/api/me/data-export",
            post(governance::handlers::request_export),
        )
        .route(
            "/api/me/delete-account",
            post(governance::handlers::request_deletion),
        )
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::DataGovernance)));

    // Admin routes (requires auth + system admin)
    // Auth middleware first, then admin router applies require_system_admin internally
    let admin_routes = admin::router(state.clone());

    // Protected routes that require authentication
    let protected_routes = Router::new()
        .merge(api_routes)
        .merge(governance_routes)
        .merge(discovery_join_routes)
        .merge(search_routes)
        .nest("/api", social_routes)
        .route("/api/reports", post(moderation::handlers::create_report))
        .nest("/api/admin", admin_routes)
        .layer(from_fn_with_state(state.clone(), auth::require_auth));

    let app_routes = Router::new()
        // Public guild discovery (browsing, no auth required, IP rate limited)
        .nest(
            "/api/discover",
            discovery::public_router()
                .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
                .layer(from_fn(with_category(RateLimitCategory::Search))),
        )
        // Public server settings
        .route("/api/settings", get(settings::get_server_settings))
        .route(
            "/api/config/upload-limits",
            get(settings::get_upload_limits),
        )
        .route("/api/config/limits", get(settings::get_instance_limits))
        // Setup routes (status and config are public, complete requires auth)
        .nest("/api/setup", setup::router(state.clone()))
        // Auth routes (pass state for middleware)
        .nest("/auth", auth::router(state.clone()))
        // Protected chat and voice routes
        .merge(protected_routes)
        // Incoming webhook execution (token in the URL is the credential;
        // Discord-compatible). IP rate limited + failed-auth blocked.
        .merge(
            incoming_webhooks::public_router()
                .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
                .layer(from_fn(with_category(RateLimitCategory::Write))),
        )
        // Public file redirect (presigned S3 URLs)
        .route("/api/files/{*key}", get(chat::files::serve))
        // Public message routes (download handles its own auth via query param)
        .nest("/api/messages", chat::messages_public_router())
        // WebSocket
        .route("/ws", get(ws::handler))
        // Bot Gateway WebSocket (uses bot token auth)
        .route(
            "/api/gateway/bot",
            get(ws::bot_gateway::bot_gateway_handler),
        )
        // API documentation
        .merge(api_docs(state.config.enable_api_docs))
        .layer(OtelInResponseLayer)
        .layer(OtelAxumLayer::default());

    Router::new()
        // Health check
        .route("/health", get(health_check))
        .merge(app_routes)
        // Middleware
        .layer(from_fn(security_headers))
        .layer(from_fn(http_metrics))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
        // Request ID for tracing correlation
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        // Increase body limit for file uploads (default is 2MB)
        .layer(DefaultBodyLimit::max(max_upload_size))
        // State
        .with_state(state)
}

/// Health check response.
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct HealthResponse {
    /// Overall service status ("ok" or "degraded")
    status: &'static str,
    /// Database connectivity status
    database: bool,
    /// Redis connectivity status
    redis: bool,
    /// Whether rate limiting is enabled
    rate_limiting: bool,
}

/// Health check endpoint.
///
/// Verifies connectivity to critical dependencies (database, Redis).
/// Returns "degraded" status if any dependency is unavailable.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Service healthy", body = HealthResponse),
        (status = 503, description = "Service degraded — database or Redis unreachable", body = HealthResponse),
    ),
)]
pub(crate) async fn health_check(
    State(state): State<AppState>,
) -> (StatusCode, Json<HealthResponse>) {
    // Check database connectivity
    let db_ok = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    // Check Redis connectivity
    let redis_ok = state.redis.ping::<String>(None).await.is_ok();

    // Determine overall status
    let (status, http_status) = if db_ok && redis_ok {
        ("ok", StatusCode::OK)
    } else {
        ("degraded", StatusCode::SERVICE_UNAVAILABLE)
    };

    (
        http_status,
        Json(HealthResponse {
            status,
            database: db_ok,
            redis: redis_ok,
            rate_limiting: state.rate_limiter.is_some(),
        }),
    )
}

/// Middleware that counts HTTP error responses (4xx/5xx).
async fn http_metrics(
    request: Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let start = std::time::Instant::now();
    let response = next.run(request).await;
    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
    let status = response.status().as_u16();
    crate::observability::metrics::record_http_request(status, duration_ms);
    response
}

/// Middleware that adds security headers to all responses.
async fn security_headers(request: Request<axum::body::Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::HeaderName::from_static("referrer-policy"),
        axum::http::HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    headers.insert(
        axum::http::HeaderName::from_static("strict-transport-security"),
        axum::http::HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
    );
    response
}

/// API documentation routes.
///
/// Serves Swagger UI at `/api/docs` when enabled via `ENABLE_API_DOCS` env var.
/// Defaults to enabled in debug builds, disabled in release builds.
fn api_docs(enable: bool) -> Router<AppState> {
    if !enable {
        return Router::new();
    }
    Router::new().merge(
        SwaggerUi::new("/api/docs")
            .url("/api/docs/openapi.json", crate::openapi::ApiDoc::openapi()),
    )
}
