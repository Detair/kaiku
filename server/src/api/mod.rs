//! API Router and Application State
//!
//! Central routing configuration and shared state.

pub mod favorites;
pub mod pins;
pub mod preferences;
pub mod reactions;
pub mod unread;
mod settings;
mod setup;

use axum::{
    extract::DefaultBodyLimit, extract::State, middleware::from_fn, middleware::from_fn_with_state,
    routing::{delete, get, post, put}, Json, Router,
};
use fred::interfaces::ClientLike;
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};

use crate::{
    admin, auth, chat, connectivity, crypto,
    auth::oidc::OidcProviderManager,
    chat::S3Client,
    config::Config,
    email::EmailService,
    guild, moderation, pages,
    ratelimit::{rate_limit_by_user, with_category, RateLimitCategory, RateLimiter},
    social, voice,
    voice::SfuServer,
    ws,
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
    /// Email service (optional, requires SMTP configuration)
    pub email: Option<Arc<EmailService>>,
    /// OIDC provider manager (optional, requires MFA encryption key)
    pub oidc_manager: Option<Arc<OidcProviderManager>>,
}

impl AppState {
    /// Create new application state.
    #[must_use]
    pub fn new(
        db: PgPool,
        redis: fred::clients::Client,
        config: Config,
        s3: Option<S3Client>,
        sfu: SfuServer,
        rate_limiter: Option<RateLimiter>,
        email: Option<EmailService>,
        oidc_manager: Option<OidcProviderManager>,
    ) -> Self {
        Self {
            db,
            redis,
            config: Arc::new(config),
            s3,
            sfu: Arc::new(sfu),
            rate_limiter,
            email: email.map(Arc::new),
            oidc_manager: oidc_manager.map(Arc::new),
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
    // In production, set CORS_ALLOWED_ORIGINS to specific origins
    let cors = if state.config.cors_allowed_origins.iter().any(|o| o == "*") {
        // Development mode: allow any origin
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        // Production mode: restrict to configured origins
        use axum::http::{header, HeaderName, Method};
        let origins: Vec<_> = state
            .config
            .cors_allowed_origins
            .iter()
            .filter_map(|o| {
                if let Ok(origin) = o.parse() { Some(origin) } else {
                    tracing::warn!(origin = %o, "Invalid CORS origin in configuration, skipping");
                    None
                }
            })
            .collect();

        if origins.is_empty() {
            tracing::error!("No valid CORS origins configured! All cross-origin requests will fail.");
        }

        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                HeaderName::from_static("x-request-id"),
            ])
            .allow_credentials(true)
    };

    // Get max upload size from config (default 50MB)
    let max_upload_size = state.config.max_upload_size;

    // Social routes with Social rate limit category (20 req/60s)
    let social_routes = social::router()
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Social)));

    // Other API routes with Write rate limit category (30 req/60s)
    let api_routes = Router::new()
        .nest("/api/channels", chat::channels_router())
        .nest("/api/messages", chat::messages_router())
        .nest("/api/guilds", guild::router())
        .nest("/api/invites", guild::invite_router())
        .nest("/api/pages", pages::platform_pages_router())
        .nest("/api/dm", chat::dm_router())
        .nest("/api/dm", voice::call_handlers::call_router())
        .nest("/api/voice", voice::router())
        .nest("/api/me/connection", connectivity::router())
        .nest("/api/me/preferences", preferences::router())
        .route("/api/me/pins", get(pins::list_pins).post(pins::create_pin))
        .route("/api/me/pins/reorder", put(pins::reorder_pins))
        .route("/api/me/pins/{id}", put(pins::update_pin).delete(pins::delete_pin))
        .route("/api/me/favorites", get(favorites::list_favorites))
        .route("/api/me/favorites/reorder", put(favorites::reorder_channels))
        .route("/api/me/favorites/reorder-guilds", put(favorites::reorder_guilds))
        .route("/api/me/favorites/{channel_id}", post(favorites::add_favorite).delete(favorites::remove_favorite))
        .route("/api/me/unread", get(unread::get_unread_aggregate))
        .nest("/api/keys", crypto::router())
        .nest("/api/users/{user_id}/keys", crypto::user_keys_router())
        // Message reactions
        .route(
            "/api/channels/{channel_id}/messages/{message_id}/reactions",
            get(reactions::get_reactions).put(reactions::add_reaction),
        )
        .route(
            "/api/channels/{channel_id}/messages/{message_id}/reactions/{emoji}",
            delete(reactions::remove_reaction),
        )
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Write)));

    // Admin routes (requires auth + system admin)
    // Auth middleware first, then admin router applies require_system_admin internally
    let admin_routes = admin::router(state.clone());

    // Protected routes that require authentication
    let protected_routes = Router::new()
        .merge(api_routes)
        .nest("/api", social_routes)
        .route("/api/reports", post(moderation::handlers::create_report))
        .nest("/api/admin", admin_routes)
        .layer(from_fn_with_state(state.clone(), auth::require_auth));

    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Public server settings
        .route("/api/settings", get(settings::get_server_settings))
        .route("/api/config/upload-limits", get(settings::get_upload_limits))
        // Setup routes (status and config are public, complete requires auth)
        .route("/api/setup/status", get(setup::status))
        .route("/api/setup/config", get(setup::get_config))
        .route("/api/setup/complete", post(setup::complete)
            .route_layer(from_fn_with_state(state.clone(), auth::require_auth)))
        // Auth routes (pass state for middleware)
        .nest("/auth", auth::router(state.clone()))
        // Protected chat and voice routes
        .merge(protected_routes)
        // Public message routes (download handles its own auth via query param)
        .nest("/api/messages", chat::messages_public_router())
        // WebSocket
        .route("/ws", get(ws::handler))
        // API documentation
        .merge(api_docs())
        // Middleware
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
#[derive(Serialize)]
struct HealthResponse {
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
async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    // Check database connectivity
    let db_ok = sqlx::query("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    // Check Redis connectivity
    let redis_ok = state
        .redis
        .ping::<String>(None)
        .await
        .is_ok();

    // Determine overall status
    let status = if db_ok && redis_ok { "ok" } else { "degraded" };

    Json(HealthResponse {
        status,
        database: db_ok,
        redis: redis_ok,
        rate_limiting: state.rate_limiter.is_some(),
    })
}

/// API documentation routes.
fn api_docs() -> Router<AppState> {
    // TODO: Setup utoipa swagger-ui
    Router::new()
}
