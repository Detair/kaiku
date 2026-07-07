//! Self-assignable (reaction) role bindings.
//!
//! Admins bind an emoji on a message to a role; members react to grant/revoke
//! it themselves. Creation is gated by `MANAGE_ROLES` + a hierarchy guard +
//! a "not dangerous" guard; reaction-time self-assign is intentionally
//! unprivileged (that is the feature).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

use super::queries::reaction_roles as queries;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{
    can_manage_role, require_guild_permission, GuildPermissions, PermissionError,
};

/// Reject binding a role that carries any permission a member must never be
/// able to self-grant. Reuses the canonical `EVERYONE_FORBIDDEN` deny-list so
/// this stays in lockstep with the @everyone guard.
///
/// Returns `true` if the role is safe to make self-assignable.
#[must_use]
pub fn is_role_self_assignable(role_permissions: GuildPermissions) -> bool {
    !role_permissions.intersects(GuildPermissions::EVERYONE_FORBIDDEN)
}

/// Body for `POST /api/guilds/{id}/reaction-roles`.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateReactionRoleRequest {
    pub channel_id: Uuid,
    pub message_id: Uuid,
    #[validate(length(min = 1, max = 128))]
    pub emoji: String,
    pub role_id: Uuid,
    /// "toggle" (default) or "unique".
    #[serde(default = "default_mode")]
    #[validate(custom(function = "validate_mode"))]
    pub mode: String,
    #[validate(length(max = 64))]
    pub group_key: Option<String>,
}

fn default_mode() -> String {
    "toggle".to_string()
}

fn validate_mode(mode: &str) -> Result<(), validator::ValidationError> {
    if mode == "toggle" || mode == "unique" {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_mode"))
    }
}

/// A reaction-role binding as returned by the API.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReactionRoleResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub channel_id: Uuid,
    pub message_id: Uuid,
    pub emoji: String,
    pub role_id: Uuid,
    pub group_key: Option<String>,
    pub mode: String,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug, Error)]
