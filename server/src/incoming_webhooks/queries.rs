//! Incoming webhook SQL queries.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::error::IncomingWebhookError;
use super::types::IncomingWebhook;
use crate::db::Message;

/// Discord's per-channel webhook cap.
pub const MAX_WEBHOOKS_PER_CHANNEL: i64 = 15;

pub async fn create_webhook(
    pool: &PgPool,
    guild_id: Uuid,
    channel_id: Uuid,
    name: &str,
    avatar_url: Option<&str>,
    token: &str,
    created_by: Uuid,
) -> Result<IncomingWebhook, IncomingWebhookError> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM incoming_webhooks WHERE channel_id = $1")
            .bind(channel_id)
            .fetch_one(pool)
            .await?;
    if count >= MAX_WEBHOOKS_PER_CHANNEL {
        return Err(IncomingWebhookError::MaxWebhooksReached);
    }

    let webhook = sqlx::query_as::<_, IncomingWebhook>(
        "INSERT INTO incoming_webhooks (guild_id, channel_id, name, avatar_url, token, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(guild_id)
    .bind(channel_id)
    .bind(name)
    .bind(avatar_url)
    .bind(token)
    .bind(created_by)
    .fetch_one(pool)
    .await?;
    Ok(webhook)
}

pub async fn find_webhook_by_id(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<IncomingWebhook>, IncomingWebhookError> {
    let webhook =
        sqlx::query_as::<_, IncomingWebhook>("SELECT * FROM incoming_webhooks WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(webhook)
}

pub async fn list_channel_webhooks(
    pool: &PgPool,
    channel_id: Uuid,
) -> Result<Vec<IncomingWebhook>, IncomingWebhookError> {
    let webhooks = sqlx::query_as::<_, IncomingWebhook>(
        "SELECT * FROM incoming_webhooks WHERE channel_id = $1 ORDER BY created_at",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await?;
    Ok(webhooks)
}

pub async fn list_guild_webhooks(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Vec<IncomingWebhook>, IncomingWebhookError> {
    let webhooks = sqlx::query_as::<_, IncomingWebhook>(
        "SELECT * FROM incoming_webhooks WHERE guild_id = $1 ORDER BY created_at",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await?;
    Ok(webhooks)
}

pub async fn update_webhook(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    avatar_url: Option<&str>,
    channel_id: Option<Uuid>,
) -> Result<IncomingWebhook, IncomingWebhookError> {
    let webhook = sqlx::query_as::<_, IncomingWebhook>(
        "UPDATE incoming_webhooks SET
            name = COALESCE($2, name),
            avatar_url = COALESCE($3, avatar_url),
            channel_id = COALESCE($4, channel_id),
            updated_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .bind(name)
    .bind(avatar_url)
    .bind(channel_id)
    .fetch_one(pool)
    .await?;
    Ok(webhook)
}

pub async fn delete_webhook(pool: &PgPool, id: Uuid) -> Result<bool, IncomingWebhookError> {
    let affected = sqlx::query("DELETE FROM incoming_webhooks WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(affected > 0)
}

// ============================================================================
// Webhook message inserts
// ============================================================================

pub struct WebhookMessageParams<'a> {
    pub channel_id: Uuid,
    pub webhook_id: Uuid,
    /// Effective display name (override ?? webhook.name) — snapshotted.
    pub username: &'a str,
    /// Effective avatar (override ?? `webhook.avatar_url`) — snapshotted.
    pub avatar_url: Option<&'a str>,
    pub content: &'a str,
    pub embeds: Option<&'a serde_json::Value>,
}

/// Insert a plain webhook message (no author user; snapshot columns instead).
pub async fn create_webhook_message(
    pool: &PgPool,
    params: WebhookMessageParams<'_>,
) -> Result<Message, IncomingWebhookError> {
    let message = sqlx::query_as::<_, Message>(
        "INSERT INTO messages
            (channel_id, user_id, content, encrypted, embeds,
             webhook_id, webhook_username, webhook_avatar_url)
         VALUES ($1, NULL, $2, false, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(params.channel_id)
    .bind(params.content)
    .bind(params.embeds)
    .bind(params.webhook_id)
    .bind(params.username)
    .bind(params.avatar_url)
    .fetch_one(pool)
    .await?;
    Ok(message)
}

/// Insert a webhook thread reply and bump the parent's counters
/// (mirrors `db::create_thread_reply`, but with a NULL author).
pub async fn create_webhook_thread_reply(
    pool: &PgPool,
    parent_id: Uuid,
    params: WebhookMessageParams<'_>,
) -> Result<Message, IncomingWebhookError> {
    let mut tx = pool.begin().await?;

    let message = sqlx::query_as::<_, Message>(
        "INSERT INTO messages
            (channel_id, user_id, content, encrypted, parent_id, embeds,
             webhook_id, webhook_username, webhook_avatar_url)
         VALUES ($1, NULL, $2, false, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(params.channel_id)
    .bind(params.content)
    .bind(parent_id)
    .bind(params.embeds)
    .bind(params.webhook_id)
    .bind(params.username)
    .bind(params.avatar_url)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE messages
         SET thread_reply_count = thread_reply_count + 1,
             thread_last_reply_at = $2
         WHERE id = $1",
    )
    .bind(parent_id)
    .bind(message.created_at)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE forum_posts SET last_activity_at = NOW() WHERE root_message_id = $1")
        .bind(parent_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(message)
}

/// Root message + `forum_posts` row created by a webhook (`thread_name`).
/// Mirrors `chat::forum::create_post`, with a NULL author.
pub async fn create_webhook_forum_post(
    pool: &PgPool,
    title: &str,
    params: WebhookMessageParams<'_>,
) -> Result<(Message, Uuid, DateTime<Utc>, DateTime<Utc>), IncomingWebhookError> {
    let mut tx = pool.begin().await?;

    let message = sqlx::query_as::<_, Message>(
        "INSERT INTO messages
            (channel_id, user_id, content, encrypted, embeds,
             webhook_id, webhook_username, webhook_avatar_url)
         VALUES ($1, NULL, $2, false, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(params.channel_id)
    .bind(params.content)
    .bind(params.embeds)
    .bind(params.webhook_id)
    .bind(params.username)
    .bind(params.avatar_url)
    .fetch_one(&mut *tx)
    .await?;

    let post: (Uuid, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO forum_posts (channel_id, root_message_id, title, author_id)
         VALUES ($1, $2, $3, NULL)
         RETURNING id, created_at, last_activity_at",
    )
    .bind(params.channel_id)
    .bind(message.id)
    .bind(title)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok((message, post.0, post.1, post.2))
}

// ============================================================================
// Webhook message lookups / edit / delete (token-scoped)
// ============================================================================

/// Find a live message created by this specific webhook.
pub async fn find_webhook_message(
    pool: &PgPool,
    message_id: Uuid,
    webhook_id: Uuid,
) -> Result<Option<Message>, IncomingWebhookError> {
    let message = sqlx::query_as::<_, Message>(
        "SELECT * FROM messages
         WHERE id = $1 AND webhook_id = $2 AND deleted_at IS NULL",
    )
    .bind(message_id)
    .bind(webhook_id)
    .fetch_optional(pool)
    .await?;
    Ok(message)
}

pub async fn update_webhook_message(
    pool: &PgPool,
    message_id: Uuid,
    content: Option<&str>,
    embeds: Option<&serde_json::Value>,
) -> Result<Message, IncomingWebhookError> {
    let message = sqlx::query_as::<_, Message>(
        "UPDATE messages SET
            content = COALESCE($2, content),
            embeds = COALESCE($3, embeds),
            edited_at = NOW()
         WHERE id = $1
         RETURNING *",
    )
    .bind(message_id)
    .bind(content)
    .bind(embeds)
    .fetch_one(pool)
    .await?;
    Ok(message)
}

/// Soft-delete (same semantics as `db::delete_message`).
pub async fn delete_webhook_message(
    pool: &PgPool,
    message_id: Uuid,
) -> Result<(), IncomingWebhookError> {
    sqlx::query("UPDATE messages SET deleted_at = NOW(), content = '[deleted]' WHERE id = $1")
        .bind(message_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolve a `thread_id` to a thread root message within the given channel.
///
/// Accepts either a root message id or a forum post id (Discord uses the
/// thread/post id; Kaiku forum posts have distinct ids from their root
/// message).
pub async fn resolve_thread_root(
    pool: &PgPool,
    thread_id: Uuid,
    channel_id: Uuid,
) -> Result<Option<(Uuid, bool)>, IncomingWebhookError> {
    // (root_message_id, locked)
    let row: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT m.id, COALESCE(p.locked, false)
         FROM messages m
         LEFT JOIN forum_posts p ON p.root_message_id = m.id
         WHERE m.channel_id = $2 AND m.parent_id IS NULL AND m.deleted_at IS NULL
           AND (m.id = $1 OR p.id = $1)",
    )
    .bind(thread_id)
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
