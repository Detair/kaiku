//! Voice metrics storage.
//!
//! This module provides functions for storing voice connection metrics
//! in `TimescaleDB` for historical analysis. The actual SQL lives in
//! [`super::queries`]; this file holds the small amount of orchestration
//! and logging that wraps the raw queries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::queries;
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
    if let Err(e) =
        queries::insert_connection_metric(&pool, &stats, user_id, channel_id, guild_id).await
    {
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
    queries::find_channel_guild_id(pool, channel_id).await
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
    if queries::session_has_connection_metrics(pool, session_id).await? {
        queries::insert_session_with_aggregated_metrics(
            pool,
            session_id,
            user_id,
            channel_id,
            guild_id,
            started_at,
        )
        .await
    } else {
        queries::insert_session_without_metrics(
            pool,
            session_id,
            user_id,
            channel_id,
            guild_id,
            started_at,
        )
        .await
    }
}
