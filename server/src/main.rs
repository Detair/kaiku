//! `VoiceChat` Server - Main Entry Point
//!
//! Self-hosted voice and text chat platform backend.

use anyhow::Result;
use std::net::SocketAddr;
use tracing::info;

use vc_server::{api, chat, config, db, voice};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize rustls crypto provider (required for WebRTC)
    // This must happen before any TLS/WebRTC operations
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());

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
            tracing::warn!(
                "S3 client initialization failed: {}. File uploads disabled.",
                e
            );
            None
        }
    };

    // Initialize rate limiter (optional)
    // Needs to be initialized before SFU to be passed to it
    let rate_limiter = {
        use vc_server::ratelimit::{RateLimitConfig, RateLimiter};

        let rl_config = RateLimitConfig::from_env();
        if rl_config.enabled {
            // Clone redis for rate limiter
            let mut limiter = RateLimiter::new(redis.clone(), rl_config);
            match limiter.init().await {
                Ok(()) => {
                    info!("Rate limiter initialized");
                    Some(limiter)
                }
                Err(e) => {
                    tracing::warn!(
                        "Rate limiter initialization failed: {}. Rate limiting disabled.",
                        e
                    );
                    None
                }
            }
        } else {
            info!("Rate limiting disabled by configuration");
            None
        }
    };

    // Initialize SFU server for voice
    // Pass config and rate limiter
    let sfu = voice::SfuServer::new(std::sync::Arc::new(config.clone()), rate_limiter.clone())?;
    info!("Voice SFU server initialized");



    // Build application state
    let state = api::AppState::new(db_pool, redis, config.clone(), s3, sfu, rate_limiter);

    // Build router
    let app = api::create_router(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    info!(address = %config.bind_address, "Server listening");

    // Graceful shutdown handler
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        info!("Received shutdown signal, cleaning up...");
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await?;

    info!("Server shutdown complete");

    Ok(())
}
