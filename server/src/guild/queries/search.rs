//! Guild search queries.
//!
//! Most of the heavy lifting (`db::search_messages_filtered`,
//! `db::count_search_messages_filtered`, etc.) lives in `crate::db` and is
//! called directly by the handler. The functions here cover the small inline
//! queries that the search handler issues.

use sqlx::PgPool;
use uuid::Uuid;

use super::super::search::SearchError;

/// Check whether a guild exists.
pub async fn guild_exists(pool: &PgPool, guild_id: Uuid) -> Result<bool, SearchError> {
    let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM guilds WHERE id = $1)")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// Fetch `(channel_id, name)` pairs for a set of channels (used to look up
/// channel display names for search results).
pub async fn fetch_channel_names(
    pool: &PgPool,
    channel_ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, SearchError> {
    let channels: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM channels WHERE id = ANY($1)")
            .bind(channel_ids)
            .fetch_all(pool)
            .await?;
    Ok(channels)
}
