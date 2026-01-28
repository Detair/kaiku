//! Database Layer
//!
//! `PostgreSQL` and Redis connections.

mod models;
mod queries;
pub mod user_features;

#[cfg(test)]
mod tests;

use std::time::Duration;

use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tracing::info;

pub use models::*;
pub use queries::*;
pub use user_features::UserFeatures;

/// Create `PostgreSQL` connection pool with health configuration.
pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        // Keep minimum connections warm to prevent cold-start latency
        .min_connections(5)
        .max_connections(20)
        // Prevent hanging requests on pool exhaustion
        .acquire_timeout(Duration::from_secs(5))
        // Clean up idle connections to prevent stale connection issues
        .idle_timeout(Duration::from_secs(600))
        // Validate connections before use to catch stale/broken connections
        .test_before_acquire(true)
        .connect(database_url)
        .await?;

    info!("Connected to PostgreSQL");
    Ok(pool)
}

/// Run database migrations.
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    info!("Database migrations completed");
    Ok(())
}

/// Create Redis client.
pub async fn create_redis_client(redis_url: &str) -> Result<fred::clients::RedisClient> {
    use fred::prelude::*;

    let config = RedisConfig::from_url(redis_url)?;
    let client = RedisClient::new(config, None, None, None);
    client.connect();
    client.wait_for_connect().await?;

    info!("Connected to Redis");
    Ok(client)
}
