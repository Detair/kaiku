//! Channel permission override handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{require_guild_permission, GuildPermissions, PermissionError};

// ============================================================================
// Error Type
// ============================================================================

#[derive(Debug, Error)]
pub enum OverrideError {
    #[error("Channel not found")]
    ChannelNotFound,

    #[error("Role not found")]
    RoleNotFound,

    #[error("Not a member of this guild")]
    NotMember,

    #[error("{0}")]
    Permission(#[from] PermissionError),

    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for OverrideError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Channel not found"}),
            ),
            Self::RoleNotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Role not found"}),
            ),
            Self::NotMember => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_member", "message": "Not a member of this guild"}),
            ),
            Self::Permission(e) => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "permission", "message": e.to_string()}),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "database", "message": "Database error"}),
            ),
        };
        (status, Json(body)).into_response()
    }
}

// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct OverrideResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub role_id: Uuid,
    pub allow_permissions: u64,
    pub deny_permissions: u64,
}

#[derive(Debug, Deserialize)]
pub struct SetOverrideRequest {
    pub allow: Option<u64>,
    pub deny: Option<u64>,
}

// ============================================================================
// Handlers
// ============================================================================

/// List all permission overrides for a channel.
///
/// `GET /api/channels/:channel_id/overrides`
#[tracing::instrument(skip(state))]
pub async fn list_overrides(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Vec<OverrideResponse>>, OverrideError> {
    // Get channel and its guild
    let channel: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT guild_id FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await?;

    let channel = channel.ok_or(OverrideError::ChannelNotFound)?;
    let guild_id = channel.0.ok_or(OverrideError::ChannelNotFound)?;

    // Check membership
    let _ctx = require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(|e| match e {
            PermissionError::NotGuildMember => OverrideError::NotMember,
            other => OverrideError::Permission(other),
        })?;

    let overrides = sqlx::query_as::<_, (Uuid, Uuid, Uuid, i64, i64)>(
        r"
        SELECT id, channel_id, role_id, allow_permissions, deny_permissions
        FROM channel_overrides
        WHERE channel_id = $1
        ",
    )
    .bind(channel_id)
    .fetch_all(&state.db)
    .await?;

    let response: Vec<OverrideResponse> = overrides
        .into_iter()
        .map(|(id, channel_id, role_id, allow, deny)| OverrideResponse {
            id,
            channel_id,
            role_id,
            allow_permissions: allow as u64,
            deny_permissions: deny as u64,
        })
        .collect();

    Ok(Json(response))
}

/// Set permission override for a role on a channel.
///
/// `PUT /api/channels/:channel_id/overrides/:role_id`
#[tracing::instrument(skip(state, body))]
pub async fn set_override(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, role_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetOverrideRequest>,
) -> Result<Json<OverrideResponse>, OverrideError> {
    // Get channel and its guild
    let channel: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT guild_id FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await?;

    let channel = channel.ok_or(OverrideError::ChannelNotFound)?;
    let guild_id = channel.0.ok_or(OverrideError::ChannelNotFound)?;

    // Check permission
    let ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_CHANNELS)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => OverrideError::NotMember,
                other => OverrideError::Permission(other),
            })?;

    // Verify role belongs to this guild
    let role_exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM guild_roles WHERE id = $1 AND guild_id = $2")
            .bind(role_id)
            .bind(guild_id)
            .fetch_optional(&state.db)
            .await?;

    if role_exists.is_none() {
        return Err(OverrideError::RoleNotFound);
    }

    // Security: Prevent permission escalation via channel overrides
    // Users cannot grant permissions they don't have themselves
    let allow_perms = GuildPermissions::from_bits_truncate(body.allow.unwrap_or(0));
    let escalation = allow_perms & !ctx.computed_permissions;
    if !escalation.is_empty() {
        return Err(OverrideError::Permission(PermissionError::CannotEscalate(
            escalation,
        )));
    }

    let allow = body.allow.unwrap_or(0) as i64;
    let deny = body.deny.unwrap_or(0) as i64;

    let override_entry = sqlx::query_as::<_, (Uuid, Uuid, Uuid, i64, i64)>(
        r"
        INSERT INTO channel_overrides (channel_id, role_id, allow_permissions, deny_permissions)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (channel_id, role_id) DO UPDATE SET
            allow_permissions = $3,
            deny_permissions = $4
        RETURNING id, channel_id, role_id, allow_permissions, deny_permissions
        ",
    )
    .bind(channel_id)
    .bind(role_id)
    .bind(allow)
    .bind(deny)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(OverrideResponse {
        id: override_entry.0,
        channel_id: override_entry.1,
        role_id: override_entry.2,
        allow_permissions: override_entry.3 as u64,
        deny_permissions: override_entry.4 as u64,
    }))
}

/// Remove permission override.
///
/// `DELETE /api/channels/:channel_id/overrides/:role_id`
#[tracing::instrument(skip(state))]
pub async fn delete_override(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, OverrideError> {
    // Get channel and its guild
    let channel: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT guild_id FROM channels WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(&state.db)
            .await?;

    let channel = channel.ok_or(OverrideError::ChannelNotFound)?;
    let guild_id = channel.0.ok_or(OverrideError::ChannelNotFound)?;

    // Check permission
    let _ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_CHANNELS)
            .await
            .map_err(|e| match e {
                PermissionError::NotGuildMember => OverrideError::NotMember,
                other => OverrideError::Permission(other),
            })?;

    let result = sqlx::query("DELETE FROM channel_overrides WHERE channel_id = $1 AND role_id = $2")
        .bind(channel_id)
        .bind(role_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(OverrideError::RoleNotFound);
    }

    Ok(Json(
        serde_json::json!({"deleted": true, "channel_id": channel_id, "role_id": role_id}),
    ))
}
