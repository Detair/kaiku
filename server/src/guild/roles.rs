//! Guild role management handlers.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::{
    can_manage_role, require_guild_permission, GuildPermissions, PermissionError,
};

use super::types::{CreateRoleRequest, RoleResponse, UpdateRoleRequest};

// ============================================================================
// Error Type
// ============================================================================

#[derive(Debug, Error)]
pub enum RoleError {
    #[error("Role not found")]
    NotFound,

    #[error("Not a member of this guild")]
    NotMember,

    #[error("{0}")]
    Permission(#[from] PermissionError),

    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Database error")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for RoleError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                serde_json::json!({"error": "not_found", "message": "Role not found"}),
            ),
            Self::NotMember => (
                StatusCode::FORBIDDEN,
                serde_json::json!({"error": "not_member", "message": "Not a member of this guild"}),
            ),
            Self::Permission(e) => {
                let body = match e {
                    PermissionError::MissingPermission(p) => serde_json::json!({
                        "error": "missing_permission",
                        "required": format!("{:?}", p),
                        "message": e.to_string()
                    }),
                    PermissionError::RoleHierarchy { actor_position, target_position } => serde_json::json!({
                        "error": "role_hierarchy",
                        "your_position": actor_position,
                        "target_position": target_position,
                        "message": e.to_string()
                    }),
                    PermissionError::CannotEscalate(p) => serde_json::json!({
                        "error": "cannot_escalate",
                        "attempted": format!("{:?}", p),
                        "message": e.to_string()
                    }),
                    PermissionError::NotGuildMember => serde_json::json!({
                        "error": "not_member",
                        "message": e.to_string()
                    }),
                    _ => serde_json::json!({
                        "error": "permission",
                        "message": e.to_string()
                    }),
                };
                (StatusCode::FORBIDDEN, body)
            }
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error": "validation", "message": msg}),
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
// Handlers
// ============================================================================

/// List all roles in a guild.
///
/// `GET /api/guilds/:guild_id/roles`
#[tracing::instrument(skip(state))]
pub async fn list_roles(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<RoleResponse>>, RoleError> {
    // Just need to be a member to view roles
    let _ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::empty(),
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => RoleError::NotMember,
        other => RoleError::Permission(other),
    })?;

    let roles = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, i64, i32, bool, chrono::DateTime<chrono::Utc>)>(
        r"
        SELECT id, guild_id, name, color, permissions, position, is_default, created_at
        FROM guild_roles
        WHERE guild_id = $1
        ORDER BY position ASC
        ",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    let response: Vec<RoleResponse> = roles
        .into_iter()
        .map(|(id, guild_id, name, color, permissions, position, is_default, created_at)| {
            RoleResponse {
                id,
                guild_id,
                name,
                color,
                permissions: permissions as u64,
                position,
                is_default,
                created_at,
            }
        })
        .collect();

    Ok(Json(response))
}

/// Create a new role.
///
/// `POST /api/guilds/:guild_id/roles`
#[tracing::instrument(skip(state, body))]
pub async fn create_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
    Json(body): Json<CreateRoleRequest>,
) -> Result<Json<RoleResponse>, RoleError> {
    body.validate()
        .map_err(|e| RoleError::Validation(e.to_string()))?;

    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => RoleError::NotMember,
        other => RoleError::Permission(other),
    })?;

    // Check if trying to grant permissions we don't have
    let new_perms = GuildPermissions::from_bits_truncate(body.permissions.unwrap_or(0));
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position.unwrap_or(i32::MAX),
        i32::MAX, // New role, no position yet
        Some(new_perms),
    )?;

    // Get next position (higher number = lower rank)
    let max_position: (i64,) =
        sqlx::query_as("SELECT COALESCE(MAX(position), 0) FROM guild_roles WHERE guild_id = $1")
            .bind(guild_id)
            .fetch_one(&state.db)
            .await?;

    let role_id = Uuid::now_v7();
    let position = max_position.0 as i32 + 1;

    let role = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, i64, i32, bool, chrono::DateTime<chrono::Utc>)>(
        r"
        INSERT INTO guild_roles (id, guild_id, name, color, permissions, position)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, guild_id, name, color, permissions, position, is_default, created_at
        ",
    )
    .bind(role_id)
    .bind(guild_id)
    .bind(&body.name)
    .bind(&body.color)
    .bind(new_perms.bits() as i64)
    .bind(position)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(RoleResponse {
        id: role.0,
        guild_id: role.1,
        name: role.2,
        color: role.3,
        permissions: role.4 as u64,
        position: role.5,
        is_default: role.6,
        created_at: role.7,
    }))
}

