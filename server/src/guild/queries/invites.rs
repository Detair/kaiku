//! Guild invite queries.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use super::super::error::GuildError;
use super::super::types::GuildInvite;

/// List active (non-expired) invites for a guild.
pub async fn list_active_invites(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<GuildInvite>, GuildError> {
    let invites = sqlx::query_as::<_, GuildInvite>(
        r"SELECT id, guild_id, code, created_by, expires_at, use_count, created_at
           FROM guild_invites
           WHERE guild_id = $1 AND (expires_at IS NULL OR expires_at > NOW())
           ORDER BY created_at DESC",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    Ok(invites)
}

/// Count active (non-expired) invites for a guild.
pub async fn count_active_invites(pool: &PgPool, guild_id: Uuid) -> Result<i64, GuildError> {
    let row: (i64,) = sqlx::query_as(
        r"SELECT COUNT(*) FROM guild_invites
           WHERE guild_id = $1 AND (expires_at IS NULL OR expires_at > NOW())",
    )
    .bind(guild_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Check whether an invite code is already in use.
pub async fn invite_code_exists(pool: &PgPool, code: &str) -> Result<bool, GuildError> {
    let row: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM guild_invites WHERE code = $1")
        .bind(code)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

/// Insert a new invite row.
pub async fn insert_invite(
    pool: &PgPool,
    guild_id: Uuid,
    code: &str,
    created_by: Uuid,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<GuildInvite, GuildError> {
    let invite = sqlx::query_as::<_, GuildInvite>(
        r"INSERT INTO guild_invites (guild_id, code, created_by, expires_at)
           VALUES ($1, $2, $3, $4)
           RETURNING id, guild_id, code, created_by, expires_at, use_count, created_at",
    )
    .bind(guild_id)
    .bind(code)
    .bind(created_by)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(invite)
}

/// Delete an invite by `(guild_id, code)`. Returns the number of rows
/// affected.
pub async fn delete_invite(pool: &PgPool, guild_id: Uuid, code: &str) -> Result<u64, GuildError> {
    let result = sqlx::query("DELETE FROM guild_invites WHERE guild_id = $1 AND code = $2")
        .bind(guild_id)
        .bind(code)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Look up an invite by code, filtering out expired entries.
pub async fn fetch_active_invite_by_code(
    pool: &PgPool,
    code: &str,
) -> Result<Option<GuildInvite>, GuildError> {
    let invite = sqlx::query_as::<_, GuildInvite>(
        r"SELECT id, guild_id, code, created_by, expires_at, use_count, created_at
           FROM guild_invites
           WHERE code = $1 AND (expires_at IS NULL OR expires_at > NOW())",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(invite)
}

/// Check whether a user has an active global ban.
pub async fn is_user_globally_banned(pool: &PgPool, user_id: Uuid) -> Result<bool, GuildError> {
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM global_bans WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(banned)
}

/// Acquire the per-guild advisory lock used to serialize member joins
/// (seed 53).
pub async fn lock_member_join(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 53))")
        .bind(guild_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Check whether a user is banned from a specific guild.
pub async fn is_user_banned_from_guild(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, GuildError> {
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM guild_bans WHERE guild_id = $1 AND user_id = $2 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(banned)
}

/// Check whether a user is already a guild member inside a transaction.
pub async fn is_guild_member_tx(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, GuildError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2)",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(exists)
}

/// Count members of a guild inside a transaction (used under the join
/// advisory lock).
pub async fn count_guild_members_tx(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
) -> Result<i64, GuildError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guild_members WHERE guild_id = $1")
        .bind(guild_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Insert a new guild member, ignoring duplicate `(guild_id, user_id)`. Returns
/// the number of rows affected.
pub async fn insert_member_idempotent(
    tx: &mut Transaction<'_, Postgres>,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<u64, GuildError> {
    let result = sqlx::query(
        // New joiners enter 'pending' when the guild has screening enabled,
        // else 'active' (existing default).
        "INSERT INTO guild_members (guild_id, user_id, membership_state)
         VALUES ($1, $2, CASE WHEN (SELECT screening_enabled FROM guilds WHERE id = $1)
                              THEN 'pending' ELSE 'active' END)
         ON CONFLICT DO NOTHING",
    )
    .bind(guild_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

/// Increment the `use_count` of an invite.
pub async fn increment_invite_use_count(
    tx: &mut Transaction<'_, Postgres>,
    invite_id: Uuid,
) -> Result<(), GuildError> {
    sqlx::query("UPDATE guild_invites SET use_count = use_count + 1 WHERE id = $1")
        .bind(invite_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Fetch a guild's display name.
pub async fn fetch_guild_name(pool: &PgPool, guild_id: Uuid) -> Result<String, GuildError> {
    let row: (String,) = sqlx::query_as("SELECT name FROM guilds WHERE id = $1")
        .bind(guild_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}
