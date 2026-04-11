//! Guild-role queries.
//!
//! Functions return `Result<T, RoleError>` so handlers can use `?` directly.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::roles::RoleError;

/// Tuple representing a `guild_roles` row in the order returned by SELECT/INSERT
/// statements throughout this module.
#[allow(clippy::type_complexity)]
pub type RoleRow = (
    Uuid,
    Uuid,
    String,
    Option<String>,
    i64,
    i32,
    bool,
    DateTime<Utc>,
);

// ============================================================================
// Roles
// ============================================================================

/// List all roles in a guild ordered by position.
pub async fn list_roles(pool: &PgPool, guild_id: Uuid) -> Result<Vec<RoleRow>, RoleError> {
    let roles = sqlx::query_as::<_, RoleRow>(
        r"
        SELECT id, guild_id, name, color, permissions, position, is_default, created_at
        FROM guild_roles
        WHERE guild_id = $1
        ORDER BY position ASC
        ",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    Ok(roles)
}

/// Acquire the per-guild advisory lock used to serialize role creation
/// (seed 57).
pub async fn lock_role_create(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<(), RoleError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 57))")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Count roles in a guild inside a transaction (used under the role-create
/// advisory lock).
pub async fn count_roles_tx(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<i64, RoleError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guild_roles WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Compute the next role position inside a transaction (higher = lower rank).
pub async fn next_role_position(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<i32, RoleError> {
    let max_position: i32 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), 0) FROM guild_roles WHERE guild_id = $1",
    )
    .bind(guild_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(max_position)
}

/// Insert a new role inside a transaction.
pub async fn insert_role(
    tx: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    guild_id: Uuid,
    name: &str,
    color: &Option<String>,
    permissions: i64,
    position: i32,
) -> Result<RoleRow, RoleError> {
    let role = sqlx::query_as::<_, RoleRow>(
        r"
        INSERT INTO guild_roles (id, guild_id, name, color, permissions, position)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, guild_id, name, color, permissions, position, is_default, created_at
        ",
    )
    .bind(role_id)
    .bind(guild_id)
    .bind(name)
    .bind(color)
    .bind(permissions)
    .bind(position)
    .fetch_one(&mut **tx)
    .await?;
    Ok(role)
}

/// Fetch a role's `(position, permissions, is_default)` triple.
pub async fn fetch_role_state(
    pool: &PgPool,
    role_id: Uuid,
    guild_id: Uuid,
) -> Result<Option<(i32, i64, bool)>, RoleError> {
    let row: Option<(i32, i64, bool)> = sqlx::query_as(
        "SELECT position, permissions, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Apply a partial update to a role and return the resulting row.
#[allow(clippy::too_many_arguments)]
pub async fn update_role(
    pool: &PgPool,
    role_id: Uuid,
    guild_id: Uuid,
    name: &Option<String>,
    color: &Option<String>,
    permissions: Option<i64>,
    position: Option<i32>,
) -> Result<RoleRow, RoleError> {
    let role = sqlx::query_as::<_, RoleRow>(
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
    .bind(name)
    .bind(color)
    .bind(permissions)
    .bind(position)
    .fetch_one(pool)
    .await?;
    Ok(role)
}

/// Delete a role.
pub async fn delete_role(pool: &PgPool, role_id: Uuid) -> Result<(), RoleError> {
    sqlx::query("DELETE FROM guild_roles WHERE id = $1")
        .bind(role_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch a role's `(position, is_default)` pair (used by delete/assign).
pub async fn fetch_role_position_and_default(
    pool: &PgPool,
    role_id: Uuid,
    guild_id: Uuid,
) -> Result<Option<(i32, bool)>, RoleError> {
    let row: Option<(i32, bool)> = sqlx::query_as(
        "SELECT position, is_default FROM guild_roles WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch only a role's position (used by remove-role).
pub async fn fetch_role_position(
    pool: &PgPool,
    role_id: Uuid,
    guild_id: Uuid,
) -> Result<Option<i32>, RoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT position FROM guild_roles WHERE id = $1 AND guild_id = $2")
            .bind(role_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(p,)| p))
}

// ============================================================================
// Role assignment
// ============================================================================

/// Check whether a user is a member of a guild (used by role assignment).
pub async fn is_guild_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, RoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// Assign a role to a member (idempotent — duplicate assignments are ignored).
pub async fn assign_role_to_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    assigned_by: Uuid,
) -> Result<(), RoleError> {
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
    .bind(assigned_by)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a role from a member. Returns the number of rows affected.
pub async fn remove_role_from_member(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> Result<u64, RoleError> {
    let result = sqlx::query(
        "DELETE FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
