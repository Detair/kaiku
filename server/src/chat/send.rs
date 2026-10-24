//! Shared send path for new channel messages.
//!
//! Text messages (`messages::create`) and file messages
//! (`uploads::upload_message_with_file`) run the same authorization before
//! anything is stored and the same fan-out after the message is committed.

use tracing::warn;
use uuid::Uuid;

use super::{queries, ChatError};
use crate::api::AppState;
use crate::db;
use crate::moderation::filter_types::FilterAction;
use crate::moderation::queries as filter_queries;
use crate::permissions::GuildPermissions;
use crate::social::block_cache;
use crate::ws::{broadcast_admin_event, broadcast_to_channel, ServerEvent};

/// What the sender is about to post, as far as authorization cares.
pub struct SendIntent<'a> {
    /// Message text (may be empty for file-only messages).
    pub content: &'a str,
    /// E2EE message: content cannot be inspected by the content filter.
    pub encrypted: bool,
    /// The message carries a file attachment (requires `ATTACH_FILES`).
    pub has_attachment: bool,
}

/// Check that `user_id` may post `intent` into `channel_id`.
///
/// Order: channel exists, channel access, guild `SEND_MESSAGES` (+
/// `ATTACH_FILES` for attachments), announcement `SEND_ANNOUNCEMENTS`, DM
/// block check, `@everyone`/`@here` permission, guild content filter.
///
/// Returns the channel on success.
#[tracing::instrument(skip(state, intent))]
pub async fn authorize_send(
    state: &AppState,
    user_id: Uuid,
    channel_id: Uuid,
    intent: &SendIntent<'_>,
) -> Result<db::Channel, ChatError> {
    // Check channel exists
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    let ctx = crate::permissions::require_channel_access(&state.db, user_id, channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    // For guild channels, also check SEND_MESSAGES permission (and ATTACH_FILES
    // when the message carries a file)
    if channel.guild_id.is_some() {
        if !ctx.has_permission(GuildPermissions::SEND_MESSAGES) {
            return Err(ChatError::Forbidden);
        }
        if intent.has_attachment && !ctx.has_permission(GuildPermissions::ATTACH_FILES) {
            return Err(ChatError::Forbidden);
        }
    }

    // Announcement channels require SEND_ANNOUNCEMENTS to publish (a distinct,
    // more restricted right than SEND_MESSAGES).
    if channel.channel_type == db::ChannelType::Announcement
        && !ctx.has_permission(GuildPermissions::SEND_ANNOUNCEMENTS)
    {
        return Err(ChatError::Forbidden);
    }

    // For DM channels, check if any participant has blocked the other
    if channel.channel_type == db::ChannelType::Dm {
        let participants = queries::list_dm_participant_ids(&state.db, channel_id).await?;

        for &participant_id in &participants {
            if participant_id != user_id {
                match block_cache::is_blocked_either_direction(
                    &state.redis,
                    user_id,
                    participant_id,
                )
                .await
                {
                    Ok(true) => return Err(ChatError::Blocked),
                    Ok(false) => {}
                    Err(e) => {
                        warn!(
                            error = %e,
                            user_id = %user_id,
                            target_id = %participant_id,
                            fail_open = state.config.block_check_fail_open,
                            "Redis block check failed, using failsafe policy"
                        );
                        if !state.config.block_check_fail_open {
                            return Err(ChatError::Blocked);
                        }
                    }
                }
            }
        }
    }

    // Check for @everyone/@here mentions in guild channels. `ctx` comes from
    // `require_channel_access`, so channel overrides apply.
    if channel.guild_id.is_some() {
        let mentions = super::mentions::extract_mentions(intent.content);
        if (mentions.everyone || mentions.here)
            && !ctx.has_permission(GuildPermissions::MENTION_EVERYONE)
        {
            return Err(ChatError::Validation(
                "You do not have permission to mention @everyone or @here".to_string(),
            ));
        }
    }

    // Content filtering: skip encrypted messages (can't inspect E2EE), DMs
    // (guild-scoped) and empty text (file-only messages)
    if !intent.encrypted && !intent.content.is_empty() {
        if let Some(guild_id) = channel.guild_id {
            if let Ok(engine) = state.filter_cache.get_or_build(&state.db, guild_id).await {
                let result = engine.check(intent.content);
                if result.blocked {
                    // Log all matches to moderation_actions table
                    for m in &result.matches {
                        filter_queries::log_moderation_action(
                            &state.db,
                            &filter_queries::LogActionParams {
                                guild_id,
                                user_id,
                                channel_id,
                                action: m.action,
                                category: Some(m.category),
                                matched_pattern: &m.matched_pattern,
                                original_content: intent.content,
                                custom_pattern_id: m.custom_pattern_id,
                            },
                        )
                        .await
                        .ok();
                    }
                    // Notify admins
                    if let Some(first) = result.matches.first() {
                        broadcast_admin_event(
                            &state.redis,
                            &ServerEvent::AdminModerationBlocked {
                                guild_id,
                                user_id,
                                channel_id,
                                category: first.category.to_string(),
                            },
                        )
                        .await
                        .ok();
                    }
                    return Err(ChatError::ContentFiltered);
                }
                // For "log" and "warn" actions, still log but allow the message
                for m in result
                    .matches
                    .iter()
                    .filter(|m| m.action == FilterAction::Log || m.action == FilterAction::Warn)
                {
                    filter_queries::log_moderation_action(
                        &state.db,
                        &filter_queries::LogActionParams {
                            guild_id,
                            user_id,
                            channel_id,
                            action: m.action,
                            category: Some(m.category),
                            matched_pattern: &m.matched_pattern,
                            original_content: intent.content,
                            custom_pattern_id: m.custom_pattern_id,
                        },
                    )
                    .await
                    .ok();
                }
            }
        }
    }

    Ok(channel)
}

/// A committed top-level message to fan out.
pub struct NewMessageDispatch<'a> {
    pub channel: &'a db::Channel,
    pub author_id: Uuid,
    pub message_id: Uuid,
    pub content: &'a str,
    pub encrypted: bool,
    pub is_crosspost: bool,
    /// Serialized `MessageResponse` (without the sender-only nonce).
    pub message_json: serde_json::Value,
}

