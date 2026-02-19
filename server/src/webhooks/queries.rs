//! Webhook Database Queries
//!
//! All webhook-related database operations.
//! Uses runtime queries (`sqlx::query` / `sqlx::query_as`) to avoid
//! requiring a live database at compile time.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

use super::events::BotEventType;
use super::types::{DeliveryLogEntry, Webhook, WebhookResponse};

/// Create a webhook.
pub async fn create_webhook(
    pool: &PgPool,
    application_id: Uuid,
    url: &str,
    signing_secret: &str,
    subscribed_events: &[BotEventType],
    description: Option<&str>,
) -> sqlx::Result<Uuid> {
    let row: (Uuid,) = sqlx::query_as(
        r"
        INSERT INTO webhooks (application_id, url, signing_secret, subscribed_events, description)
        VALUES ($1, $2, $3, $4::webhook_event_type[], $5)
        RETURNING id
        ",
    )
    .bind(application_id)
    .bind(url)
    .bind(signing_secret)
    .bind(
        subscribed_events
            .iter()
            .map(|e| e.as_str())
            .collect::<Vec<_>>(),
    )
    .bind(description)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

/// List webhooks for an application (no signing secret returned).
pub async fn list_webhooks(
    pool: &PgPool,
    application_id: Uuid,
) -> sqlx::Result<Vec<WebhookResponse>> {
    sqlx::query_as::<_, WebhookResponse>(
        r"
        SELECT id, application_id, url,
               subscribed_events,
               active, description,
               created_at, updated_at
        FROM webhooks
        WHERE application_id = $1
        ORDER BY created_at ASC
        ",
    )
    .bind(application_id)
    .fetch_all(pool)
    .await
}

/// Get a single webhook (no signing secret).
pub async fn get_webhook(
    pool: &PgPool,
    webhook_id: Uuid,
    application_id: Uuid,
) -> sqlx::Result<Option<WebhookResponse>> {
    sqlx::query_as::<_, WebhookResponse>(
        r"
        SELECT id, application_id, url,
               subscribed_events,
               active, description,
               created_at, updated_at
        FROM webhooks
        WHERE id = $1 AND application_id = $2
        ",
    )
    .bind(webhook_id)
    .bind(application_id)
    .fetch_optional(pool)
    .await
}

/// Get full webhook including signing secret (for delivery).
pub async fn get_webhook_full(pool: &PgPool, webhook_id: Uuid) -> sqlx::Result<Option<Webhook>> {
    sqlx::query_as::<_, Webhook>(
        r"
        SELECT id, application_id, url, signing_secret,
               subscribed_events,
               active, description,
               created_at, updated_at
        FROM webhooks
        WHERE id = $1
        ",
    )
    .bind(webhook_id)
    .fetch_optional(pool)
    .await
}

/// Update a webhook.
pub async fn update_webhook(
    pool: &PgPool,
    webhook_id: Uuid,
    application_id: Uuid,
    url: Option<&str>,
    subscribed_events: Option<&[BotEventType]>,
    active: Option<bool>,
    description: Option<Option<&str>>,
) -> sqlx::Result<bool> {
    let events_strs: Option<Vec<&str>> =
        subscribed_events.map(|evts| evts.iter().map(|e| e.as_str()).collect());

    let result = sqlx::query(
        r"
        UPDATE webhooks
        SET url = COALESCE($3, url),
            subscribed_events = COALESCE($4::webhook_event_type[], subscribed_events),
            active = COALESCE($5, active),
            description = CASE WHEN $6 THEN $7 ELSE description END,
            updated_at = NOW()
        WHERE id = $1 AND application_id = $2
        ",
    )
    .bind(webhook_id)
    .bind(application_id)
    .bind(url)
    .bind(events_strs)
    .bind(active)
    .bind(description.is_some())
    .bind(description.flatten())
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Delete a webhook.
pub async fn delete_webhook(
    pool: &PgPool,
    webhook_id: Uuid,
    application_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM webhooks WHERE id = $1 AND application_id = $2")
        .bind(webhook_id)
        .bind(application_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Count webhooks for an application.
pub async fn count_webhooks(pool: &PgPool, application_id: Uuid) -> sqlx::Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM webhooks WHERE application_id = $1")
        .bind(application_id)
        .fetch_one(pool)
        .await?;

    Ok(row.0)
}

/// List recent delivery log entries for a webhook.
pub async fn list_deliveries(
    pool: &PgPool,
    webhook_id: Uuid,
    limit: i64,
) -> sqlx::Result<Vec<DeliveryLogEntry>> {
    sqlx::query_as::<_, DeliveryLogEntry>(
        r"
        SELECT id, webhook_id,
               event_type,
               event_id, response_status, success,
               attempt, error_message, latency_ms, created_at
        FROM webhook_delivery_log
        WHERE webhook_id = $1
        ORDER BY created_at DESC
        LIMIT $2
        ",
    )
    .bind(webhook_id)
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// Log a delivery attempt.
pub async fn log_delivery(
    pool: &PgPool,
    webhook_id: Uuid,
    event_type: BotEventType,
    event_id: Uuid,
    response_status: Option<i16>,
    success: bool,
    attempt: i32,
    error_message: Option<&str>,
    latency_ms: Option<i32>,
) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO webhook_delivery_log
            (webhook_id, event_type, event_id, response_status, success, attempt, error_message, latency_ms)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ",
    )
    .bind(webhook_id)
    .bind(event_type.as_str())
    .bind(event_id)
    .bind(response_status)
    .bind(success)
    .bind(attempt)
    .bind(error_message)
    .bind(latency_ms)
    .execute(pool)
    .await?;

    Ok(())
}

/// Insert a dead letter entry.
pub async fn insert_dead_letter(
    pool: &PgPool,
    webhook_id: Uuid,
    event_type: BotEventType,
    event_id: Uuid,
    payload: &serde_json::Value,
    attempts: i32,
    last_error: Option<&str>,
    event_time: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO webhook_dead_letters
            (webhook_id, event_type, event_id, payload, attempts, last_error, event_time)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ",
    )
    .bind(webhook_id)
    .bind(event_type.as_str())
    .bind(event_id)
    .bind(payload)
    .bind(attempts)
    .bind(last_error)
    .bind(event_time)
    .execute(pool)
    .await?;

    Ok(())
}

/// Look up the signing secret for a webhook by ID.
pub async fn get_signing_secret(pool: &PgPool, webhook_id: Uuid) -> sqlx::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT signing_secret FROM webhooks WHERE id = $1 AND active = true")
            .bind(webhook_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(s,)| s))
}

