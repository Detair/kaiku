//! Database Queries
//!
//! Runtime queries (no compile-time `DATABASE_URL` required).

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::models::{Channel, ChannelMember, ChannelType, FileAttachment, Message, Session, User};

// ============================================================================
// User Queries
// ============================================================================

/// Find user by ID.
pub async fn find_user_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// Find user by username.
pub async fn find_user_by_username(pool: &PgPool, username: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
        .bind(username)
        .fetch_optional(pool)
        .await
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
}

/// Find user by email.
pub async fn find_user_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<User>> {
    sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_optional(pool)
        .await
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
}

/// Delete a session by ID.
pub async fn delete_session(pool: &PgPool, session_id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a session by token hash.
pub async fn delete_session_by_token_hash(pool: &PgPool, token_hash: &str) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete all sessions for a user (logout everywhere).
pub async fn delete_all_user_sessions(pool: &PgPool, user_id: Uuid) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Clean up expired sessions (for background job).
pub async fn cleanup_expired_sessions(pool: &PgPool) -> sqlx::Result<u64> {
    let result = sqlx::query("DELETE FROM sessions WHERE expires_at < NOW()")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Channel Queries
// ============================================================================

/// List all channels.
pub async fn list_channels(pool: &PgPool) -> sqlx::Result<Vec<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        SELECT id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at
        FROM channels
        ORDER BY position ASC
        "
    )
    .fetch_all(pool)
    .await
}

/// List channels by category.
pub async fn list_channels_by_category(
    pool: &PgPool,
    category_id: Option<Uuid>,
) -> sqlx::Result<Vec<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        SELECT id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at
        FROM channels
        WHERE category_id IS NOT DISTINCT FROM $1
        ORDER BY position ASC
        ",
    )
    .bind(category_id)
    .fetch_all(pool)
    .await
}

/// Find channel by ID.
pub async fn find_channel_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        SELECT id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at
        FROM channels
        WHERE id = $1
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// Create a new channel.
pub async fn create_channel(
    pool: &PgPool,
    name: &str,
    channel_type: &ChannelType,
    category_id: Option<Uuid>,
    guild_id: Option<Uuid>,
    topic: Option<&str>,
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
        INSERT INTO channels (name, channel_type, category_id, guild_id, topic, user_limit, position)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at
        ",
    )
    .bind(name)
    .bind(channel_type)
    .bind(category_id)
    .bind(guild_id)
    .bind(topic)
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
    user_limit: Option<i32>,
    position: Option<i32>,
) -> sqlx::Result<Option<Channel>> {
    sqlx::query_as::<_, Channel>(
        r"
        UPDATE channels
        SET name = COALESCE($2, name),
            topic = COALESCE($3, topic),
            user_limit = COALESCE($4, user_limit),
            position = COALESCE($5, position),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at
        ",
    )
    .bind(id)
    .bind(name)
    .bind(topic)
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

/// Search messages within a guild using PostgreSQL full-text search.
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
/// Currently returns true for any authenticated user if the attachment exists,
/// since channels don't require membership for viewing/sending messages.
/// This keeps download access consistent with upload/message creation behavior.
pub async fn check_attachment_access(
    pool: &PgPool,
    attachment_id: Uuid,
    _user_id: Uuid,
) -> sqlx::Result<bool> {
    // Just verify the attachment exists - any authenticated user can access
    // since we don't require channel membership for other operations
    let result: (bool,) = sqlx::query_as(
        r"
        SELECT EXISTS(
            SELECT 1
            FROM file_attachments fa
            WHERE fa.id = $1
        )
        ",
    )
    .bind(attachment_id)
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
        SELECT id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at
        FROM channels
        WHERE guild_id = $1
        ORDER BY position ASC
        ",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}
