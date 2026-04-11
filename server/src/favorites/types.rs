//! Request and response types for the favorites API.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Maximum number of favorites per user.
pub(super) const MAX_FAVORITES_PER_USER: i64 = 25;

/// Database row returned by the favorites list query.
#[derive(Debug, Serialize, FromRow)]
pub struct FavoriteChannelRow {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub channel_type: String,
    pub guild_id: Uuid,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub guild_position: i32,
    pub channel_position: i32,
}

/// Favorite channel with string IDs for API responses.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FavoriteChannel {
    pub channel_id: String,
    pub channel_name: String,
    pub channel_type: String,
    pub guild_id: String,
    pub guild_name: String,
    pub guild_icon: Option<String>,
    pub guild_position: i32,
    pub channel_position: i32,
}

impl From<FavoriteChannelRow> for FavoriteChannel {
    fn from(row: FavoriteChannelRow) -> Self {
        Self {
            channel_id: row.channel_id.to_string(),
            channel_name: row.channel_name,
            channel_type: row.channel_type,
            guild_id: row.guild_id.to_string(),
            guild_name: row.guild_name,
            guild_icon: row.guild_icon,
            guild_position: row.guild_position,
            channel_position: row.channel_position,
        }
    }
}

/// Response listing a user's favorite channels.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FavoritesResponse {
    pub favorites: Vec<FavoriteChannel>,
}

/// Database row for a single favorite entry.
#[derive(Debug, Serialize, FromRow)]
pub struct FavoriteRow {
    pub channel_id: Uuid,
    pub guild_id: Uuid,
    pub position: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Single favorite with string IDs.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Favorite {
    pub channel_id: String,
    pub guild_id: String,
    pub guild_position: i32,
    pub channel_position: i32,
    pub created_at: String,
}

/// Request to reorder favorite channels within a guild.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderChannelsRequest {
    pub guild_id: String,
    pub channel_ids: Vec<String>,
}

/// Request to reorder guild groups in favorites.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderGuildsRequest {
    pub guild_ids: Vec<String>,
}
