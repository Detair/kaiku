//! HTTP handlers for the favorites API.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;

use super::error::FavoritesError;
use super::types::{
    Favorite, FavoriteChannel, FavoriteChannelRow, FavoriteRow, FavoritesResponse,
    ReorderChannelsRequest, ReorderGuildsRequest, MAX_FAVORITES_PER_USER,
};

/// GET /api/me/favorites - List user's favorite channels
#[utoipa::path(
    get,
    path = "/api/me/favorites",
    tag = "favorites",
    responses(
        (status = 200, description = "List of favorites"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_favorites(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> Result<Json<FavoritesResponse>, FavoritesError> {
    let rows = sqlx::query_as::<_, FavoriteChannelRow>(
        r"
        SELECT
            fc.channel_id,
            c.name as channel_name,
            c.channel_type::TEXT as channel_type,
            fc.guild_id,
            g.name as guild_name,
            g.icon_url as guild_icon,
            fg.position as guild_position,
            fc.position as channel_position
        FROM user_favorite_channels fc
        JOIN user_favorite_guilds fg ON fg.user_id = fc.user_id AND fg.guild_id = fc.guild_id
        JOIN channels c ON c.id = fc.channel_id
        JOIN guilds g ON g.id = fc.guild_id
        JOIN guild_members gm ON gm.guild_id = fc.guild_id AND gm.user_id = fc.user_id
        WHERE fc.user_id = $1
        ORDER BY fg.position ASC, fc.position ASC
        ",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    let favorites: Vec<FavoriteChannel> = rows.into_iter().map(FavoriteChannel::from).collect();
    Ok(Json(FavoritesResponse { favorites }))
}

/// POST `/api/me/favorites/:channel_id` - Add channel to favorites
#[utoipa::path(
    post,
    path = "/api/me/favorites/{channel_id}",
    tag = "favorites",
    params(
        ("channel_id" = Uuid, Path, description = "Channel ID"),
    ),
    responses(
        (status = 200, description = "Favorite added", body = Favorite),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn add_favorite(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Favorite>, FavoritesError> {
    // 1. Check limit (max 25)
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_favorite_channels WHERE user_id = $1")
            .bind(auth_user.id)
            .fetch_one(&state.db)
            .await?;

    if count.0 >= MAX_FAVORITES_PER_USER {
        return Err(FavoritesError::LimitExceeded);
    }

    // 2. Verify channel exists and get guild_id
    let channel = sqlx::query_as::<_, (Uuid, Option<Uuid>)>(
        "SELECT id, guild_id FROM channels WHERE id = $1",
    )
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(FavoritesError::ChannelNotFound)?;

    let guild_id = channel.1.ok_or(FavoritesError::InvalidChannel)?;

    // 3. Verify user has access to guild
    let is_member = sqlx::query("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(auth_user.id)
        .fetch_optional(&state.db)
        .await?
        .is_some();

    if !is_member {
        return Err(FavoritesError::ChannelNotFound); // Don't leak existence
    }

    // 4. Verify user has VIEW_CHANNEL permission for the channel
    crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| FavoritesError::ChannelNotFound)?; // Generic error to avoid leaking permission details

    // 5. Transaction for atomic insert
    let mut tx = state.db.begin().await?;

    // 6. Insert guild entry (ON CONFLICT for race condition)
    sqlx::query(
        r"
        INSERT INTO user_favorite_guilds (user_id, guild_id, position)
        SELECT $1, $2, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_guilds WHERE user_id = $1), 0)
        ON CONFLICT (user_id, guild_id) DO NOTHING
        ",
    )
    .bind(auth_user.id)
    .bind(guild_id)
    .execute(&mut *tx)
    .await?;

    // 7. Insert channel entry
    let result = sqlx::query_as::<_, FavoriteRow>(
        r"
        INSERT INTO user_favorite_channels (user_id, channel_id, guild_id, position)
        VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $3), 0))
        RETURNING channel_id, guild_id, position, created_at
        ",
    )
    .bind(auth_user.id)
    .bind(channel_id)
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await;

    let favorite = match result {
        Ok(row) => row,
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
            return Err(FavoritesError::AlreadyFavorited);
        }
        Err(e) => return Err(FavoritesError::Database(e)),
    };

    // 7. Get guild_position for response
    let guild_pos: (i32,) = sqlx::query_as(
        "SELECT position FROM user_favorite_guilds WHERE user_id = $1 AND guild_id = $2",
    )
    .bind(auth_user.id)
    .bind(guild_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(Favorite {
        channel_id: favorite.channel_id.to_string(),
        guild_id: favorite.guild_id.to_string(),
        guild_position: guild_pos.0,
        channel_position: favorite.position,
        created_at: favorite.created_at.to_rfc3339(),
    }))
}