/// Update a role.
///
/// `PATCH /api/guilds/:guild_id/roles/:role_id`
#[tracing::instrument(skip(state, body))]
pub async fn update_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, role_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<RoleResponse>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => RoleError::NotMember,
        other => RoleError::Permission(other),
    })?;

    // Get current role
    let current_role: Option<(i32, i64, bool)> = sqlx::query_as(
        "SELECT position, permissions, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?;

    let current_role = current_role.ok_or(RoleError::NotFound)?;

    // Cannot edit @everyone role name
    if current_role.2 && body.name.is_some() {
        return Err(RoleError::Validation(
            "Cannot rename @everyone role".to_string(),
        ));
    }

    // Check hierarchy - cannot edit roles at or above our position
    let new_perms = body.permissions.map(GuildPermissions::from_bits_truncate);
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position.unwrap_or(i32::MAX),
        current_role.0,
        new_perms,
    )?;

    let role = sqlx::query_as::<_, (Uuid, Uuid, String, Option<String>, i64, i32, bool, chrono::DateTime<chrono::Utc>)>(
        r"
        UPDATE guild_roles SET
            name = COALESCE($3, name),
            color = COALESCE($4, color),
            permissions = COALESCE($5, permissions),
            position = COALESCE($6, position)
        WHERE id = $1 AND guild_id = $2
        RETURNING id, guild_id, name, color, permissions, position, is_default, created_at
        ",
    )
    .bind(role_id)
    .bind(guild_id)
    .bind(&body.name)
    .bind(&body.color)
    .bind(body.permissions.map(|p| p as i64))
    .bind(body.position)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(RoleResponse {
        id: role.0,
        guild_id: role.1,
        name: role.2,
        color: role.3,
        permissions: role.4 as u64,
        position: role.5,
        is_default: role.6,
        created_at: role.7,
    }))
}

/// Delete a role.
///
/// `DELETE /api/guilds/:guild_id/roles/:role_id`
#[tracing::instrument(skip(state))]
pub async fn delete_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, role_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => RoleError::NotMember,
        other => RoleError::Permission(other),
    })?;

    // Get role to check position and if it's default
    let role: Option<(i32, bool)> = sqlx::query_as(
        "SELECT position, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?;

    let role = role.ok_or(RoleError::NotFound)?;

    if role.1 {
        return Err(RoleError::Validation(
            "Cannot delete @everyone role".to_string(),
        ));
    }

    // Check hierarchy
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position.unwrap_or(i32::MAX),
        role.0,
        None,
    )?;

    sqlx::query("DELETE FROM guild_roles WHERE id = $1")
        .bind(role_id)
        .execute(&state.db)
        .await?;

    Ok(Json(
        serde_json::json!({"deleted": true, "role_id": role_id}),
    ))
}

/// Assign a role to a member.
///
/// `POST /api/guilds/:guild_id/members/:user_id/roles/:role_id`
#[tracing::instrument(skip(state))]
pub async fn assign_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id, role_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => RoleError::NotMember,
        other => RoleError::Permission(other),
    })?;

    // Get role to check position
    let role: Option<(i32, bool)> = sqlx::query_as(
        "SELECT position, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?;

    let role = role.ok_or(RoleError::NotFound)?;

    if role.1 {
        return Err(RoleError::Validation(
            "Cannot assign @everyone role".to_string(),
        ));
    }

    // Check hierarchy
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position.unwrap_or(i32::MAX),
        role.0,
        None,
    )?;

    // Check target is a member
    let is_member: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;

    if is_member.is_none() {
        return Err(RoleError::Validation(
            "User is not a member of this guild".to_string(),
        ));
    }

    // Assign role (ignore if already assigned)
    sqlx::query(
        r"
        INSERT INTO guild_member_roles (guild_id, user_id, role_id, assigned_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (guild_id, user_id, role_id) DO NOTHING
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .bind(auth.id)
    .execute(&state.db)
    .await?;

    Ok(Json(
        serde_json::json!({"assigned": true, "user_id": user_id, "role_id": role_id}),
    ))
}

/// Remove a role from a member.
///
/// `DELETE /api/guilds/:guild_id/members/:user_id/roles/:role_id`
#[tracing::instrument(skip(state))]
pub async fn remove_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((guild_id, user_id, role_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, RoleError> {
    let ctx = require_guild_permission(
        &state.db,
        guild_id,
        auth.id,
        GuildPermissions::MANAGE_ROLES,
    )
    .await
    .map_err(|e| match e {
        PermissionError::NotGuildMember => RoleError::NotMember,
        other => RoleError::Permission(other),
    })?;

    // Get role to check position
    let role: Option<(i32,)> = sqlx::query_as(
        "SELECT position FROM guild_roles WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?;

    let role = role.ok_or(RoleError::NotFound)?;

    // Check hierarchy
    can_manage_role(
        ctx.computed_permissions,
        ctx.highest_role_position.unwrap_or(i32::MAX),
        role.0,
        None,
    )?;

    let result = sqlx::query(
        "DELETE FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(RoleError::NotFound);
    }

    Ok(Json(
        serde_json::json!({"removed": true, "user_id": user_id, "role_id": role_id}),
    ))
}