pub enum ReactionRoleError {
    #[error("Binding not found")]
    NotFound,
    #[error("Not a member of this guild")]
    NotMember,
    #[error("Role not found")]
    RoleNotFound,
    #[error("Message not found in channel")]
    MessageNotFound,
    #[error("Channel not in guild")]
    ChannelNotFound,
    #[error("Role is not self-assignable (carries a privileged permission)")]
    RoleNotSelfAssignable,
    #[error("Cannot bind the @everyone/default role")]
    DefaultRoleNotBindable,
    #[error("A binding for this emoji already exists on the message")]
    DuplicateBinding,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    #[error("Validation failed: {0}")]
    Validation(String),
    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for ReactionRoleError {
    fn into_response(self) -> Response {
        if let Self::Database(db_err) = &self {
            tracing::error!(error = %db_err, "Reaction-role database operation failed");
        }
        let (status, code, message) = match &self {
            Self::NotFound => (StatusCode::NOT_FOUND, "BINDING_NOT_FOUND", self.to_string()),
            Self::NotMember => (StatusCode::FORBIDDEN, "NOT_MEMBER", self.to_string()),
            Self::RoleNotFound => (StatusCode::NOT_FOUND, "ROLE_NOT_FOUND", self.to_string()),
            Self::MessageNotFound => {
                (StatusCode::NOT_FOUND, "MESSAGE_NOT_FOUND", self.to_string())
            }
            Self::ChannelNotFound => {
                (StatusCode::NOT_FOUND, "CHANNEL_NOT_FOUND", self.to_string())
            }
            Self::RoleNotSelfAssignable => (
                StatusCode::FORBIDDEN,
                "ROLE_NOT_SELF_ASSIGNABLE",
                self.to_string(),
            ),
            Self::DefaultRoleNotBindable => (
                StatusCode::BAD_REQUEST,
                "DEFAULT_ROLE_NOT_BINDABLE",
                self.to_string(),
            ),
            Self::DuplicateBinding => {
                (StatusCode::CONFLICT, "DUPLICATE_BINDING", self.to_string())
            }
            Self::Permission(_) => {
                (StatusCode::FORBIDDEN, "PERMISSION_DENIED", self.to_string())
            }
            Self::Validation(_) => {
                (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", self.to_string())
            }
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}

fn map_perm(e: PermissionError) -> ReactionRoleError {
    match e {
        PermissionError::NotGuildMember => ReactionRoleError::NotMember,
        other => ReactionRoleError::Permission(other),
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// Query params for listing.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub message_id: Option<Uuid>,
}

/// `GET /api/guilds/{id}/reaction-roles`
#[utoipa::path(
    get,
    path = "/api/guilds/{id}/reaction-roles",
    tag = "reaction-roles",
    params(("id" = Uuid, Path, description = "Guild ID")),
    responses((status = 200, body = Vec<ReactionRoleResponse>)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn list_reaction_roles(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<ReactionRoleResponse>>, ReactionRoleError> {
    // Membership is enough to view bindings.
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::empty())
        .await
        .map_err(map_perm)?;
    let bindings = queries::list_bindings(&state.db, guild_id, q.message_id).await?;
    Ok(Json(bindings))
}

/// `POST /api/guilds/{id}/reaction-roles`
#[utoipa::path(
    post,
    path = "/api/guilds/{id}/reaction-roles",
    tag = "reaction-roles",
    params(("id" = Uuid, Path, description = "Guild ID")),
    request_body = CreateReactionRoleRequest,
    responses((status = 200, body = ReactionRoleResponse)),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state, body))]
pub async fn create_reaction_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateReactionRoleRequest>,
) -> Result<Json<ReactionRoleResponse>, ReactionRoleError> {
    body.validate().map_err(|e| {
        ReactionRoleError::Validation(crate::validation::format_validation_errors(&e))
    })?;

    let ctx =
        require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_ROLES)
            .await
            .map_err(map_perm)?;

    // Validate the channel belongs to the guild and the message to the channel.
    if !queries::channel_in_guild(&state.db, body.channel_id, guild_id).await? {
        return Err(ReactionRoleError::ChannelNotFound);
    }
    if !queries::message_in_channel(&state.db, body.message_id, body.channel_id).await? {
        return Err(ReactionRoleError::MessageNotFound);
    }

    // Load the target role's guard state.
    let (position, perms_bits, is_default) =
        queries::fetch_role_guard_state(&state.db, guild_id, body.role_id)
            .await?
            .ok_or(ReactionRoleError::RoleNotFound)?;

    if is_default {
        return Err(ReactionRoleError::DefaultRoleNotBindable);
    }

    // Hierarchy guard: target role must be strictly below the actor's highest.
    let actor_position = if ctx.is_owner {
        -1
    } else {
        ctx.highest_role_position.unwrap_or(i32::MAX)
    };
    can_manage_role(ctx.computed_permissions, actor_position, position, None)?;

    // Dangerous-permission guard.
    let role_perms = GuildPermissions::from_bits_truncate(perms_bits as u64);
    if !is_role_self_assignable(role_perms) {
        return Err(ReactionRoleError::RoleNotSelfAssignable);
    }

    let binding = queries::insert_binding(
        &state.db,
        guild_id,
        body.channel_id,
        body.message_id,
        &body.emoji,
        body.role_id,
        body.group_key.as_deref(),
        &body.mode,
        auth.id,
    )
    .await
    .map_err(|e| match e {
        ReactionRoleError::Database(sqlx::Error::Database(ref db)) if db.is_unique_violation() => {
            ReactionRoleError::DuplicateBinding
        }
        other => other,
    })?;

    Ok(Json(binding))
}

/// `DELETE /api/guilds/{id}/reaction-roles/{binding_id}`
#[utoipa::path(
    delete,
    path = "/api/guilds/{id}/reaction-roles/{binding_id}",
    tag = "reaction-roles",
    params(
        ("id" = Uuid, Path, description = "Guild ID"),
        ("binding_id" = Uuid, Path, description = "Binding ID")
    ),
    responses((status = 200, description = "Binding removed")),
    security(("bearer_auth" = []))
)]
#[tracing::instrument(skip(state))]
pub async fn delete_reaction_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, binding_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ReactionRoleError> {
    require_guild_permission(&state.db, guild_id, auth.id, GuildPermissions::MANAGE_ROLES)
        .await
        .map_err(map_perm)?;

    let affected = queries::delete_binding(&state.db, guild_id, binding_id).await?;
    if affected == 0 {
        return Err(ReactionRoleError::NotFound);
    }
    Ok(Json(serde_json::json!({ "deleted": true, "id": binding_id })))
}

