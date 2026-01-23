//! API Router and Application State
//!
//! Central routing configuration and shared state.

pub mod preferences;
mod settings;

use axum::{
    extract::DefaultBodyLimit, extract::State, middleware::from_fn, middleware::from_fn_with_state,
    routing::get, Json, Router,
};
use serde::Serialize;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{
    admin, auth, chat, connectivity, crypto,
    chat::S3Client,
    config::Config,
    guild, pages,
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
    pub redis: fred::clients::RedisClient,
    /// Server configuration
    pub config: Arc<Config>,
    /// S3 client for file storage (optional)
    pub s3: Option<S3Client>,
    /// SFU server for voice channels
    pub sfu: Arc<SfuServer>,
    /// Rate limiter (optional, uses Redis)
    pub rate_limiter: Option<RateLimiter>,
}

impl AppState {
    /// Create new application state.
    #[must_use]
    pub fn new(
        db: PgPool,
        redis: fred::clients::RedisClient,
        config: Config,
        s3: Option<S3Client>,
        sfu: SfuServer,
        rate_limiter: Option<RateLimiter>,
    ) -> Self {
        Self {
            db,
            redis,
            config: Arc::new(config),
            s3,
            sfu: Arc::new(sfu),
            rate_limiter,
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
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

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
        .nest("/api/guilds/{guild_id}/pages", pages::guild_pages_router())
        .nest("/api/invites", guild::invite_router())
        .nest("/api/pages", pages::platform_pages_router())
        .nest("/api/dm", chat::dm_router())
        .nest("/api/dm", voice::call_handlers::call_router())
        .nest("/api/voice", voice::router())
        .nest("/api/me/connection", connectivity::router())
        .nest("/api/me/preferences", preferences::router())
        .nest("/api/keys", crypto::router())
        .nest("/api/users/{user_id}/keys", crypto::user_keys_router())
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::Write)));

    // Admin routes (requires auth + system admin)
    // Auth middleware first, then admin router applies require_system_admin internally
    let admin_routes = admin::router(state.clone());

    // Protected routes that require authentication
    let protected_routes = Router::new()
        .merge(api_routes)
        .nest("/api", social_routes)
        .nest("/api/admin", admin_routes)
        .layer(from_fn_with_state(state.clone(), auth::require_auth));

    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Public server settings
        .route("/api/settings", get(settings::get_server_settings))
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
        // Increase body limit for file uploads (default is 2MB)
        .layer(DefaultBodyLimit::max(max_upload_size))
        // State
        .with_state(state)
}

/// Health check response.
#[derive(Serialize)]
struct HealthResponse {
    /// Service status
    status: &'static str,
    /// Whether rate limiting is enabled
    rate_limiting: bool,
}

/// Health check endpoint.
async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        rate_limiting: state.rate_limiter.is_some(),
    })
}

/// API documentation routes.
fn api_docs() -> Router<AppState> {
    // TODO: Setup utoipa swagger-ui
    Router::new()
}
