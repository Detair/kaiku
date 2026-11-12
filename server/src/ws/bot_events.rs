//! Bot Gateway Event Publishing
//!
//! Publishes events to `bot:{bot_user_id}` Redis channels for all bots
//! installed in a guild that have the matching intent declared.
//!
//! What a bot may see is decided **when an event is dispatched**: its
//! visibility of the channel is read from the database for every event,
//! here, before the publish. That is the rule for bots — there is no access
//! epoch for bot sockets and no second check when the gateway writes a
//! message event, so an event already on its way when a bot's role is
//! removed is still delivered; everything dispatched after that commit is
//! not. (Interactions are checked once more at the gateway.)

use fred::interfaces::PubsubInterface;
use fred::prelude::*;
use sqlx::PgPool;
use tracing::{error, warn};
use uuid::Uuid;

use super::bot_gateway::BotServerEvent;

/// Find bot user IDs for bots with a specific intent installed in a guild.
async fn bots_with_intent(
    db: &PgPool,
    guild_id: Uuid,
    intent: &str,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        r"
        SELECT ba.bot_user_id
        FROM bot_applications ba
        JOIN guild_bot_installations gbi ON gbi.application_id = ba.id
        WHERE gbi.guild_id = $1
          AND ba.bot_user_id IS NOT NULL
          AND $2 = ANY(ba.gateway_intents)
        ",
    )
    .bind(guild_id)
    .bind(intent)
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Publish a `MessageCreated` event to the bots with `messages` intent in a
/// guild that can view `channel_id`.
pub async fn publish_message_created(
    db: &PgPool,
    redis: &Client,
    guild_id: Uuid,
    channel_id: Uuid,
    message_id: Uuid,
    user_id: Uuid,
    content: &str,
) {
    let bot_ids = match bots_with_intent(db, guild_id, "messages").await {
        Ok(ids) => ids,
        Err(e) => {
            warn!(guild_id = %guild_id, error = %e, "Failed to find bots with messages intent");
            return;
        }
    };

    if bot_ids.is_empty() {
        return;
    }

    // Only bots that can view the channel (roles + overrides, like a member).
    // Fails closed: no delivery if the check fails.
    let viewing =
        match crate::permissions::bots_viewing_channel(db, guild_id, channel_id, &bot_ids).await {
            Ok(v) => v,
            Err(e) => {
                warn!(channel_id = %channel_id, error = %e,
                    "Bot visibility check failed; message not delivered to bots");
                return;
            }
        };
    let bot_ids: Vec<Uuid> = bot_ids
        .into_iter()
        .filter(|id| viewing.contains(id))
        .collect();
    if bot_ids.is_empty() {
        return;
    }

    let event = BotServerEvent::MessageCreated {
        message_id,
        channel_id,
        guild_id: Some(guild_id),
        user_id,
        content: content.to_string(),
    };

    let payload = match serde_json::to_string(&event) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to serialize MessageCreated event: {}", e);
            return;
        }
    };

    for bot_id in bot_ids {
        let channel = format!("bot:{bot_id}");
        if let Err(e) = redis.publish::<(), _, _>(&channel, &payload).await {
            warn!(bot_id = %bot_id, error = %e, "Failed to publish MessageCreated to bot");
        }
    }
}

/// Publish a `MemberJoined` event to all bots with `members` intent in a guild.
pub async fn publish_member_joined(
    db: &PgPool,
    redis: &Client,
    guild_id: Uuid,
    user_id: Uuid,
    username: &str,
    display_name: &str,
) {
    let bot_ids = match bots_with_intent(db, guild_id, "members").await {
        Ok(ids) => ids,
        Err(e) => {
            warn!(guild_id = %guild_id, error = %e, "Failed to find bots with members intent");
            return;
        }
    };

    if bot_ids.is_empty() {
        return;
    }

    let event = BotServerEvent::MemberJoined {
        guild_id,
        user_id,
        username: username.to_string(),
        display_name: display_name.to_string(),
    };

    let payload = match serde_json::to_string(&event) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to serialize MemberJoined event: {}", e);
            return;
        }
    };

    for bot_id in bot_ids {
        let channel = format!("bot:{bot_id}");
        if let Err(e) = redis.publish::<(), _, _>(&channel, &payload).await {
            warn!(bot_id = %bot_id, error = %e, "Failed to publish MemberJoined to bot");
        }
    }
}

/// Publish a `MemberLeft` event to all bots with `members` intent in a guild.
pub async fn publish_member_left(db: &PgPool, redis: &Client, guild_id: Uuid, user_id: Uuid) {
    let bot_ids = match bots_with_intent(db, guild_id, "members").await {
        Ok(ids) => ids,
        Err(e) => {
            warn!(guild_id = %guild_id, error = %e, "Failed to find bots with members intent");
            return;
        }
    };

    if bot_ids.is_empty() {
        return;
    }

    let event = BotServerEvent::MemberLeft { guild_id, user_id };

    let payload = match serde_json::to_string(&event) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to serialize MemberLeft event: {}", e);
            return;
        }
    };

    for bot_id in bot_ids {
        let channel = format!("bot:{bot_id}");
        if let Err(e) = redis.publish::<(), _, _>(&channel, &payload).await {
            warn!(bot_id = %bot_id, error = %e, "Failed to publish MemberLeft to bot");
        }
    }
}