/// Fan out a committed top-level message: `MessageNew` broadcast,
/// announcement crosspost, push wake (DMs, mentions), bot/webhook events.
///
/// Never fails; delivery problems are logged.
pub async fn dispatch_new_message(state: &AppState, d: NewMessageDispatch<'_>) {
    let channel_id = d.channel.id;

    // Regular message: broadcast MessageNew
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::MessageNew {
            channel_id,
            message: d.message_json,
        },
    )
    .await
    {
        warn!(channel_id = %channel_id, error = %e, "Failed to broadcast new message event");
    }

    // Announcement publish → fan out to follower channels (async, loop-guarded).
    // The crosspost carries text only, so a file-only message is not forwarded.
    if d.channel.channel_type == db::ChannelType::Announcement
        && !d.is_crosspost
        && !d.content.is_empty()
    {
        crate::chat::announcements::spawn_crosspost(
            state.clone(),
            channel_id,
            d.content.to_string(),
        );
    }

    // Content-free mobile wake: DM participants and @mention targets.
    // Resolution and delivery run in a spawned task.
    crate::push::spawn_wake_for_new_message(
        state,
        d.channel.clone(),
        d.author_id,
        d.content.to_string(),
        d.encrypted,
    );

    dispatch_bot_events(
        state,
        d.channel,
        d.author_id,
        d.message_id,
        d.content,
        d.encrypted,
    );
}

/// Dispatch a new message to the bot ecosystem (non-blocking, fire-and-forget).
///
/// Only guild channels and non-encrypted messages are dispatched.
pub fn dispatch_bot_events(
    state: &AppState,
    channel: &db::Channel,
    author_id: Uuid,
    message_id: Uuid,
    content: &str,
    encrypted: bool,
) {
    if let Some(guild_id) = channel.guild_id {
        if !encrypted {
            let db = state.db.clone();
            let redis = state.redis.clone();
            let msg_id = message_id;
            let ch_id = channel.id;
            let uid = author_id;
            let content = content.to_string();
            tokio::spawn(async move {
                crate::ws::bot_events::publish_message_created(
                    &db, &redis, guild_id, ch_id, msg_id, uid, &content,
                )
                .await;
                crate::webhooks::dispatch::dispatch_guild_event(
                    &db,
                    &redis,
                    guild_id,
                    crate::webhooks::events::BotEventType::MessageCreated,
                    serde_json::json!({
                        "guild_id": guild_id,
                        "channel_id": ch_id,
                        "message_id": msg_id,
                        "user_id": uid,
                        "content": content,
                    }),
                )
                .await;
            });
        }
    }
}
