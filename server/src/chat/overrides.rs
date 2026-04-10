//! Channel permission override handlers.

use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use super::queries;
use super::types::{OverrideResponse, SetOverrideRequest};
use super::ChatError;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{GuildPermissions, PermissionError};

// ============================================================================
// Handlers
// ============================================================================

/// List all permission overrides for a channel.
///
/// `GET /api/channels/:channel_id/overrides`
#[utoipa::path(
    get,
    path = "/api/channels/{id}/overrides",
    tag = "overrides",
    params(("id" = Uuid, Path, description = "Channel ID")),
    responses(
        (status = 200, body = Vec<OverrideResponse>),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn list_overrides(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<Vec<OverrideResponse>>, ChatError> {
    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth.id, channel_id)
        .await
        .map_err(|e| match e {
            PermissionError::NotGuildMember => ChatError::NotMember,
            PermissionError::NotFound => ChatError::ChannelNotFound,
            other => ChatError::Permission(other),
        })?;

    if !ctx.has_permission(GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Permission(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_CHANNELS,
        )));
    }

    let overrides = queries::list_channel_overrides(&state.db, channel_id).await?;

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
#[utoipa::path(
    put,
    path = "/api/channels/{id}/overrides/{role_id}",
    tag = "overrides",
    params(
        ("id" = Uuid, Path, description = "Channel ID"),
        ("role_id" = Uuid, Path, description = "Role ID"),
    ),
    request_body = SetOverrideRequest,
    responses(
        (status = 200, body = OverrideResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body))]
pub async fn set_override(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, role_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetOverrideRequest>,
) -> Result<Json<OverrideResponse>, ChatError> {
    // Get channel to check guild_id
    let guild_id = queries::find_channel_guild_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?
        .ok_or(ChatError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth.id, channel_id)
        .await
        .map_err(|e| match e {
            PermissionError::NotGuildMember => ChatError::NotMember,
            PermissionError::NotFound => ChatError::ChannelNotFound,
            other => ChatError::Permission(other),
        })?;

    if !ctx.has_permission(GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Permission(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_CHANNELS,
        )));
    }

    // Verify role belongs to this guild
    if !queries::guild_role_exists(&state.db, role_id, guild_id).await? {
        return Err(ChatError::RoleNotFound);
    }

    // Security: Prevent permission escalation via channel overrides
    // Users cannot grant permissions they don't have themselves
    let allow_perms = GuildPermissions::from_bits_truncate(body.allow.unwrap_or(0));
    let escalation = allow_perms & !ctx.computed_permissions;
    if !escalation.is_empty() {
        return Err(ChatError::Permission(PermissionError::CannotEscalate(
            escalation,
        )));
    }

    let allow = body.allow.unwrap_or(0) as i64;
    let deny = body.deny.unwrap_or(0) as i64;

    let override_entry =
        queries::upsert_channel_override(&state.db, channel_id, role_id, allow, deny).await?;

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
#[utoipa::path(
    delete,
    path = "/api/channels/{id}/overrides/{role_id}",
    tag = "overrides",
    params(
        ("id" = Uuid, Path, description = "Channel ID"),
        ("role_id" = Uuid, Path, description = "Role ID"),
    ),
    responses(
        (status = 200, description = "Override deleted"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn delete_override(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ChatError> {
    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth.id, channel_id)
        .await
        .map_err(|e| match e {
            PermissionError::NotGuildMember => ChatError::NotMember,
            PermissionError::NotFound => ChatError::ChannelNotFound,
            other => ChatError::Permission(other),
        })?;

    if !ctx.has_permission(GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Permission(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_CHANNELS,
        )));
    }

    if !queries::delete_channel_override(&state.db, channel_id, role_id).await? {
        return Err(ChatError::RoleNotFound);
    }

    Ok(Json(
        serde_json::json!({"deleted": true, "channel_id": channel_id, "role_id": role_id}),
    ))
}
