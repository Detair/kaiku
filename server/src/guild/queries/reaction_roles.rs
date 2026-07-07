//! Queries for reaction-role bindings and reaction-time grant/revoke.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::reaction_roles::{ReactionRoleError, ReactionRoleResponse};

/// A binding row, in the column order used across this module.
type BindingRow = (
    Uuid,           // id
    Uuid,           // guild_id
    Uuid,           // channel_id
    Uuid,           // message_id
    String,         // emoji
    Uuid,           // role_id
    Option<String>, // group_key
    String,         // mode
    DateTime<Utc>,  // created_at
);

fn to_response(r: BindingRow) -> ReactionRoleResponse {
    ReactionRoleResponse {
        id: r.0,
        guild_id: r.1,
        channel_id: r.2,
        message_id: r.3,
        emoji: r.4,
        role_id: r.5,
        group_key: r.6,
        mode: r.7,
        created_at: r.8,
    }
}

/// Insert a binding. Returns the created row. Unique-violation on
/// (`message_id`, emoji) surfaces as `sqlx::Error` for the handler to map.
#[allow(clippy::too_many_arguments)]
pub async fn insert_binding(
    pool: &PgPool,
    guild_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    emoji: &str,
    role_id: Uuid,
    group_key: Option<&str>,
    mode: &str,
    created_by: Uuid,
) -> Result<ReactionRoleResponse, ReactionRoleError> {
    let row: BindingRow = sqlx::query_as(
        "INSERT INTO reaction_role_bindings
             (guild_id, channel_id, message_id, emoji, role_id, group_key, mode, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, guild_id, channel_id, message_id, emoji, role_id, group_key, mode, created_at",
    )
    .bind(guild_id)
    .bind(channel_id)
    .bind(message_id)
    .bind(emoji)
    .bind(role_id)
    .bind(group_key)
    .bind(mode)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(to_response(row))
}

/// List bindings for a guild, optionally filtered to one message.
pub async fn list_bindings(
    pool: &PgPool,
    guild_id: Uuid,
    message_id: Option<Uuid>,
) -> Result<Vec<ReactionRoleResponse>, ReactionRoleError> {
    let rows: Vec<BindingRow> = sqlx::query_as(
        "SELECT id, guild_id, channel_id, message_id, emoji, role_id, group_key, mode, created_at
         FROM reaction_role_bindings
         WHERE guild_id = $1 AND ($2::uuid IS NULL OR message_id = $2)
         ORDER BY created_at ASC",
    )
    .bind(guild_id)
    .bind(message_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(to_response).collect())
}

/// Delete a binding scoped to its guild. Returns rows affected.
pub async fn delete_binding(
    pool: &PgPool,
    guild_id: Uuid,
    binding_id: Uuid,
) -> Result<u64, ReactionRoleError> {
    let res = sqlx::query("DELETE FROM reaction_role_bindings WHERE id = $1 AND guild_id = $2")
        .bind(binding_id)
        .bind(guild_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// A binding as needed by the reaction-time hook.
pub struct HookBinding {
    pub role_id: Uuid,
    pub mode: String,
    pub group_key: Option<String>,
}

/// Look up the binding for (`message_id`, emoji), if any.
pub async fn find_binding_for_reaction(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    emoji: &str,
) -> Result<Option<HookBinding>, ReactionRoleError> {
    let row: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "SELECT role_id, mode, group_key FROM reaction_role_bindings
         WHERE message_id = $1 AND emoji = $2",
    )
    .bind(message_id)
    .bind(emoji)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(role_id, mode, group_key)| HookBinding {
        role_id,
        mode,
        group_key,
    }))
}

/// Sibling bindings in the same group on the same message (excludes `emoji`).
/// Returns (emoji, `role_id`) pairs for the `unique` swap.
pub async fn sibling_bindings_in_group(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    group_key: &str,
    exclude_emoji: &str,
) -> Result<Vec<(String, Uuid)>, ReactionRoleError> {
    let rows: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT emoji, role_id FROM reaction_role_bindings
         WHERE message_id = $1 AND group_key = $2 AND emoji <> $3",
    )
    .bind(message_id)
    .bind(group_key)
    .bind(exclude_emoji)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Grant a role to a member inside the transaction (idempotent).
pub async fn tx_assign_role(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    assigned_by: Uuid,
) -> Result<(), ReactionRoleError> {
    sqlx::query(
        "INSERT INTO guild_member_roles (guild_id, user_id, role_id, assigned_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (guild_id, user_id, role_id) DO NOTHING",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .bind(assigned_by)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Revoke a role from a member inside the transaction.
pub async fn tx_remove_role(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> Result<(), ReactionRoleError> {
    sqlx::query("DELETE FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2 AND role_id = $3")
        .bind(guild_id)
        .bind(user_id)
        .bind(role_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Remove a user's stored reaction for an emoji inside the transaction
/// (used to clear the losing reaction on a `unique` swap).
pub async fn tx_remove_user_reaction(
    tx: &mut Transaction<'_, Postgres>,
    message_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<(), ReactionRoleError> {
    sqlx::query("DELETE FROM message_reactions WHERE message_id = $1 AND user_id = $2 AND emoji = $3")
        .bind(message_id)
        .bind(user_id)
        .bind(emoji)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The member's full set of role IDs (for the `MemberRolesUpdated` payload).
pub async fn tx_member_role_ids(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<Vec<Uuid>, ReactionRoleError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT role_id FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|(r,)| r).collect())
}

/// Whether the user is a member of the guild (transaction variant, hook guard).
pub async fn is_member(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, ReactionRoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2")
            .bind(guild_id)
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(row.is_some())
}

/// Fetch a role's (position, permissions, `is_default`) for the creation guard.
pub async fn fetch_role_guard_state(
    pool: &PgPool,
    guild_id: Uuid,
    role_id: Uuid,
) -> Result<Option<(i32, i64, bool)>, ReactionRoleError> {
    let row: Option<(i32, i64, bool)> = sqlx::query_as(
        "SELECT position, permissions, is_default FROM guild_roles
         WHERE id = $1 AND guild_id = $2",
    )
    .bind(role_id)
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Whether a message exists in the given channel (creation-time validation).
pub async fn message_in_channel(
    pool: &PgPool,
    message_id: Uuid,
    channel_id: Uuid,
) -> Result<bool, ReactionRoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM messages WHERE id = $1 AND channel_id = $2")
            .bind(message_id)
            .bind(channel_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

/// Whether a channel belongs to the guild (creation-time validation).
pub async fn channel_in_guild(
    pool: &PgPool,
    channel_id: Uuid,
    guild_id: Uuid,
) -> Result<bool, ReactionRoleError> {
    let row: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM channels WHERE id = $1 AND guild_id = $2")
            .bind(channel_id)
            .bind(guild_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}
