//! Database Layer
//!
//! `PostgreSQL` and Redis connections.
//!
//! ## Advisory Lock Seed Registry
//!
//! `PostgreSQL` advisory locks use integer seeds to prevent collisions.
//! Seeds are allocated per lock site:
//!
//! - 41 = `workspace_create` (per-user workspace creation limit)
//!   - Called from: `server/src/workspaces/handlers.rs`
//! - 43 = `workspace_entry` (per-workspace entry creation limit)
//!   - Called from: `server/src/workspaces/handlers.rs`
//! - 51 = `guild_create` (per-user guild creation limit)
//!   - Called from: `server/src/guild/handlers.rs`
//! - 53 = `guild_member_join` (per-guild join serialization)
//!   - Called from: `server/src/guild/invites.rs` (invite join path)
//!   - Called from: `server/src/discovery/handlers.rs` (discovery join path)
//! - 55 = `channel_create` (per-guild channel creation limit)
//!   - Called from: `server/src/chat/channels.rs`
//! - 57 = `role_create` (per-guild role creation limit)
//!   - Called from: `server/src/guild/roles.rs`
//! - 59 = `emoji_create` (per-guild emoji creation limit, COUNT + INSERT only)
//!   - Called from: `server/src/guild/emojis.rs`
//! - 61 = `page_create` (per-guild page creation limit)
//!   - Called from: `server/src/pages/handlers.rs`
//! - 63 = `bot_install` (per-guild bot installation limit)
//!   - Called from: `server/src/guild/handlers.rs`
//! - 65 = `push_register_user` (per-user push registration serialization)
//!   - Called from: `server/src/push/mod.rs`
//! - 67 = `push_register_endpoint` (per-(provider, endpoint) registration serialization, always
//!   taken after the user lock)
//!   - Called from: `server/src/push/mod.rs`

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
        .idle_timeout(Duration::from_mins(10))
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

/// Enable admin RLS bypass on a transaction.
///
/// Sets the `app.admin_bypass` session variable to `'true'` so that RLS
/// policies on `connection_metrics` and `connection_sessions` allow the
/// query to read all rows regardless of the row-level security user filter.
///
/// The setting is `LOCAL` (transaction-scoped) and automatically reverts
/// when the transaction commits or rolls back.
pub async fn set_admin_bypass(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.admin_bypass', 'true', true)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Default per-command Valkey timeout (`VALKEY_COMMAND_TIMEOUT_MS`).
pub const DEFAULT_VALKEY_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Create and connect a Valkey client with [`DEFAULT_VALKEY_COMMAND_TIMEOUT`].
pub async fn create_redis_client(redis_url: &str) -> Result<fred::clients::Client> {
    create_redis_client_with_timeout(redis_url, DEFAULT_VALKEY_COMMAND_TIMEOUT).await
}

/// Create and connect a Valkey client whose commands fail after
/// `command_timeout` (see [`build_redis_client`]).
pub async fn create_redis_client_with_timeout(
    redis_url: &str,
    command_timeout: std::time::Duration,
) -> Result<fred::clients::Client> {
    use fred::prelude::*;

    let client = build_redis_client(redis_url, command_timeout)?;
    client.connect();
    client.wait_for_connect().await?;

    info!("Connected to Redis");
    Ok(client)
}

/// Build (but do not connect) a Valkey client.
///
/// Every command, the TCP connect and fred's internal setup commands
/// (`PING`, `AUTH`, `SELECT`, ...) fail after `command_timeout`, so a hung or
/// unreachable Valkey makes callers fail quickly instead of waiting for it.
/// Clients derived from this one (`clone_new`, the per-socket subscriber)
/// inherit these settings.
///
/// Reconnects never give up, with capped exponential backoff (100 ms doubling
/// to at most 5 s): the first retry stays fast, an outage does not spin.
pub fn build_redis_client(
    redis_url: &str,
    command_timeout: std::time::Duration,
) -> Result<fred::clients::Client> {
    use fred::prelude::*;

    let config = Config::from_url(redis_url)?;
    let perf = PerformanceConfig {
        default_command_timeout: command_timeout,
        ..PerformanceConfig::default()
    };
    let connection = ConnectionConfig {
        connection_timeout: command_timeout,
        internal_command_timeout: command_timeout,
        ..ConnectionConfig::default()
    };
    let policy = ReconnectPolicy::new_exponential(0, 100, 5_000, 2);

    Ok(Client::new(
        config,
        Some(perf),
        Some(connection),
        Some(policy),
    ))
}

/// A Valkey command failed (timeout, lost connection or error reply).
///
/// Security checks that depend on Valkey turn this into "service unavailable"
/// (fail closed); see [`crate::auth::AuthError::ServiceUnavailable`].
#[derive(Debug, thiserror::Error)]
#[error("Valkey unavailable: {0}")]
pub struct ValkeyUnavailable(#[from] pub fred::error::Error);

/// Lets a log line through at most once per `interval`, so a Valkey outage
/// does not log once per request. Use one `static` per log site.
pub struct LogThrottle {
    /// Milliseconds since [`LogThrottle::epoch`] of the last allowed line, +1
    /// (0 = never).
    last_ms: std::sync::atomic::AtomicU64,
}

impl LogThrottle {
    /// A throttle that allows its first line.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_ms: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn epoch() -> std::time::Instant {
        static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        *EPOCH.get_or_init(std::time::Instant::now)
    }

    /// `true` if a line may be logged now (and records that it was).
    pub fn allow(&self, interval: std::time::Duration) -> bool {
        use std::sync::atomic::Ordering;

        let now = u64::try_from(Self::epoch().elapsed().as_millis())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1);
        let interval = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
        let last = self.last_ms.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < interval {
            return false;
        }
        self.last_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }
}

impl Default for LogThrottle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod valkey_client_tests {
    use std::time::Duration;

    use fred::prelude::*;

    use super::build_redis_client;

    #[test]
    fn client_fails_commands_and_connects_within_the_command_timeout() {
        let timeout = Duration::from_millis(1234);
        let client = build_redis_client("redis://127.0.0.1:6379", timeout).unwrap();

        assert_eq!(client.perf_config().default_command_timeout, timeout);
        let connection = client.connection_config();
        assert_eq!(connection.connection_timeout, timeout);
        assert_eq!(connection.internal_command_timeout, timeout);
    }

    #[test]
    fn client_reconnects_forever_with_capped_exponential_backoff() {
        let client = build_redis_client("redis://127.0.0.1:6379", Duration::from_secs(3)).unwrap();
        match client.client_reconnect_policy() {
            Some(ReconnectPolicy::Exponential {
                max_attempts,
                min_delay,
                max_delay,
                ..
            }) => {
                assert_eq!(max_attempts, 0, "never give up reconnecting");
                assert_eq!(min_delay, 100, "first retry stays fast");
                assert_eq!(max_delay, 5_000, "backoff is capped");
            }
            other => panic!("expected capped exponential backoff, got {other:?}"),
        }
    }

    #[test]
    fn log_throttle_allows_one_line_per_interval() {
        let throttle = super::LogThrottle::new();
        assert!(throttle.allow(Duration::from_mins(1)));
        assert!(!throttle.allow(Duration::from_mins(1)));
        assert!(throttle.allow(Duration::ZERO));
    }
}
