//! Guild Management Handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;
use validator::Validate;

use super::types::{CreateGuildRequest, Guild, GuildMember, JoinGuildRequest, UpdateGuildRequest};
use crate::{api::AppState, auth::AuthUser, db};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum GuildError {
    NotFound,
    Forbidden,
    Validation(String),
    Database(sqlx::Error),
}

impl IntoResponse for GuildError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Guild not found"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Access denied"),
            Self::Validation(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response()
            }
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<sqlx::Error> for GuildError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new guild
#[tracing::instrument(skip(state))]
pub async fn create_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // Validate request
    body.validate()
        .map_err(|e| GuildError::Validation(e.to_string()))?;

    // Insert guild
    let guild_id = Uuid::now_v7();
    let guild = sqlx::query_as::<_, Guild>(
        r"INSERT INTO guilds (id, name, owner_id, description)
           VALUES ($1, $2, $3, $4)
           RETURNING id, name, owner_id, icon_url, description, created_at",
    )
    .bind(guild_id)
    .bind(&body.name)
    .bind(auth.id)
    .bind(&body.description)
    .fetch_one(&state.db)
    .await?;

    // Add owner as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    // Create default @everyone role
    sqlx::query(
        r"INSERT INTO guild_roles (guild_id, name, permissions, position, is_default)
           VALUES ($1, 'everyone', 0, 0, true)",
    )
    .bind(guild_id)
    .execute(&state.db)
    .await?;

    Ok(Json(guild))
}

/// List guilds for the current user
#[tracing::instrument(skip(state))]
pub async fn list_guilds(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<Guild>>, GuildError> {
    let guilds = sqlx::query_as::<_, Guild>(
        r"SELECT g.id, g.name, g.owner_id, g.icon_url, g.description, g.created_at
           FROM guilds g
           INNER JOIN guild_members gm ON g.id = gm.guild_id
           WHERE gm.user_id = $1
           ORDER BY g.created_at",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(guilds))
}

/// Get guild details
#[tracing::instrument(skip(state))]
pub async fn get_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Guild>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let guild = sqlx::query_as::<_, Guild>(
        "SELECT id, name, owner_id, icon_url, description, created_at FROM guilds WHERE id = $1",
    )
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(GuildError::NotFound)?;

    Ok(Json(guild))
}

/// Update guild
#[tracing::instrument(skip(state))]
pub async fn update_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<UpdateGuildRequest>,
) -> Result<Json<Guild>, GuildError> {
    // Validate request
    body.validate()
        .map_err(|e| GuildError::Validation(e.to_string()))?;

    // Verify ownership
    let owner_check: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Build dynamic update query
    let mut query_parts = vec![];
    let mut bindings: Vec<String> = vec![];

    if let Some(name) = &body.name {
        query_parts.push(format!("name = ${}", bindings.len() + 2));
        bindings.push(name.clone());
    }
    if body.description.is_some() {
        query_parts.push(format!("description = ${}", bindings.len() + 2));
        bindings.push(body.description.clone().unwrap_or_default());
    }
    if body.icon_url.is_some() {
        query_parts.push(format!("icon_url = ${}", bindings.len() + 2));
        bindings.push(body.icon_url.clone().unwrap_or_default());
    }

    if query_parts.is_empty() {
        // No changes, return current guild
        return get_guild(State(state), auth, Path(guild_id)).await;
    }

    let query_str = format!(
        "UPDATE guilds SET {} WHERE id = $1 RETURNING id, name, owner_id, icon_url, description, created_at",
        query_parts.join(", ")
    );

    // Execute update with proper bindings
    let mut query = sqlx::query_as::<_, Guild>(&query_str).bind(guild_id);
    if let Some(name) = body.name {
        query = query.bind(name);
    }
    if let Some(desc) = body.description {
        query = query.bind(desc);
    }
    if let Some(icon) = body.icon_url {
        query = query.bind(icon);
    }

    let updated_guild = query.fetch_one(&state.db).await?;

    Ok(Json(updated_guild))
}

/// Delete guild (owner only)
#[tracing::instrument(skip(state))]
pub async fn delete_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_check: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Join guild (placeholder - requires invite system)
#[tracing::instrument(skip(state))]
pub async fn join_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(_): Json<JoinGuildRequest>,
) -> Result<StatusCode, GuildError> {
    // Verify guild exists
    let guild_check: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    if guild_check.is_none() {
        return Err(GuildError::NotFound);
    }

    // Check if already a member
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if is_member {
        return Ok(StatusCode::OK);
    }

    // Add as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::OK)
}

/// Leave guild
#[tracing::instrument(skip(state))]
pub async fn leave_guild(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<StatusCode, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::NotFound);
    }

    // Check if owner (owners can't leave, must transfer ownership first)
    let owner_check: (Uuid,) = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_one(&state.db)
        .await?;

    if owner_check.0 == auth.id {
        return Err(GuildError::Validation(
            "Guild owner must transfer ownership before leaving".to_string(),
        ));
    }

    // Remove membership
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(auth.id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// List guild members
#[tracing::instrument(skip(state))]
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildMember>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let members = sqlx::query_as::<_, GuildMember>(
        r"SELECT
            u.id as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            gm.nickname,
            gm.joined_at,
            u.status::text as status,
            u.last_seen_at
           FROM guild_members gm
           INNER JOIN users u ON gm.user_id = u.id
           WHERE gm.guild_id = $1
           ORDER BY gm.joined_at",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(members))
}

/// Kick a member from guild (owner only)
#[tracing::instrument(skip(state))]
pub async fn kick_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, GuildError> {
    // Verify ownership
    let owner_check: Option<(Uuid,)> = sqlx::query_as("SELECT owner_id FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_optional(&state.db)
        .await?;

    let owner_id = owner_check.ok_or(GuildError::NotFound)?.0;

    if owner_id != auth.id {
        return Err(GuildError::Forbidden);
    }

    // Cannot kick yourself (owner)
    if user_id == auth.id {
        return Err(GuildError::Validation(
            "Cannot kick yourself from the guild".to_string(),
        ));
    }

    // Remove membership
    let result = sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
        .bind(guild_id)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(GuildError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List guild channels
#[tracing::instrument(skip(state))]
pub async fn list_channels(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<db::Channel>>, GuildError> {
    // Verify membership
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    let channels = db::get_guild_channels(&state.db, guild_id).await?;

    Ok(Json(channels))
}
