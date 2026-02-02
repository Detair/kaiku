//! Voice metrics storage.
//!
//! This module provides functions for storing voice connection metrics
//! in `TimescaleDB` for historical analysis.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::stats::VoiceStats;

/// Store connection metrics in `TimescaleDB` (fire-and-forget).
///
/// This function is designed to be spawned as a background task.
/// Errors are logged but not propagated to avoid impacting the
/// caller's flow.
pub async fn store_metrics(
    pool: PgPool,
    stats: VoiceStats,
    user_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
) {
    let result = sqlx::query(
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
    .execute(&pool)
    .await;

    if let Err(e) = result {
        tracing::warn!(
            user_id = %user_id,
            session_id = %stats.session_id,
            "Failed to store connection metrics: {}",
            e
        );
    }
}

/// Get `guild_id` from `channel_id`.
///
/// Returns `None` if the channel doesn't exist or doesn't belong to a guild.
pub async fn get_guild_id(pool: &PgPool, channel_id: Uuid) -> Option<Uuid> {
    sqlx::query_scalar("SELECT guild_id FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
}

/// Finalize session with aggregated metrics on disconnect.
///
/// Creates a session record in `connection_sessions` with aggregated
/// metrics from all connection metrics collected during the session.
/// For very short calls with no metrics, NULL aggregates are stored.
pub async fn finalize_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    channel_id: Uuid,
    guild_id: Option<Uuid>,
    started_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    // Check if any metrics exist for this session
    let has_metrics: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connection_metrics WHERE session_id = $1)")
            .bind(session_id)
            .fetch_one(pool)
            .await?;

    if has_metrics {
        // Insert session with aggregated metrics
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
    } else {
        // Insert session with NULL aggregates (very short call)
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
    }

    Ok(())
}
