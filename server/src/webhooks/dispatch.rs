//! Webhook Event Dispatch
//!
//! Non-blocking entry points for dispatching events to webhook subscribers.

use fred::prelude::*;
use sqlx::PgPool;
use tracing::{error, warn};
use uuid::Uuid;

use super::events::BotEventType;
use super::types::WebhookDeliveryItem;
use super::{delivery, queries};

/// Dispatch an event to all webhook subscribers for bots installed in a guild.
///
/// Only for guild-level events (members). Events that carry channel content
/// go through [`dispatch_channel_message_event`].
///
/// Queries webhooks joined with `guild_bot_installations` where the webhook
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

/// Dispatch an event about content of `channel_id` to the webhooks of the
/// installed applications whose bot can view that channel (`VIEW_CHANNEL`
/// with roles and overrides, like a member).
///
/// Fails closed: if the visibility check fails, nothing is delivered.
pub async fn dispatch_channel_message_event(
    db: &PgPool,
    redis: &Client,
    guild_id: Uuid,
    channel_id: Uuid,
    event_type: BotEventType,
    payload: serde_json::Value,
) {
    let webhooks = match queries::find_guild_webhooks_with_bot(db, guild_id, event_type).await {
        Ok(wh) => wh,
        Err(e) => {
            warn!(guild_id = %guild_id, event_type = %event_type, error = %e,
                "Failed to find guild webhooks for event");
            return;
        }
    };
    if webhooks.is_empty() {
        return;
    }
    let bots: Vec<Uuid> = webhooks.iter().map(|w| w.bot_user_id).collect();
    let viewing =
        match crate::permissions::bots_viewing_channel(db, guild_id, channel_id, &bots).await {
            Ok(v) => v,
            Err(e) => {
                warn!(channel_id = %channel_id, error = %e,
                    "Bot visibility check failed; webhook event not delivered");
                return;
            }
        };

    let event_id = Uuid::new_v4();
    let event_time = chrono::Utc::now();
    for entry in webhooks
        .into_iter()
        .filter(|w| viewing.contains(&w.bot_user_id))
    {
        let webhook = entry.webhook;
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
