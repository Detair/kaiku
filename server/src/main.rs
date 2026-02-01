//! `VoiceChat` Server - Main Entry Point
//!
//! Self-hosted voice and text chat platform backend.

use anyhow::Result;
use std::net::SocketAddr;
use tracing::info;

use vc_server::{api, chat, config, db, email, voice};

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

    // Start background cleanup task for voice stats rate limiter to prevent memory leaks
    let voice_cleanup_handle = sfu.start_cleanup_task();

    // Start background cleanup task for database (sessions, prekeys, device transfers)
    let db_pool_clone = db_pool.clone();
    let db_cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600)); // Every hour
        loop {
            interval.tick().await;

            // Cleanup expired sessions
            match db::cleanup_expired_sessions(&db_pool_clone).await {
                Ok(count) if count > 0 => {
                    tracing::debug!(count, "Cleaned up expired sessions");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to cleanup expired sessions");
                }
                _ => {}
            }

            // Cleanup claimed prekeys older than 7 days
            match db::cleanup_claimed_prekeys(&db_pool_clone).await {
                Ok(count) if count > 0 => {
                    tracing::debug!(count, "Cleaned up claimed prekeys");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to cleanup claimed prekeys");
                }
                _ => {}
            }

            // Cleanup expired device transfers
            match db::cleanup_expired_device_transfers(&db_pool_clone).await {
                Ok(count) if count > 0 => {
                    tracing::debug!(count, "Cleaned up expired device transfers");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to cleanup expired device transfers");
                }
                _ => {}
            }

            // Cleanup expired password reset tokens
            match db::cleanup_expired_reset_tokens(&db_pool_clone).await {
                Ok(count) if count > 0 => {
                    tracing::debug!(count, "Cleaned up expired reset tokens");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to cleanup expired reset tokens");
                }
                _ => {}
            }
        }
    });

    info!("Voice SFU server initialized");

    // Initialize email service (optional - password reset will be disabled if not configured)
    let email_service = if config.has_smtp() {
        match email::EmailService::new(&config) {
            Ok(service) => {
                info!("Email service initialized (SMTP)");
                Some(service)
            }
            Err(e) => {
                tracing::warn!("Email service initialization failed: {}. Password reset disabled.", e);
                None
            }
        }
    } else {
        info!("SMTP not configured. Password reset disabled.");
        None
    };

    // Initialize OIDC provider manager (requires MFA encryption key)
    let oidc_manager = if let Some(ref key_hex) = config.mfa_encryption_key {
        match hex::decode(key_hex) {
            Ok(key) if key.len() == 32 => {
                let manager = vc_server::auth::oidc::OidcProviderManager::new(key);

                // Seed legacy OIDC config from environment variables
                if let Err(e) = manager.seed_from_env(&config, &db_pool).await {
                    tracing::warn!(error = %e, "Failed to seed OIDC from env vars");
                }

                // Load providers from database
                if let Err(e) = manager.load_providers(&db_pool).await {
                    tracing::warn!(error = %e, "Failed to load OIDC providers");
                }

                info!("OIDC provider manager initialized");
                Some(manager)
            }
            Ok(key) => {
                tracing::warn!(
                    len = key.len(),
                    "MFA_ENCRYPTION_KEY has wrong length (expected 32 bytes). OIDC disabled."
                );
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "Invalid MFA_ENCRYPTION_KEY hex. OIDC disabled.");
                None
            }
        }
    } else {
        info!("MFA_ENCRYPTION_KEY not set. OIDC provider management disabled.");
        None
    };

    // Build application state
    let state = api::AppState::new(db_pool.clone(), redis.clone(), config.clone(), s3, sfu, rate_limiter, email_service, oidc_manager);

    // Build router
    let app = api::create_router(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(&config.bind_address).await?;
    info!(address = %config.bind_address, "Server listening");

    // Graceful shutdown handler with proper cleanup
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        info!("Received shutdown signal, initiating graceful shutdown...");
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await?;

    // =========================================================================
    // Graceful shutdown: clean up background tasks
    // =========================================================================

    info!("HTTP server stopped, cleaning up background tasks...");

    // 1. Abort background cleanup tasks
    voice_cleanup_handle.abort();
    db_cleanup_handle.abort();
    // Wait for them to finish (will return Err(JoinError) due to abort, which is expected)
    let _ = voice_cleanup_handle.await;
    let _ = db_cleanup_handle.await;
    info!("Background cleanup tasks stopped");

    // 2. Close database pool gracefully
    // This waits for active queries to complete (up to acquire_timeout)
    db_pool.close().await;
    info!("Database pool closed");

    // 3. Close Redis connection
    // fred doesn't have an explicit close, but dropping the client handles cleanup
    drop(redis);
    info!("Redis connection closed");

    info!("Server shutdown complete");

    Ok(())
}
