//! Database Queries
//!
//! Runtime queries (no compile-time `DATABASE_URL` required).
//!
//! All query functions include error context logging to aid debugging.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, QueryBuilder, Row};
use tracing::error;
use uuid::Uuid;

use super::models::{
    AuthMethodsConfig, Channel, ChannelMember, ChannelType, ChannelUnread, FileAttachment,
    GuildUnreadSummary, Message, MfaBackupCode, OidcProviderRow, PasswordResetToken, Session,
    UnreadAggregate, User,
};

/// Log and return a database error with context.
///
/// This helper ensures all database errors are logged with relevant context
/// before being propagated, making production debugging easier.
macro_rules! db_error {
    ($query:expr, $($field:tt)*) => {
        |e| {
            error!(query = $query, $($field)*, error = %e, "Database query failed");
            e
        }
    };
}

// ============================================================================
// User Queries
// ============================================================================

/// Find user by ID.
pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_error!("find_user_by_id", user_id = %id))
}

/// Find user by username.
pub async fn find_user_by_username(pool: &PgPool, username: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
        .map_err(db_error!("find_user_by_username", username = %username))
}

/// Find user by external ID (for OIDC).
pub async fn find_user_by_external_id(
    pool: &PgPool,
    external_id: &str,
) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE external_id = $1")
        .bind(external_id)
        .fetch_optional(pool)
        .await
        .map_err(db_error!("find_user_by_external_id", external_id = %external_id))
}

/// Find user by email.
pub async fn find_user_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(db_error!("find_user_by_email", email = %email))
}

/// Find multiple users by IDs (bulk lookup to avoid N+1 queries).
pub async fn find_users_by_ids(pool: &PgPool, ids: &[Uuid]) -> sqlx::Result<Vec<User>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(pool)
        .await
}

/// Check if username exists.
pub async fn username_exists(pool: &PgPool, username: &str) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
        .bind(username)
        .fetch_one(pool)
        .await?;

    Ok(result.0)
}

/// Check if email exists.
pub async fn email_exists(pool: &PgPool, email: &str) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
        .bind(email)
        .fetch_one(pool)
        .await?;

    Ok(result.0)
}

/// Create a new local user.
pub async fn create_user(
    pool: &PgPool,
    username: &str,
    display_name: &str,
    email: Option<&str>,
    password_hash: &str,
) -> sqlx::Result<User> {
    sqlx::query_as::<_, User>(
        r"
        INSERT INTO users (username, display_name, email, password_hash, auth_method)
        VALUES ($1, $2, $3, $4, 'local')
        RETURNING *
        ",
    )
    .bind(username)
    .bind(display_name)
    .bind(email)
    .bind(password_hash)
    .fetch_one(pool)
    .await
}

