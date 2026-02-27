//! Advisory Lock Seed Registry
//!
//! `PostgreSQL` advisory locks use integer seeds to prevent collisions.
//! Seeds are allocated per lock site:
//! - 41 = `workspace_create` (per-user workspace creation limit)
//! - 43 = `workspace_entry` (per-workspace entry creation limit)
//! - 51 = `guild_create` (per-user guild creation limit) — see fix/review-focus-guild-discovery branch
//! - 53 = `guild_join` (per-guild join serialization) — see fix/review-focus-guild-discovery branch
//! - 61 = `page_create` (per-guild page creation limit)
//!

//! Database Layer
//!
//! `PostgreSQL` and Redis connections.
//!
//! Advisory Lock Seed Registry
//! - 41 = `workspace_create`
//! - 43 = `workspace_entry`
//! - 51 = `guild_create`
//! - 53 = `guild_member_join` (used by both invite joins and discovery joins)
//!   - Called from: server/src/guild/invites.rs (invite join path)
//!   - Called from: server/src/discovery/handlers.rs (discovery join path)
//!

mod models;
mod queries;
pub mod user_features;

#[cfg(test)]
mod tests;

use std::time::Duration;

use anyhow::Result;
pub use models::*;
pub use queries::*;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;
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
pub async fn create_redis_client(redis_url: &str) -> Result<fred::clients::Client> {
    use fred::prelude::*;

    let config = Config::from_url(redis_url)?;
    let client = Client::new(config, None, None, None);
    client.connect();
    client.wait_for_connect().await?;

    info!("Connected to Redis");
    Ok(client)
}