/// Delete delivery log entries older than `retention_days`.
pub async fn cleanup_old_delivery_logs(pool: &PgPool, retention_days: i32) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM webhook_delivery_log WHERE created_at < NOW() - make_interval(days => $1)",
    )
    .bind(retention_days)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Delete dead letter entries older than `retention_days`.
pub async fn cleanup_old_dead_letters(pool: &PgPool, retention_days: i32) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM webhook_dead_letters WHERE created_at < NOW() - make_interval(days => $1)",
    )
    .bind(retention_days)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Find all active webhooks for bots installed in a guild that subscribe to an event type.
pub async fn find_guild_webhooks_for_event(
    pool: &PgPool,
    guild_id: Uuid,
    event_type: BotEventType,
) -> sqlx::Result<Vec<Webhook>> {
    let event_str = event_type.as_str();

    sqlx::query_as::<_, Webhook>(
        r"
        SELECT w.id, w.application_id, w.url, w.signing_secret,
               w.subscribed_events,
               w.active, w.description, w.created_at, w.updated_at
        FROM webhooks w
        JOIN guild_bot_installations gbi ON gbi.application_id = w.application_id
        WHERE gbi.guild_id = $1
          AND w.active = true
          AND $2::webhook_event_type = ANY(w.subscribed_events)
        ",
    )
    .bind(guild_id)
    .bind(event_str)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        error!(guild_id = %guild_id, event_type = %event_str, error = %e, "Failed to find guild webhooks");
        e
    })
}

/// Find active webhooks for a specific application that subscribe to an event type.
pub async fn find_app_webhooks_for_event(
    pool: &PgPool,
    application_id: Uuid,
    event_type: BotEventType,
) -> sqlx::Result<Vec<Webhook>> {
    let event_str = event_type.as_str();

    sqlx::query_as::<_, Webhook>(
        r"
        SELECT id, application_id, url, signing_secret,
               subscribed_events,
               active, description, created_at, updated_at
        FROM webhooks
        WHERE application_id = $1
          AND active = true
          AND $2::webhook_event_type = ANY(subscribed_events)
        ",
    )
    .bind(application_id)
    .bind(event_str)
    .fetch_all(pool)
    .await
}