/// Update user's avatar URL.
pub async fn update_user_avatar(
    pool: &PgPool,
    user_id: Uuid,
    avatar_url: Option<&str>,
) -> sqlx::Result<User> {
    sqlx::query_as::<_, User>(
        "UPDATE users SET avatar_url = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
    )
    .bind(avatar_url)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Update user's profile (`display_name`, email).
///
/// Only non-None values are updated.
pub async fn update_user_profile(
    pool: &PgPool,
    user_id: Uuid,
    display_name: Option<&str>,
    email: Option<Option<&str>>, // Some(Some(email)) = set, Some(None) = clear, None = no change
) -> sqlx::Result<User> {
    let mut builder = QueryBuilder::new("UPDATE users SET updated_at = NOW()");

    if let Some(name) = display_name {
        builder.push(", display_name = ").push_bind(name);
    }
    if let Some(mail) = email {
        builder.push(", email = ").push_bind(mail);
    }

    builder
        .push(" WHERE id = ")
        .push_bind(user_id)
        .push(" RETURNING *");

    builder.build_query_as::<User>().fetch_one(pool).await
}

/// Get list of guild IDs the user is a member of.
pub async fn get_user_guild_ids(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<Uuid>> {
    let guild_ids =
        sqlx::query_scalar::<_, Uuid>("SELECT guild_id FROM guild_members WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(pool)
            .await?;

    Ok(guild_ids)
}

/// Update user's MFA secret.
pub async fn set_mfa_secret(
    pool: &PgPool,
    user_id: Uuid,
    mfa_secret: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET mfa_secret = $1, updated_at = NOW() WHERE id = $2")
        .bind(mfa_secret)
        .bind(user_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ============================================================================
// Session Queries
// ============================================================================

/// Create a new session (for refresh token tracking).
pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
    ip_address: Option<&str>,
    user_agent: Option<&str>,
) -> sqlx::Result<Session> {
    sqlx::query_as::<_, Session>(
        r"
        INSERT INTO sessions (user_id, token_hash, expires_at, ip_address, user_agent)
        VALUES ($1, $2, $3, $4::inet, $5)
        RETURNING id, user_id, token_hash, expires_at, host(ip_address) as ip_address, user_agent, created_at
        ",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .bind(ip_address)
    .bind(user_agent)
    .fetch_one(pool)
    .await
    .map_err(db_error!("create_session", user_id = %user_id))
}

/// Find session by token hash.
pub async fn find_session_by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> sqlx::Result<Option<Session>> {
    sqlx::query_as::<_, Session>(
        r"
        SELECT id, user_id, token_hash, expires_at, host(ip_address) as ip_address, user_agent, created_at
        FROM sessions
        WHERE token_hash = $1 AND expires_at > NOW()
        ",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        error!(query = "find_session_by_token_hash", error = %e, "Database query failed");
        e
    })
}

/// Delete a session by ID.
pub async fn delete_session(pool: &PgPool, session_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await
        .map_err(db_error!("delete_session", session_id = %session_id))?;
    Ok(())
}

/// Delete a session by token hash.
pub async fn delete_session_by_token_hash(pool: &PgPool, token_hash: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await
        .map_err(|e| {
            error!(query = "delete_session_by_token_hash", error = %e, "Database query failed");
            e
        })?;
    Ok(())
}

/// Delete all sessions for a user (logout everywhere).
pub async fn delete_all_user_sessions(pool: &PgPool, user_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(db_error!("delete_all_user_sessions", user_id = %user_id))?;
    Ok(result.rows_affected())
}

/// Clean up expired sessions (for background job).
pub async fn cleanup_expired_sessions(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at < NOW()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Clean up claimed prekeys older than 7 days (for background job).
///
/// Prekeys are one-time use keys for establishing E2EE sessions.
/// Once claimed, they're no longer needed and can be cleaned up after a retention period.
pub async fn cleanup_claimed_prekeys(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM prekeys WHERE claimed_at IS NOT NULL AND claimed_at < NOW() - INTERVAL '7 days'",
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Clean up expired device transfers (for background job).
///
/// Device transfers have a 5-minute TTL for security. This removes expired entries.
pub async fn cleanup_expired_device_transfers(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM device_transfers WHERE expires_at < NOW()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Password Reset Token Queries
// ============================================================================

/// Create a password reset token.
pub async fn create_password_reset_token(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> sqlx::Result<PasswordResetToken> {
    sqlx::query_as::<_, PasswordResetToken>(
        r"
        INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
        VALUES ($1, $2, $3)
        RETURNING *
        ",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await
    .map_err(db_error!("create_password_reset_token", user_id = %user_id))
}

/// Find a valid (unused, non-expired) password reset token by its hash.
pub async fn find_valid_reset_token(
    pool: &PgPool,
    token_hash: &str,
) -> sqlx::Result<Option<PasswordResetToken>> {
    sqlx::query_as::<_, PasswordResetToken>(
        "SELECT * FROM password_reset_tokens WHERE token_hash = $1 AND used_at IS NULL AND expires_at > NOW()",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        error!(query = "find_valid_reset_token", error = %e, "Database query failed");
        e
    })
}

/// Mark a password reset token as used.
pub async fn mark_reset_token_used(pool: &PgPool, token_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(pool)
        .await
        .map_err(db_error!("mark_reset_token_used", token_id = %token_id))?;
    Ok(())
}

/// Invalidate all unused password reset tokens for a user.
pub async fn invalidate_user_reset_tokens(pool: &PgPool, user_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE password_reset_tokens SET used_at = NOW() WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(db_error!("invalidate_user_reset_tokens", user_id = %user_id))?;
    Ok(result.rows_affected())
}

/// Clean up expired password reset tokens (for background job).
///
/// Removes tokens that expired more than 24 hours ago.
pub async fn cleanup_expired_reset_tokens(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM password_reset_tokens WHERE expires_at < NOW() - INTERVAL '24 hours'",
    )
    .execute(pool)
    .await
    .map_err(|e| {
        error!(query = "cleanup_expired_reset_tokens", error = %e, "Database query failed");
        e
    })?;
    Ok(result.rows_affected())
}

// ============================================================================
// MFA Backup Code Queries
// ============================================================================

/// Insert a batch of MFA backup code hashes for a user.
///
/// Replaces any existing codes by deleting first, then inserting the new batch.
/// All 10 codes are inserted in a single transaction.
pub async fn store_mfa_backup_codes(
    pool: &PgPool,
    user_id: Uuid,
    code_hashes: &[String],
) -> sqlx::Result<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(db_error!("store_mfa_backup_codes", user_id = %user_id))?;

    // Delete existing codes for this user
    sqlx::query("DELETE FROM mfa_backup_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(db_error!("store_mfa_backup_codes/delete", user_id = %user_id))?;

    // Insert new codes
    for code_hash in code_hashes {
        sqlx::query("INSERT INTO mfa_backup_codes (user_id, code_hash) VALUES ($1, $2)")
            .bind(user_id)
            .bind(code_hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error!("store_mfa_backup_codes/insert", user_id = %user_id))?;
    }

    tx.commit()
        .await
        .map_err(db_error!("store_mfa_backup_codes/commit", user_id = %user_id))?;

    Ok(())
}

/// Fetch all unused MFA backup codes for a user.
pub async fn get_unused_mfa_backup_codes(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<MfaBackupCode>> {
    sqlx::query_as::<_, MfaBackupCode>(
        "SELECT * FROM mfa_backup_codes WHERE user_id = $1 AND used_at IS NULL ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(db_error!("get_unused_mfa_backup_codes", user_id = %user_id))
}

/// Mark an MFA backup code as used by its ID.
pub async fn mark_mfa_backup_code_used(pool: &PgPool, code_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE mfa_backup_codes SET used_at = NOW() WHERE id = $1")
        .bind(code_id)
        .execute(pool)
        .await
        .map_err(db_error!("mark_mfa_backup_code_used", code_id = %code_id))?;
    Ok(())
}

/// Count unused MFA backup codes for a user.
pub async fn count_unused_mfa_backup_codes(pool: &PgPool, user_id: Uuid) -> sqlx::Result<i64> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM mfa_backup_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(db_error!("count_unused_mfa_backup_codes", user_id = %user_id))?;
    Ok(count)
}

/// Count all MFA backup codes for a user (used and unused).
/// Used to detect whether backup codes were ever generated (first-time setup detection).
pub async fn count_all_mfa_backup_codes(pool: &PgPool, user_id: Uuid) -> sqlx::Result<i64> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM mfa_backup_codes WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .map_err(db_error!("count_all_mfa_backup_codes", user_id = %user_id))?;
    Ok(count)
}

/// Delete all MFA backup codes for a user (e.g. when MFA is disabled).
pub async fn delete_mfa_backup_codes(pool: &PgPool, user_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM mfa_backup_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .map_err(db_error!("delete_mfa_backup_codes", user_id = %user_id))?;
    Ok(result.rows_affected())
}

// ============================================================================
// Channel Queries
// ============================================================================

/// Find channel by ID.
pub async fn find_channel_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        SELECT id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at
        FROM channels
        WHERE id = $1
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_error!("find_channel_by_id", channel_id = %id))
}

/// Parameters for creating a new channel.
pub struct CreateChannelParams<'a> {
    pub name: &'a str,
    pub channel_type: &'a ChannelType,
    pub category_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub topic: Option<&'a str>,
    pub icon_url: Option<&'a str>,
    pub user_limit: Option<i32>,
}

/// Create a new channel.
pub async fn create_channel(
    pool: &PgPool,
    params: CreateChannelParams<'_>,
) -> sqlx::Result<Channel> {
    // Get the next position for this category
    let row = sqlx::query(
        "SELECT COALESCE(MAX(position), 0) + 1 as next_pos FROM channels WHERE category_id IS NOT DISTINCT FROM $1",
    )
    .bind(params.category_id)
    .fetch_one(pool)
    .await?;

    let position: i32 = row.get("next_pos");

    sqlx::query_as::<_, Channel>(
        r"
        INSERT INTO channels (name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at
        ",
    )
    .bind(params.name)
    .bind(params.channel_type)
    .bind(params.category_id)
    .bind(params.guild_id)
    .bind(params.topic)
    .bind(params.icon_url)
    .bind(params.user_limit)
    .bind(position)
    .fetch_one(pool)
    .await
}

/// Update a channel.
pub async fn update_channel(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    topic: Option<&str>,
    icon_url: Option<&str>,
    user_limit: Option<i32>,
    position: Option<i32>,
) -> sqlx::Result<Option<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        UPDATE channels
        SET name = COALESCE($2, name),
            topic = COALESCE($3, topic),
            icon_url = COALESCE($4, icon_url),
            user_limit = COALESCE($5, user_limit),
            position = COALESCE($6, position),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at
        ",
    )
    .bind(id)
    .bind(name)
    .bind(topic)
    .bind(icon_url)
    .bind(user_limit)
    .bind(position)
    .fetch_optional(pool)
    .await
}

/// Delete a channel.
pub async fn delete_channel(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Get a channel by ID (alias for `find_channel_by_id`).
///
/// This is a convenience wrapper for use in permission checks.
pub async fn get_channel_by_id(pool: &PgPool, channel_id: Uuid) -> sqlx::Result<Option<Channel>> {
    find_channel_by_id(pool, channel_id).await
}

/// Check if a user is a participant in a DM channel.
pub async fn is_dm_participant(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<bool> {
    let result: Option<(bool,)> = sqlx::query_as(
        r"
        SELECT EXISTS(
            SELECT 1
            FROM dm_participants
            WHERE channel_id = $1 AND user_id = $2
        )
        ",
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(result.map(|(exists,)| exists).unwrap_or(false))
}

/// Get all permission overrides for a channel.
pub async fn get_channel_overrides(
    pool: &PgPool,
    channel_id: Uuid,
) -> sqlx::Result<Vec<crate::permissions::models::ChannelOverride>> {
    sqlx::query_as::<_, crate::permissions::models::ChannelOverride>(
        r"
        SELECT id, channel_id, role_id, allow_permissions, deny_permissions
        FROM channel_overrides
        WHERE channel_id = $1
        ",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await
}

/// Get all permission overrides for multiple channels in a single query.
pub async fn get_channel_overrides_batch(
    pool: &PgPool,
    channel_ids: &[Uuid],
) -> sqlx::Result<Vec<crate::permissions::models::ChannelOverride>> {
    if channel_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, crate::permissions::models::ChannelOverride>(
        r"
        SELECT id, channel_id, role_id, allow_permissions, deny_permissions
        FROM channel_overrides
        WHERE channel_id = ANY($1)
        ",
    )
    .bind(channel_ids)
    .fetch_all(pool)
    .await
}

// ============================================================================
// Channel Member Queries
// ============================================================================

/// List members of a channel.
pub async fn list_channel_members(
    pool: &PgPool,
    channel_id: Uuid,
) -> sqlx::Result<Vec<ChannelMember>> {
    sqlx::query_as::<_, ChannelMember>(
        "SELECT * FROM channel_members WHERE channel_id = $1 ORDER BY joined_at ASC",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await
}

/// List members with user details.
pub async fn list_channel_members_with_users(
    pool: &PgPool,
    channel_id: Uuid,
) -> sqlx::Result<Vec<User>> {
    sqlx::query_as::<_, User>(
        r"
        SELECT u.*
        FROM users u
        INNER JOIN channel_members cm ON cm.user_id = u.id
        WHERE cm.channel_id = $1
        ORDER BY cm.joined_at ASC
        ",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await
}

/// Check if a user is a member of a channel.
pub async fn is_channel_member(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM channel_members WHERE channel_id = $1 AND user_id = $2)",
    )
    .bind(channel_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}

/// Add a member to a channel.
pub async fn add_channel_member(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    role_id: Option<Uuid>,
) -> sqlx::Result<ChannelMember> {
    sqlx::query_as::<_, ChannelMember>(
        r"
        INSERT INTO channel_members (channel_id, user_id, role_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (channel_id, user_id) DO NOTHING
        RETURNING channel_id, user_id, role_id, joined_at
        ",
    )
    .bind(channel_id)
    .bind(user_id)
    .bind(role_id)
    .fetch_one(pool)
    .await
}

/// Remove a member from a channel.
pub async fn remove_channel_member(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM channel_members WHERE channel_id = $1 AND user_id = $2")
        .bind(channel_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Message Queries
// ============================================================================

/// List messages in a channel with pagination.
pub async fn list_messages(
    pool: &PgPool,
    channel_id: Uuid,
    before: Option<Uuid>,
    limit: i64,
) -> sqlx::Result<Vec<Message>> {
    if let Some(before_id) = before {
        // Use (created_at, id) tuple comparison for correct cursor pagination
        // This works with UUIDv4 (random) since we compare by timestamp first
        sqlx::query_as::<_, Message>(
            r"
            SELECT m.* FROM messages m
            WHERE m.channel_id = $1
              AND m.deleted_at IS NULL
              AND m.parent_id IS NULL
              AND (m.created_at, m.id) < (
                SELECT created_at, id FROM messages WHERE id = $2
              )
            ORDER BY m.created_at DESC, m.id DESC
            LIMIT $3
            ",
        )
        .bind(channel_id)
        .bind(before_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Message>(
            r"
            SELECT * FROM messages
            WHERE channel_id = $1
              AND deleted_at IS NULL
              AND parent_id IS NULL
            ORDER BY created_at DESC, id DESC
            LIMIT $2
            ",
        )
        .bind(channel_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

/// Find message by ID.
pub async fn find_message_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Message>> {
    sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_error!("find_message_by_id", message_id = %id))
}

/// Create a new message.
pub async fn create_message(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
    encrypted: bool,
    nonce: Option<&str>,
    reply_to: Option<Uuid>,
) -> sqlx::Result<Message> {
    sqlx::query_as::<_, Message>(
        r"
        INSERT INTO messages (channel_id, user_id, content, encrypted, nonce, reply_to)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        ",
    )
    .bind(channel_id)
    .bind(user_id)
    .bind(content)
    .bind(encrypted)
    .bind(nonce)
    .bind(reply_to)
    .fetch_one(pool)
    .await
}

/// Update a message (edit).
pub async fn update_message(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    content: &str,
) -> sqlx::Result<Option<Message>> {
    sqlx::query_as::<_, Message>(
        r"
        UPDATE messages
        SET content = $3, edited_at = NOW()
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        RETURNING *
        ",
    )
    .bind(id)
    .bind(user_id)
    .bind(content)
    .fetch_optional(pool)
    .await
}

/// Soft delete a message.
pub async fn delete_message(pool: &PgPool, id: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r"
        UPDATE messages
        SET deleted_at = NOW(), content = '[deleted]'
        WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
        ",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Admin delete a message (ignores `user_id` check).
/// If the message is a thread reply, also decrements the parent's thread counters.
pub async fn admin_delete_message(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
    // Fetch parent_id before deletion so we can update thread counters
    let parent_id: Option<Uuid> =
        sqlx::query_scalar("SELECT parent_id FROM messages WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .flatten();

    let result = sqlx::query(
        r"
        UPDATE messages
        SET deleted_at = NOW(), content = '[deleted]'
        WHERE id = $1 AND deleted_at IS NULL
        ",
    )
    .bind(id)
    .execute(pool)
    .await?;

    let deleted = result.rows_affected() > 0;

    // Decrement parent thread counters if this was a thread reply
    if deleted {
        if let Some(parent_id) = parent_id {
            let _ = decrement_thread_counters(pool, parent_id).await;
        }
    }

    Ok(deleted)
}

// ============================================================================
// Thread Queries
// ============================================================================

/// List thread replies for a parent message (chronological, oldest first).
pub async fn list_thread_replies(
    pool: &PgPool,
    parent_id: Uuid,
    after: Option<Uuid>,
    limit: i64,
) -> sqlx::Result<Vec<Message>> {
    if let Some(after_id) = after {
        sqlx::query_as::<_, Message>(
            r"
            SELECT m.* FROM messages m
            WHERE m.parent_id = $1
              AND m.deleted_at IS NULL
              AND (m.created_at, m.id) > (
                SELECT created_at, id FROM messages WHERE id = $2
              )
            ORDER BY m.created_at ASC, m.id ASC
            LIMIT $3
            ",
        )
        .bind(parent_id)
        .bind(after_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, Message>(
            r"
            SELECT * FROM messages
            WHERE parent_id = $1
              AND deleted_at IS NULL
            ORDER BY created_at ASC, id ASC
            LIMIT $2
            ",
        )
        .bind(parent_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

/// Parameters for creating a thread reply.
pub struct CreateThreadReplyParams<'a> {
    pub parent_id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub content: &'a str,
    pub encrypted: bool,
    pub nonce: Option<&'a str>,
    pub reply_to: Option<Uuid>,
}

/// Create a thread reply atomically: insert reply + update parent counters.
pub async fn create_thread_reply(
    pool: &PgPool,
    params: CreateThreadReplyParams<'_>,
) -> sqlx::Result<Message> {
    let mut tx = pool.begin().await?;

    let message = sqlx::query_as::<_, Message>(
        r"
        INSERT INTO messages (channel_id, user_id, content, encrypted, nonce, reply_to, parent_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        ",
    )
    .bind(params.channel_id)
    .bind(params.user_id)
    .bind(params.content)
    .bind(params.encrypted)
    .bind(params.nonce)
    .bind(params.reply_to)
    .bind(params.parent_id)
    .fetch_one(&mut *tx)
    .await?;

    // Update parent message counters
    sqlx::query(
        r"
        UPDATE messages
        SET thread_reply_count = thread_reply_count + 1,
            thread_last_reply_at = $2
        WHERE id = $1
        ",
    )
    .bind(params.parent_id)
    .bind(message.created_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(message)
}

/// Decrement thread counters on parent after a reply is deleted.
pub async fn decrement_thread_counters(pool: &PgPool, parent_id: Uuid) -> sqlx::Result<()> {
    // Decrement count and recalculate last_reply_at from remaining replies
    sqlx::query(
        r"
        UPDATE messages
        SET thread_reply_count = GREATEST(thread_reply_count - 1, 0),
            thread_last_reply_at = (
                SELECT MAX(created_at) FROM messages
                WHERE parent_id = $1 AND deleted_at IS NULL
            )
        WHERE id = $1
        ",
    )
    .bind(parent_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get distinct participant `user_ids` for a thread.
pub async fn get_thread_participants(
    pool: &PgPool,
    parent_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        r"
        SELECT DISTINCT user_id FROM messages
        WHERE parent_id = $1 AND deleted_at IS NULL
        ORDER BY user_id
        LIMIT $2
        ",
    )
    .bind(parent_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Batch-fetch distinct participant user IDs for multiple threads.
///
/// Returns a map of `parent_id -> Vec<user_id>` with up to `limit_per_thread` participants each.
/// Uses a window function to avoid N+1 queries.
pub async fn get_batch_thread_participants(
    pool: &PgPool,
    parent_ids: &[Uuid],
    limit_per_thread: i64,
) -> sqlx::Result<HashMap<Uuid, Vec<Uuid>>> {
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        r"
        SELECT parent_id, user_id
        FROM (
            SELECT
                parent_id,
                user_id,
                ROW_NUMBER() OVER (PARTITION BY parent_id ORDER BY MIN(created_at) DESC) as rn
            FROM messages
            WHERE parent_id = ANY($1) AND deleted_at IS NULL
            GROUP BY parent_id, user_id
        ) ranked
        WHERE rn <= $2
        ORDER BY parent_id, rn
        ",
    )
    .bind(parent_ids)
    .bind(limit_per_thread)
    .fetch_all(pool)
    .await?;

    let mut result: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in rows {
        let parent_id: Uuid = row.get("parent_id");
        let user_id: Uuid = row.get("user_id");
        result.entry(parent_id).or_default().push(user_id);
    }

    Ok(result)
}

/// Batch-fetch thread read states for a user across multiple threads.
///
/// Returns `{ thread_parent_id: last_read_message_id }`.
pub async fn get_batch_thread_read_states(
    pool: &PgPool,
    user_id: Uuid,
    thread_parent_ids: &[Uuid],
) -> sqlx::Result<HashMap<Uuid, Option<Uuid>>> {
    if thread_parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        r"
        SELECT thread_parent_id, last_read_message_id
        FROM thread_read_state
        WHERE user_id = $1 AND thread_parent_id = ANY($2)
        ",
    )
    .bind(user_id)
    .bind(thread_parent_ids)
    .fetch_all(pool)
    .await?;

    let mut result: HashMap<Uuid, Option<Uuid>> = HashMap::new();
    for row in rows {
        let thread_parent_id: Uuid = row.get("thread_parent_id");
        let last_read_message_id: Option<Uuid> = row.get("last_read_message_id");
        result.insert(thread_parent_id, last_read_message_id);
    }

    Ok(result)
}

/// Batch-fetch the latest reply message ID for each thread.
///
/// Returns `{ parent_id: latest_reply_id }`.
pub async fn get_batch_thread_latest_reply_ids(
    pool: &PgPool,
    parent_ids: &[Uuid],
) -> sqlx::Result<HashMap<Uuid, Uuid>> {
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        r"
        SELECT DISTINCT ON (parent_id) parent_id, id as latest_reply_id
        FROM messages
        WHERE parent_id = ANY($1) AND deleted_at IS NULL
        ORDER BY parent_id, created_at DESC, id DESC
        ",
    )
    .bind(parent_ids)
    .fetch_all(pool)
    .await?;

    let mut result: HashMap<Uuid, Uuid> = HashMap::new();
    for row in rows {
        let parent_id: Uuid = row.get("parent_id");
        let latest_reply_id: Uuid = row.get("latest_reply_id");
        result.insert(parent_id, latest_reply_id);
    }

    Ok(result)
}

/// Upsert thread read position for a user.
pub async fn update_thread_read_state(
    pool: &PgPool,
    user_id: Uuid,
    thread_parent_id: Uuid,
    last_read_message_id: Option<Uuid>,
) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO thread_read_state (user_id, thread_parent_id, last_read_at, last_read_message_id)
        VALUES ($1, $2, NOW(), $3)
        ON CONFLICT (user_id, thread_parent_id)
        DO UPDATE SET last_read_at = NOW(), last_read_message_id = $3
        ",
    )
    .bind(user_id)
    .bind(thread_parent_id)
    .bind(last_read_message_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Search messages within specific channels using `PostgreSQL` full-text search.
/// Uses `websearch_to_tsquery` for user-friendly query syntax (supports AND, OR, quotes).
///
/// **Security:** Only searches in channels the user has access to (provided as `channel_ids`).
pub async fn search_messages_in_channels(
    pool: &PgPool,
    channel_ids: &[Uuid],
    query: &str,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<Message>> {
    if channel_ids.is_empty() {
        return Ok(Vec::new());
    }

    sqlx::query_as::<_, Message>(
        r"
        SELECT m.*
        FROM messages m
        WHERE m.channel_id = ANY($1)
          AND m.deleted_at IS NULL
          AND m.encrypted = false
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ORDER BY m.created_at DESC
        LIMIT $3 OFFSET $4
        ",
    )
    .bind(channel_ids)
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Count total search results in specific channels for pagination.
///
/// **Security:** Only counts messages in channels the user has access to (provided as
/// `channel_ids`).
pub async fn count_search_messages_in_channels(
    pool: &PgPool,
    channel_ids: &[Uuid],
    query: &str,
) -> sqlx::Result<i64> {
    if channel_ids.is_empty() {
        return Ok(0);
    }

    let result: (i64,) = sqlx::query_as(
        r"
        SELECT COUNT(*)
        FROM messages m
        WHERE m.channel_id = ANY($1)
          AND m.deleted_at IS NULL
          AND m.encrypted = false
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ",
    )
    .bind(channel_ids)
    .bind(query)
    .fetch_one(pool)
    .await?;
    Ok(result.0)
}

// ============================================================================
// Advanced Search Queries
// ============================================================================

/// Sort order for search results.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SearchSort {
    /// Sort by relevance (`ts_rank`), then date.
    #[default]
    Relevance,
    /// Sort by date only (newest first).
    Date,
}

/// Advanced search filters for message search.
#[derive(Debug, Default)]
pub struct SearchFilters {
    /// Only messages created at or after this time.
    pub date_from: Option<DateTime<Utc>>,
    /// Only messages created at or before this time.
    pub date_to: Option<DateTime<Utc>>,
    /// Only messages by this author.
    pub author_id: Option<Uuid>,
    /// Only messages containing a URL.
    pub has_link: bool,
    /// Only messages with file attachments.
    pub has_file: bool,
    /// Sort order (relevance or date).
    pub sort: SearchSort,
}

/// Search result row with relevance rank and highlighted snippet.
#[derive(Debug, sqlx::FromRow)]
pub struct SearchMessageRow {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Option<Uuid>,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub rank: f32,
    pub headline: String,
}

/// Search messages with advanced filters using dynamic SQL.
///
/// Builds a query dynamically based on which filters are provided.
/// Always excludes encrypted and soft-deleted messages.
/// Returns rows with `ts_rank` relevance score and `ts_headline` snippet.
pub async fn search_messages_filtered(
    pool: &PgPool,
    channel_ids: &[Uuid],
    query: &str,
    filters: &SearchFilters,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<SearchMessageRow>> {
    if channel_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut builder = QueryBuilder::new(
        "SELECT m.id, m.channel_id, m.user_id, m.content, m.created_at, \
         ts_rank(m.content_search, websearch_to_tsquery('english', ",
    );
    builder.push_bind(query);
    builder.push(
        ")) AS rank, \
         ts_headline('english', m.content, websearch_to_tsquery('english', ",
    );
    builder.push_bind(query);
    builder.push(
        "), 'StartSel=<mark>, StopSel=</mark>, MaxWords=50, MinWords=20, MaxFragments=2') AS headline \
         FROM messages m",
    );

    if filters.has_file {
        builder.push(" INNER JOIN file_attachments fa ON fa.message_id = m.id");
    }

    builder.push(" WHERE m.channel_id = ANY(");
    builder.push_bind(channel_ids);
    builder.push(
        ") AND m.deleted_at IS NULL AND m.encrypted = false \
         AND m.content_search @@ websearch_to_tsquery('english', ",
    );
    builder.push_bind(query);
    builder.push(")");

    if let Some(date_from) = filters.date_from {
        builder.push(" AND m.created_at >= ").push_bind(date_from);
    }
    if let Some(date_to) = filters.date_to {
        builder.push(" AND m.created_at <= ").push_bind(date_to);
    }
    if let Some(author_id) = filters.author_id {
        builder.push(" AND m.user_id = ").push_bind(author_id);
    }
    if filters.has_link {
        builder.push(" AND m.content ~* 'https?://'");
    }
    // has_file is handled via the JOIN (ensures at least one attachment exists)

    match filters.sort {
        SearchSort::Relevance => builder.push(" ORDER BY rank DESC, m.created_at DESC"),
        SearchSort::Date => builder.push(" ORDER BY m.created_at DESC"),
    };

    builder
        .push(" LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    builder
        .build_query_as::<SearchMessageRow>()
        .fetch_all(pool)
        .await
}

/// Count search results with advanced filters using dynamic SQL.
pub async fn count_search_messages_filtered(
    pool: &PgPool,
    channel_ids: &[Uuid],
    query: &str,
    filters: &SearchFilters,
) -> sqlx::Result<i64> {
    if channel_ids.is_empty() {
        return Ok(0);
    }

    let mut builder = QueryBuilder::new("SELECT COUNT(*)");

    if filters.has_file {
        builder.push(" FROM messages m INNER JOIN file_attachments fa ON fa.message_id = m.id");
    } else {
        builder.push(" FROM messages m");
    }

    builder.push(" WHERE m.channel_id = ANY(");
    builder.push_bind(channel_ids);
    builder.push(
        ") AND m.deleted_at IS NULL AND m.encrypted = false \
         AND m.content_search @@ websearch_to_tsquery('english', ",
    );
    builder.push_bind(query);
    builder.push(")");

    if let Some(date_from) = filters.date_from {
        builder.push(" AND m.created_at >= ").push_bind(date_from);
    }
    if let Some(date_to) = filters.date_to {
        builder.push(" AND m.created_at <= ").push_bind(date_to);
    }
    if let Some(author_id) = filters.author_id {
        builder.push(" AND m.user_id = ").push_bind(author_id);
    }
    if filters.has_link {
        builder.push(" AND m.content ~* 'https?://'");
    }

    let (count,) = builder.build_query_as::<(i64,)>().fetch_one(pool).await?;
    Ok(count)
}

// ============================================================================
// File Attachment Queries
// ============================================================================

/// Create a new file attachment record with optional media metadata.
#[allow(clippy::too_many_arguments)]
pub async fn create_file_attachment(
    pool: &PgPool,
    message_id: Uuid,
    filename: &str,
    mime_type: &str,
    size_bytes: i64,
    s3_key: &str,
    width: Option<i32>,
    height: Option<i32>,
    blurhash: Option<&str>,
    thumbnail_s3_key: Option<&str>,
    medium_s3_key: Option<&str>,
    processing_status: &str,
) -> sqlx::Result<FileAttachment> {
    sqlx::query_as::<_, FileAttachment>(
        r"
        INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key,
                                      width, height, blurhash, thumbnail_s3_key, medium_s3_key,
                                      processing_status)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING *
        ",
    )
    .bind(message_id)
    .bind(filename)
    .bind(mime_type)
    .bind(size_bytes)
    .bind(s3_key)
    .bind(width)
    .bind(height)
    .bind(blurhash)
    .bind(thumbnail_s3_key)
    .bind(medium_s3_key)
    .bind(processing_status)
    .fetch_one(pool)
    .await
}

/// Find file attachment by ID.
pub async fn find_file_attachment_by_id(
    pool: &PgPool,
    id: Uuid,
) -> sqlx::Result<Option<FileAttachment>> {
    sqlx::query_as::<_, FileAttachment>("SELECT * FROM file_attachments WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// List file attachments for a message.
pub async fn list_file_attachments_by_message(
    pool: &PgPool,
    message_id: Uuid,
) -> sqlx::Result<Vec<FileAttachment>> {
    sqlx::query_as::<_, FileAttachment>(
        "SELECT * FROM file_attachments WHERE message_id = $1 ORDER BY created_at ASC",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await
}

/// List file attachments for multiple messages (bulk fetch to avoid N+1).
pub async fn list_file_attachments_by_messages(
    pool: &PgPool,
    message_ids: &[Uuid],
) -> sqlx::Result<Vec<FileAttachment>> {
    if message_ids.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_as::<_, FileAttachment>(
        "SELECT * FROM file_attachments WHERE message_id = ANY($1) ORDER BY created_at ASC",
    )
    .bind(message_ids)
    .fetch_all(pool)
    .await
}

/// Delete file attachment by ID, returning the deleted record.
pub async fn delete_file_attachment(
    pool: &PgPool,
    id: Uuid,
) -> sqlx::Result<Option<FileAttachment>> {
    sqlx::query_as::<_, FileAttachment>("DELETE FROM file_attachments WHERE id = $1 RETURNING *")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Delete all file attachments for a message, returning the deleted records.
pub async fn delete_file_attachments_by_message(
    pool: &PgPool,
    message_id: Uuid,
) -> sqlx::Result<Vec<FileAttachment>> {
    sqlx::query_as::<_, FileAttachment>(
        "DELETE FROM file_attachments WHERE message_id = $1 RETURNING *",
    )
    .bind(message_id)
    .fetch_all(pool)
    .await
}

/// Check if a user has access to an attachment.
///
/// Returns true only if the user has access to the channel containing the attachment:
/// - For guild channels: user must be a guild member
/// - For DM channels: user must be a DM participant
pub async fn check_attachment_access(
    pool: &PgPool,
    attachment_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as(
        r"
        SELECT EXISTS(
            SELECT 1
            FROM file_attachments fa
            JOIN messages m ON fa.message_id = m.id
            JOIN channels c ON m.channel_id = c.id
            WHERE fa.id = $1
              AND (
                -- Guild channel: user is guild member
                (c.guild_id IS NOT NULL AND EXISTS(
                    SELECT 1 FROM guild_members gm
                    WHERE gm.guild_id = c.guild_id AND gm.user_id = $2
                ))
                OR
                -- DM channel: user is participant
                (c.channel_type = 'dm' AND EXISTS(
                    SELECT 1 FROM dm_participants dp
                    WHERE dp.channel_id = c.id AND dp.user_id = $2
                ))
              )
        )
        ",
    )
    .bind(attachment_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}

// ============================================================================
// Guild Queries
// ============================================================================

/// Check if a user is a member of a guild.
pub async fn is_guild_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2)",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}

/// Get channels for a guild.
pub async fn get_guild_channels(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<Vec<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        SELECT id, name, channel_type, category_id, guild_id, topic, icon_url, user_limit, position, max_screen_shares, created_at, updated_at
        FROM channels
        WHERE guild_id = $1
        ORDER BY position ASC
        ",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

// ============================================================================
// Server Configuration
// ============================================================================

/// Get a server configuration value by key.
pub async fn get_config_value(pool: &PgPool, key: &str) -> sqlx::Result<serde_json::Value> {
    let row = sqlx::query("SELECT value FROM server_config WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                error_debug = ?e,
                config_key = %key,
                "Failed to get config value from database"
            );
            e
        })?;
    Ok(row.get("value"))
}

/// Set a server configuration value.
pub async fn set_config_value(
    pool: &PgPool,
    key: &str,
    value: serde_json::Value,
    updated_by: Uuid,
) -> sqlx::Result<()> {
    let result = sqlx::query(
        "UPDATE server_config SET value = $2, updated_by = $3, updated_at = NOW()
         WHERE key = $1",
    )
    .bind(key)
    .bind(value)
    .bind(updated_by)
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            error_debug = ?e,
            config_key = %key,
            updated_by = %updated_by,
            "Failed to execute config value update query"
        );
        e
    })?;

    if result.rows_affected() == 0 {
        tracing::error!(
            key = %key,
            "Failed to update config value - key does not exist in server_config table"
        );
        return Err(sqlx::Error::RowNotFound);
    }

    Ok(())
}

/// Check if server setup is complete.
pub async fn is_setup_complete(pool: &PgPool) -> sqlx::Result<bool> {
    let value = get_config_value(pool, "setup_complete")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                "Failed to query setup_complete status from database"
            );
            e
        })?;

    if let Some(b) = value.as_bool() {
        Ok(b)
    } else {
        tracing::warn!(
            actual_value = ?value,
            "setup_complete config value is not a boolean, defaulting to false"
        );
        Ok(false)
    }
}

/// Mark server setup as complete (irreversible).
pub async fn mark_setup_complete(pool: &PgPool, updated_by: Uuid) -> sqlx::Result<()> {
    set_config_value(pool, "setup_complete", serde_json::json!(true), updated_by).await
}

/// Count total number of users in the database.
/// Used in critical first-user registration path - errors must be logged for debugging.
pub async fn count_users(pool: &PgPool) -> sqlx::Result<i64> {
    let row = sqlx::query("SELECT COUNT(*) as count FROM users")
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                error_debug = ?e,
                "Failed to count users in database - this is critical for first-user registration"
            );
            e
        })?;
    Ok(row.get("count"))
}

// ============================================================================
// Unread Aggregation Queries
// ============================================================================

/// Get aggregate unread counts across all guilds and DMs for a user.
///
/// This query provides a centralized view of all unread activity:
/// - Guilds: Channels with unreads, grouped by guild
/// - DMs: Direct message conversations with unreads
///
/// Unread count is calculated by comparing message `created_at` with the user's
/// `last_read_at` from `channel_read_state`.
#[tracing::instrument(skip(pool))]
pub async fn get_unread_aggregate(pool: &PgPool, user_id: Uuid) -> sqlx::Result<UnreadAggregate> {
    // Get guild channel unreads
    let guild_rows = sqlx::query(
        r"
        SELECT
            g.id as guild_id,
            g.name as guild_name,
            c.id as channel_id,
            c.name as channel_name,
            COUNT(m.id)::bigint as unread_count
        FROM guild_members gm
        INNER JOIN guilds g ON g.id = gm.guild_id
        INNER JOIN channels c ON c.guild_id = g.id
        LEFT JOIN channel_read_state crs ON crs.channel_id = c.id AND crs.user_id = $1
        LEFT JOIN messages m ON m.channel_id = c.id
            AND m.deleted_at IS NULL
            AND (
                crs.last_read_at IS NULL
                OR m.created_at > crs.last_read_at
            )
        WHERE gm.user_id = $1
        GROUP BY g.id, g.name, c.id, c.name
        HAVING COUNT(m.id) > 0
        ORDER BY g.name, c.position
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(db_error!("get_unread_aggregate:guilds", user_id = %user_id))?;

    // Get DM unreads
    let dm_rows = sqlx::query(
        r"
        SELECT
            c.id as channel_id,
            c.name as channel_name,
            COUNT(m.id)::bigint as unread_count
        FROM dm_participants dp
        INNER JOIN channels c ON c.id = dp.channel_id
        LEFT JOIN channel_read_state crs ON crs.channel_id = c.id AND crs.user_id = $1
        LEFT JOIN messages m ON m.channel_id = c.id
            AND m.deleted_at IS NULL
            AND m.user_id != $1
            AND (
                crs.last_read_at IS NULL
                OR m.created_at > crs.last_read_at
            )
        WHERE dp.user_id = $1 AND c.channel_type = 'dm'
        GROUP BY c.id, c.name
        HAVING COUNT(m.id) > 0
        ORDER BY c.name
        ",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(db_error!("get_unread_aggregate:dms", user_id = %user_id))?;

    // Group guild channels by guild
    let mut guilds_map: std::collections::HashMap<Uuid, GuildUnreadSummary> =
        std::collections::HashMap::new();

    for row in guild_rows {
        let guild_id: Uuid = row.get("guild_id");
        let guild_name: String = row.get("guild_name");
        let channel_id: Uuid = row.get("channel_id");
        let channel_name: String = row.get("channel_name");
        let unread_count: i64 = row.get("unread_count");

        let guild_summary = guilds_map
            .entry(guild_id)
            .or_insert_with(|| GuildUnreadSummary {
                guild_id,
                guild_name: guild_name.clone(),
                channels: Vec::new(),
                total_unread: 0,
            });

        guild_summary.channels.push(ChannelUnread {
            channel_id,
            channel_name,
            unread_count,
        });
        guild_summary.total_unread += unread_count;
    }

    let mut guilds: Vec<GuildUnreadSummary> = guilds_map.into_values().collect();
    guilds.sort_by(|a, b| a.guild_name.cmp(&b.guild_name));

    // Build DM list
    let dms: Vec<ChannelUnread> = dm_rows
        .iter()
        .map(|row| ChannelUnread {
            channel_id: row.get("channel_id"),
            channel_name: row.get("channel_name"),
            unread_count: row.get("unread_count"),
        })
        .collect();

    // Calculate total
    let total = guilds.iter().map(|g| g.total_unread).sum::<i64>()
        + dms.iter().map(|d| d.unread_count).sum::<i64>();

    Ok(UnreadAggregate { guilds, dms, total })
}

// ── OIDC Provider Queries ──────────────────────────────────────────────

/// List all enabled OIDC providers ordered by position.
pub async fn list_oidc_providers(pool: &PgPool) -> sqlx::Result<Vec<OidcProviderRow>> {
    sqlx::query_as::<_, OidcProviderRow>(
        "SELECT * FROM oidc_providers WHERE enabled = true ORDER BY position, slug",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        error!(error = %e, "Failed to list OIDC providers");
        e
    })
}

/// List all OIDC providers (including disabled) for admin.
pub async fn list_all_oidc_providers(pool: &PgPool) -> sqlx::Result<Vec<OidcProviderRow>> {
    sqlx::query_as::<_, OidcProviderRow>("SELECT * FROM oidc_providers ORDER BY position, slug")
        .fetch_all(pool)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to list all OIDC providers");
            e
        })
}

/// Get an OIDC provider by slug.
pub async fn get_oidc_provider_by_slug(pool: &PgPool, slug: &str) -> sqlx::Result<OidcProviderRow> {
    sqlx::query_as::<_, OidcProviderRow>("SELECT * FROM oidc_providers WHERE slug = $1")
        .bind(slug)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            error!(error = %e, slug = %slug, "Failed to get OIDC provider by slug");
            e
        })
}

/// Get an OIDC provider by ID.
pub async fn get_oidc_provider_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<OidcProviderRow> {
    sqlx::query_as::<_, OidcProviderRow>("SELECT * FROM oidc_providers WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| {
            error!(error = %e, id = %id, "Failed to get OIDC provider by ID");
            e
        })
}

/// Parameters for creating a new OIDC provider.
pub struct CreateOidcProviderParams<'a> {
    pub slug: &'a str,
    pub display_name: &'a str,
    pub icon_hint: Option<&'a str>,
    pub provider_type: &'a str,
    pub issuer_url: Option<&'a str>,
    pub authorization_url: Option<&'a str>,
    pub token_url: Option<&'a str>,
    pub userinfo_url: Option<&'a str>,
    pub client_id: &'a str,
    pub client_secret_encrypted: &'a str,
    pub scopes: &'a str,
    pub created_by: Uuid,
}

/// Create a new OIDC provider.
pub async fn create_oidc_provider(
    pool: &PgPool,
    params: CreateOidcProviderParams<'_>,
) -> sqlx::Result<OidcProviderRow> {
    sqlx::query_as::<_, OidcProviderRow>(
        "INSERT INTO oidc_providers (
            slug, display_name, icon_hint, provider_type,
            issuer_url, authorization_url, token_url, userinfo_url,
            client_id, client_secret_encrypted, scopes,
            position, created_by
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            COALESCE((SELECT MAX(position) + 1 FROM oidc_providers), 0),
            $12
        ) RETURNING *",
    )
    .bind(params.slug)
    .bind(params.display_name)
    .bind(params.icon_hint)
    .bind(params.provider_type)
    .bind(params.issuer_url)
    .bind(params.authorization_url)
    .bind(params.token_url)
    .bind(params.userinfo_url)
    .bind(params.client_id)
    .bind(params.client_secret_encrypted)
    .bind(params.scopes)
    .bind(params.created_by)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        error!(error = %e, slug = %params.slug, "Failed to create OIDC provider");
        e
    })
}

/// Parameters for updating an OIDC provider.
pub struct UpdateOidcProviderParams<'a> {
    pub id: Uuid,
    pub display_name: &'a str,
    pub icon_hint: Option<&'a str>,
    pub issuer_url: Option<&'a str>,
    pub authorization_url: Option<&'a str>,
    pub token_url: Option<&'a str>,
    pub userinfo_url: Option<&'a str>,
    pub client_id: &'a str,
    pub client_secret_encrypted: Option<&'a str>,
    pub scopes: &'a str,
    pub enabled: bool,
}

/// Update an OIDC provider.
pub async fn update_oidc_provider(
    pool: &PgPool,
    params: UpdateOidcProviderParams<'_>,
) -> sqlx::Result<OidcProviderRow> {
    // If client_secret_encrypted is provided, update it; otherwise keep existing
    if let Some(secret) = params.client_secret_encrypted {
        sqlx::query_as::<_, OidcProviderRow>(
            "UPDATE oidc_providers SET
                display_name = $2, icon_hint = $3,
                issuer_url = $4, authorization_url = $5, token_url = $6, userinfo_url = $7,
                client_id = $8, client_secret_encrypted = $9, scopes = $10,
                enabled = $11, updated_at = NOW()
            WHERE id = $1 RETURNING *",
        )
        .bind(params.id)
        .bind(params.display_name)
        .bind(params.icon_hint)
        .bind(params.issuer_url)
        .bind(params.authorization_url)
        .bind(params.token_url)
        .bind(params.userinfo_url)
        .bind(params.client_id)
        .bind(secret)
        .bind(params.scopes)
        .bind(params.enabled)
        .fetch_one(pool)
        .await
    } else {
        sqlx::query_as::<_, OidcProviderRow>(
            "UPDATE oidc_providers SET
                display_name = $2, icon_hint = $3,
                issuer_url = $4, authorization_url = $5, token_url = $6, userinfo_url = $7,
                client_id = $8, scopes = $9,
                enabled = $10, updated_at = NOW()
            WHERE id = $1 RETURNING *",
        )
        .bind(params.id)
        .bind(params.display_name)
        .bind(params.icon_hint)
        .bind(params.issuer_url)
        .bind(params.authorization_url)
        .bind(params.token_url)
        .bind(params.userinfo_url)
        .bind(params.client_id)
        .bind(params.scopes)
        .bind(params.enabled)
        .fetch_one(pool)
        .await
    }
    .map_err(|e| {
        error!(error = %e, id = %params.id, "Failed to update OIDC provider");
        e
    })
}

/// Delete an OIDC provider.
pub async fn delete_oidc_provider(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM oidc_providers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| {
            error!(error = %e, id = %id, "Failed to delete OIDC provider");
            e
        })?;
    Ok(())
}

/// Get auth methods configuration.
pub async fn get_auth_methods_allowed(pool: &PgPool) -> sqlx::Result<AuthMethodsConfig> {
    match get_config_value(pool, "auth_methods_allowed").await {
        Ok(value) => match serde_json::from_value::<AuthMethodsConfig>(value.clone()) {
            Ok(config) => Ok(config),
            Err(e) => {
                error!(
                    error = %e,
                    raw_value = ?value,
                    "auth_methods_allowed config has invalid format, falling back to defaults"
                );
                Ok(AuthMethodsConfig::default())
            }
        },
        Err(sqlx::Error::RowNotFound) => Ok(AuthMethodsConfig::default()),
        Err(e) => Err(e),
    }
}

/// Set auth methods configuration.
pub async fn set_auth_methods_allowed(
    pool: &PgPool,
    config: &AuthMethodsConfig,
    updated_by: Uuid,
) -> sqlx::Result<()> {
    set_config_value(
        pool,
        "auth_methods_allowed",
        serde_json::to_value(config).unwrap_or_default(),
        updated_by,
    )
    .await
}