/// DELETE `/api/me/favorites/:channel_id` - Remove channel from favorites
#[utoipa::path(
    delete,
    path = "/api/me/favorites/{channel_id}",
    tag = "favorites",
    params(
        ("channel_id" = Uuid, Path, description = "Channel ID"),
    ),
    responses(
        (status = 204, description = "Favorite removed"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn remove_favorite(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<StatusCode, FavoritesError> {
    let result =
        sqlx::query("DELETE FROM user_favorite_channels WHERE user_id = $1 AND channel_id = $2")
            .bind(auth_user.id)
            .bind(channel_id)
            .execute(&state.db)
            .await?;

    if result.rows_affected() == 0 {
        return Err(FavoritesError::NotFavorited);
    }

    // Trigger handles guild cleanup automatically
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/me/favorites/reorder - Reorder channels within a guild
#[utoipa::path(
    put,
    path = "/api/me/favorites/reorder",
    tag = "favorites",
    request_body = ReorderChannelsRequest,
    responses(
        (status = 204, description = "Favorites reordered"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn reorder_channels(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderChannelsRequest>,
) -> Result<StatusCode, FavoritesError> {
    let guild_id = Uuid::parse_str(&request.guild_id).map_err(|_| FavoritesError::InvalidGuilds)?;

    // Start transaction for atomic reorder
    let mut tx = state.db.begin().await?;

    // Verify all channel IDs belong to user's favorites in this guild
    let existing: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT channel_id FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $2",
    )
    .bind(auth_user.id)
    .bind(guild_id)
    .fetch_all(&mut *tx)
    .await?;

    let existing_ids: std::collections::HashSet<String> =
        existing.iter().map(|r| r.0.to_string()).collect();

    // Verify all provided IDs are valid
    for id in &request.channel_ids {
        if !existing_ids.contains(id) {
            return Err(FavoritesError::InvalidChannels);
        }
    }

    // Update positions within transaction
    for (position, channel_id_str) in request.channel_ids.iter().enumerate() {
        let channel_id =
            Uuid::parse_str(channel_id_str).map_err(|_| FavoritesError::InvalidChannels)?;

        sqlx::query(
            "UPDATE user_favorite_channels SET position = $3 WHERE user_id = $1 AND channel_id = $2",
        )
        .bind(auth_user.id)
        .bind(channel_id)
        .bind(position as i32)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/me/favorites/reorder-guilds - Reorder guild groups
#[utoipa::path(
    put,
    path = "/api/me/favorites/reorder-guilds",
    tag = "favorites",
    request_body = ReorderGuildsRequest,
    responses(
        (status = 204, description = "Guild favorites reordered"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn reorder_guilds(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<ReorderGuildsRequest>,
) -> Result<StatusCode, FavoritesError> {
    // Start transaction for atomic reorder
    let mut tx = state.db.begin().await?;

    // Verify all guild IDs belong to user's favorites
    let existing: Vec<(Uuid,)> =
        sqlx::query_as("SELECT guild_id FROM user_favorite_guilds WHERE user_id = $1")
            .bind(auth_user.id)
            .fetch_all(&mut *tx)
            .await?;

    let existing_ids: std::collections::HashSet<String> =
        existing.iter().map(|r| r.0.to_string()).collect();

    // Verify all provided IDs are valid
    for id in &request.guild_ids {
        if !existing_ids.contains(id) {
            return Err(FavoritesError::InvalidGuilds);
        }
    }

    // Update positions within transaction
    for (position, guild_id_str) in request.guild_ids.iter().enumerate() {
        let guild_id = Uuid::parse_str(guild_id_str).map_err(|_| FavoritesError::InvalidGuilds)?;

        sqlx::query(
            "UPDATE user_favorite_guilds SET position = $3 WHERE user_id = $1 AND guild_id = $2",
        )
        .bind(auth_user.id)
        .bind(guild_id)
        .bind(position as i32)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