// ============================================================================
// Reaction-time hook
// ============================================================================

/// Outcome of a reaction-role hook: what to broadcast after commit.
pub struct HookOutcome {
    /// The member's full role set after the change.
    pub role_ids: Vec<Uuid>,
    /// Sibling emojis whose reaction rows were cleared (unique swap) — the
    /// caller broadcasts a ReactionRemove for each so other clients update.
    pub cleared_emojis: Vec<String>,
}

/// Apply reaction-role effects for an added reaction, inside `tx`.
///
/// Returns `None` if there is no binding for (message_id, emoji) or the user
/// is not a guild member.
pub async fn apply_on_reaction_add(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<Option<HookOutcome>, ReactionRoleError> {
    let Some(binding) = queries::find_binding_for_reaction(tx, message_id, emoji).await? else {
        return Ok(None);
    };
    if !queries::is_member(tx, guild_id, user_id).await? {
        return Ok(None);
    }

    queries::tx_assign_role(tx, guild_id, user_id, binding.role_id, user_id).await?;

    let mut cleared_emojis = Vec::new();
    if binding.mode == "unique" {
        if let Some(group) = binding.group_key.as_deref() {
            let siblings = queries::sibling_bindings_in_group(tx, message_id, group, emoji).await?;
            for (sib_emoji, sib_role) in siblings {
                queries::tx_remove_role(tx, guild_id, user_id, sib_role).await?;
                queries::tx_remove_user_reaction(tx, message_id, user_id, &sib_emoji).await?;
                cleared_emojis.push(sib_emoji);
            }
        }
    }

    let role_ids = queries::tx_member_role_ids(tx, guild_id, user_id).await?;
    Ok(Some(HookOutcome {
        role_ids,
        cleared_emojis,
    }))
}

/// Apply reaction-role effects for a removed reaction, inside `tx`.
pub async fn apply_on_reaction_remove(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<Option<HookOutcome>, ReactionRoleError> {
    let Some(binding) = queries::find_binding_for_reaction(tx, message_id, emoji).await? else {
        return Ok(None);
    };
    if !queries::is_member(tx, guild_id, user_id).await? {
        return Ok(None);
    }
    queries::tx_remove_role(tx, guild_id, user_id, binding.role_id).await?;
    let role_ids = queries::tx_member_role_ids(tx, guild_id, user_id).await?;
    Ok(Some(HookOutcome {
        role_ids,
        cleared_emojis: Vec::new(),
    }))
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn safe_roles_are_self_assignable() {
        let safe = GuildPermissions::SEND_MESSAGES
            | GuildPermissions::VOICE_CONNECT
            | GuildPermissions::ADD_REACTIONS;
        assert!(is_role_self_assignable(safe));
        // A pure cosmetic role (no perms) is fine.
        assert!(is_role_self_assignable(GuildPermissions::empty()));
    }

    #[test]
    fn dangerous_roles_are_not_self_assignable() {
        for perm in [
            GuildPermissions::MANAGE_ROLES,
            GuildPermissions::MANAGE_GUILD,
            GuildPermissions::BAN_MEMBERS,
            GuildPermissions::KICK_MEMBERS,
            GuildPermissions::MANAGE_CHANNELS,
        ] {
            let perms = GuildPermissions::SEND_MESSAGES | perm;
            assert!(
                !is_role_self_assignable(perms),
                "{perm:?} must make a role non-self-assignable"
            );
        }
    }
}
