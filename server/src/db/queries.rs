//! Database Queries
//!
//! Runtime queries (no compile-time `DATABASE_URL` required).
//!
//! All query functions include error context logging to aid debugging.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::fmt::Write;
use tracing::error;
use uuid::Uuid;

use super::models::{
    Channel, ChannelMember, ChannelType, ChannelUnread, FileAttachment, GuildUnreadSummary,
    Message, PasswordResetToken, Session, UnreadAggregate, User,
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
    // Build dynamic update query based on what's provided
    let mut query = String::from("UPDATE users SET updated_at = NOW()");
    let mut param_idx = 1;

    if display_name.is_some() {
        write!(query, ", display_name = ${param_idx}").unwrap();
        param_idx += 1;
    }
    if email.is_some() {
        write!(query, ", email = ${param_idx}").unwrap();
        param_idx += 1;
    }

    write!(query, " WHERE id = ${param_idx} RETURNING *").unwrap();

    // Build the query with dynamic bindings
    let mut q = sqlx::query_as::<_, User>(&query);

    if let Some(name) = display_name {
        q = q.bind(name);
    }
    if let Some(mail) = email {
        q = q.bind(mail);
    }
    q = q.bind(user_id);

    q.fetch_one(pool).await
}

/// Get list of guild IDs the user is a member of.
pub async fn get_user_guild_ids(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<Uuid>> {
    let guild_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT guild_id FROM guild_members WHERE user_id = $1",
    )
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
    .await?;
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

/// Create a new channel.
pub async fn create_channel(
    pool: &PgPool,
    name: &str,
    channel_type: &ChannelType,
    category_id: Option<Uuid>,
    guild_id: Option<Uuid>,
    topic: Option<&str>,
    icon_url: Option<&str>,
    user_limit: Option<i32>,
) -> sqlx::Result<Channel> {
    // Get the next position for this category
    let row = sqlx::query(
        "SELECT COALESCE(MAX(position), 0) + 1 as next_pos FROM channels WHERE category_id IS NOT DISTINCT FROM $1",
    )
    .bind(category_id)
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
    .bind(name)
    .bind(channel_type)
    .bind(category_id)
    .bind(guild_id)
    .bind(topic)
    .bind(icon_url)
    .bind(user_limit)
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
pub async fn admin_delete_message(pool: &PgPool, id: Uuid) -> sqlx::Result<bool> {
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
    Ok(result.rows_affected() > 0)
}

/// Search messages within a guild using `PostgreSQL` full-text search.
/// Uses `websearch_to_tsquery` for user-friendly query syntax (supports AND, OR, quotes).
pub async fn search_messages(
    pool: &PgPool,
    guild_id: Uuid,
    query: &str,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<Message>> {
    sqlx::query_as::<_, Message>(
        r"
        SELECT m.*
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ORDER BY m.created_at DESC
        LIMIT $3 OFFSET $4
        ",
    )
    .bind(guild_id)
    .bind(query)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

/// Count total search results for pagination.
pub async fn count_search_messages(
    pool: &PgPool,
    guild_id: Uuid,
    query: &str,
) -> sqlx::Result<i64> {
    let result: (i64,) = sqlx::query_as(
        r"
        SELECT COUNT(*)
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ",
    )
    .bind(guild_id)
    .bind(query)
    .fetch_one(pool)
    .await?;
    Ok(result.0)
}

// ============================================================================
// File Attachment Queries
// ============================================================================

/// Create a new file attachment record.
pub async fn create_file_attachment(
    pool: &PgPool,
    message_id: Uuid,
    filename: &str,
    mime_type: &str,
    size_bytes: i64,
    s3_key: &str,
) -> sqlx::Result<FileAttachment> {
    sqlx::query_as::<_, FileAttachment>(
        r"
        INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        ",
    )
    .bind(message_id)
    .bind(filename)
    .bind(mime_type)
    .bind(size_bytes)
    .bind(s3_key)
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

    if let Some(b) = value.as_bool() { Ok(b) } else {
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

        let guild_summary = guilds_map.entry(guild_id).or_insert_with(|| GuildUnreadSummary {
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
