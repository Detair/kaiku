//! API Router and Application State
//!
//! Central routing configuration and shared state.

use axum::{routing::get, Router};
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::{auth, chat, chat::S3Client, config::Config, voice, voice::SfuServer, ws};

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
}

impl AppState {
    /// Create new application state.
    pub fn new(
        db: PgPool,
        redis: fred::clients::RedisClient,
        config: Config,
        s3: Option<S3Client>,
        sfu: SfuServer,
    ) -> Self {
        Self {
            db,
            redis,
            config: Arc::new(config),
            s3,
            sfu: Arc::new(sfu),
        }
    }

    /// Check if S3 storage is configured and available.
    pub fn has_s3(&self) -> bool {
        self.s3.is_some()
    }
}

/// Create the main application router.
pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Protected routes that require authentication
    let protected_routes = Router::new()
        .nest("/api/channels", chat::channels_router())
        .nest("/api/messages", chat::messages_router())
        .nest("/api/voice", voice::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Auth routes (pass state for middleware)
        .nest("/auth", auth::router(state.clone()))
        // Protected chat and voice routes
        .merge(protected_routes)
        // WebSocket
        .route("/ws", get(ws::handler))
        // API documentation
        .merge(api_docs())
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
        // State
        .with_state(state)
}

/// Health check endpoint.
async fn health_check() -> &'static str {
    "OK"
}

/// API documentation routes.
fn api_docs() -> Router<AppState> {
    // TODO: Setup utoipa swagger-ui
    Router::new()
}
