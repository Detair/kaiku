//! Webhook Event Dispatch
//!
//! Non-blocking entry points for dispatching events to webhook subscribers.

use fred::prelude::*;
use sqlx::PgPool;
use tracing::{error, warn};
use uuid::Uuid;

use super::delivery;
use super::events::BotEventType;
use super::queries;
use super::types::WebhookDeliveryItem;

/// Dispatch an event to all webhook subscribers for bots installed in a guild.
///
/// Queries webhooks joined with guild_bot_installations where the webhook
/// subscribes to the given event type. Enqueues one delivery item per webhook.
pub async fn dispatch_guild_event(
    db: &PgPool,
    redis: &Client,
    guild_id: Uuid,
    event_type: BotEventType,
    payload: serde_json::Value,
) {
    let webhooks = match queries::find_guild_webhooks_for_event(db, guild_id, event_type).await {
        Ok(wh) => wh,
        Err(e) => {
            warn!(
                guild_id = %guild_id,
                event_type = %event_type,
                error = %e,
                "Failed to find guild webhooks for event"
            );
            return;
        }
    };

    if webhooks.is_empty() {
        return;
    }

    let event_id = Uuid::new_v4();
    let event_time = chrono::Utc::now();

    for webhook in webhooks {
        let item = WebhookDeliveryItem {
            webhook_id: webhook.id,
            url: webhook.url.clone(),
            event_type,
            event_id,
            payload: payload.clone(),
            attempt: 0,
            event_time,
        };

        if let Err(e) = delivery::enqueue(redis, &item).await {
            error!(
                webhook_id = %webhook.id,
                event_id = %event_id,
                "Failed to enqueue webhook delivery: {}", e
            );
        }
    }
}

/// Dispatch a command.invoked event to a specific application's webhooks.
pub async fn dispatch_command_event(
    db: &PgPool,
    redis: &Client,
    application_id: Uuid,
    payload: serde_json::Value,
) {
    let webhooks = match queries::find_app_webhooks_for_event(
        db,
        application_id,
        BotEventType::CommandInvoked,
    )
    .await
    {
        Ok(wh) => wh,
        Err(e) => {
            warn!(
                application_id = %application_id,
                error = %e,
                "Failed to find app webhooks for command event"
            );
            return;
        }
    };

    if webhooks.is_empty() {
        return;
    }

    let event_id = Uuid::new_v4();
    let event_time = chrono::Utc::now();

    for webhook in webhooks {
        let item = WebhookDeliveryItem {
            webhook_id: webhook.id,
            url: webhook.url.clone(),
            event_type: BotEventType::CommandInvoked,
            event_id,
            payload: payload.clone(),
            attempt: 0,
            event_time,
        };

        if let Err(e) = delivery::enqueue(redis, &item).await {
            error!(
                webhook_id = %webhook.id,
                event_id = %event_id,
                "Failed to enqueue webhook delivery: {}", e
            );
        }
    }
}
