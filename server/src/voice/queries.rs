//! Database queries for the voice module.
//!
//! The voice subsystem is mostly an in-memory SFU. The queries here cover
//! the small set of relational lookups and metric writes that the SFU and
//! its HTTP/WebSocket handlers need from `PostgreSQL`/`TimescaleDB`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::error::VoiceError;
use super::stats::VoiceStats;

// ============================================================================
// User / Channel lookups (used by ws_handler join flow)
// ============================================================================

/// Fetch a user's `(username, display_name)` pair, used to populate voice
/// participant info on join.
pub async fn find_user_username_display_name(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<(String, String), VoiceError> {
    let row: (String, String) =
        sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
    Ok(row)
}

/// Fetch the configured `max_screen_shares` cap for a channel, returning
/// `None` if the channel does not exist.
pub async fn find_channel_max_screen_shares(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Option<i32>, VoiceError> {
    let value: Option<i32> =
        sqlx::query_scalar("SELECT max_screen_shares FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await?;
    Ok(value)
}

/// Fetch the `guild_id` for a channel, returning `None` for DM channels or
/// missing channels. Errors are swallowed because metrics writers are
/// fire-and-forget; callers receive `None` on failure.
pub async fn find_channel_guild_id(pool: &PgPool, channel_id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT guild_id FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

// ============================================================================
// DM call participant lookup (used by call_handlers)
// ============================================================================

/// List the user IDs that participate in a DM channel.
pub async fn list_dm_participant_ids(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT user_id FROM dm_participants WHERE channel_id = $1",
        channel_id
    )
    .fetch_all(pool)
    .await
}

// ============================================================================
// Connection metrics (TimescaleDB)
// ============================================================================

/// Insert a per-tick `connection_metrics` row. Returns the underlying sqlx
/// error so the fire-and-forget caller can log it without converting types.
pub async fn insert_connection_metric(
    pool: &PgPool,
    stats: &VoiceStats,
    user_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"
        INSERT INTO connection_metrics
        (time, user_id, session_id, channel_id, guild_id, latency_ms, packet_loss, jitter_ms, quality)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ",
    )
    .bind(Utc::now())
    .bind(user_id)
    .bind(stats.session_id)
    .bind(channel_id)
    .bind(guild_id)
    .bind(stats.latency)
    .bind(stats.packet_loss)
    .bind(stats.jitter)
    .bind(i16::from(stats.quality))
    .execute(pool)
    .await?;
    Ok(())
}

/// Check whether any `connection_metrics` rows exist for a given session.
pub async fn session_has_connection_metrics(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connection_metrics WHERE session_id = $1)")
        .bind(session_id)
        .fetch_one(pool)
        .await
}

/// Insert a `connection_sessions` row aggregating metrics from
/// `connection_metrics` for the given session.
pub async fn insert_session_with_aggregated_metrics(
    pool: &PgPool,
    session_id: Uuid,
    user_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
    started_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"
        INSERT INTO connection_sessions
        (id, user_id, channel_id, guild_id, started_at, ended_at,
         avg_latency, avg_loss, avg_jitter, worst_quality)
        SELECT
            $1, $2, $3, $4, $5, NOW(),
            AVG(latency_ms)::SMALLINT,
            AVG(packet_loss)::REAL,
            AVG(jitter_ms)::SMALLINT,
            MIN(quality)::SMALLINT
        FROM connection_metrics
        WHERE session_id = $1
        ",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(channel_id)
    .bind(guild_id)
    .bind(started_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert a `connection_sessions` row with NULL aggregates, used for very
/// short calls that produced no metric ticks.
pub async fn insert_session_without_metrics(
    pool: &PgPool,
    session_id: Uuid,
    user_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
    started_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r"
        INSERT INTO connection_sessions
        (id, user_id, channel_id, guild_id, started_at, ended_at,
         avg_latency, avg_loss, avg_jitter, worst_quality)
        VALUES ($1, $2, $3, $4, $5, NOW(), NULL, NULL, NULL, NULL)
        ",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(channel_id)
    .bind(guild_id)
    .bind(started_at)
    .execute(pool)
    .await?;
    Ok(())
}
