//! VoiceChat Server - Main Entry Point
//!
//! Self-hosted voice and text chat platform backend.

use anyhow::Result;
use std::net::SocketAddr;
use tracing::info;

mod api;
mod auth;
mod chat;
mod config;
mod db;
mod voice;
mod ws;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vc_server=debug,tower_http=debug".into()),
        )
        .json()
        .init();

    // Load configuration
    dotenvy::dotenv().ok();
    let config = config::Config::from_env()?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting VoiceChat Server"
    );

    // Initialize database
    let db_pool = db::create_pool(&config.database_url).await?;
    db::run_migrations(&db_pool).await?;

    // Initialize Redis
    let redis = db::create_redis_client(&config.redis_url).await?;

    // Initialize S3 client (optional - file uploads will be disabled if not configured)
    let s3 = match chat::S3Client::new(&config).await {
        Ok(client) => {
            // Verify bucket access
            match client.health_check().await {
                Ok(()) => {
                    info!(bucket = %config.s3_bucket, "S3 storage connected");
                    Some(client)
                }
                Err(e) => {
                    tracing::warn!("S3 health check failed: {}. File uploads disabled.", e);
                    None
                }
            }
        }
        Err(e) => {
            tracing::warn!("S3 client initialization failed: {}. File uploads disabled.", e);
            None
        }
    };

    // Initialize SFU server for voice
    let sfu = voice::SfuServer::new(std::sync::Arc::new(config.clone()))?;
    info!("Voice SFU server initialized");

    // Build application state
    let state = api::AppState::new(db_pool, redis, config.clone(), s3, sfu);

    // Build router
    let app = api::create_router(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    info!(address = %config.bind_address, "Server listening");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
