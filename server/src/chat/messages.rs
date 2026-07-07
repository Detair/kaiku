//! Message Handlers

use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use fred::interfaces::{KeysInterface, PubsubInterface};
use tracing::warn;
use uuid::Uuid;
use validator::Validate;

use super::types::{
    detect_mention_type, AttachmentInfo, AuthorProfile, CreateMessageRequest,
    CursorPaginatedResponse, ListMessagesQuery, ListThreadRepliesQuery, MessageResponse,
    ReactionInfo, ThreadInfoResponse, UpdateMessageRequest,
};
use super::{queries, ChatError};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db;
use crate::moderation::filter_types::FilterAction;
use crate::moderation::queries as filter_queries;
use crate::permissions::{get_member_permission_context, GuildPermissions};
use crate::social::block_cache;
use crate::ws::{broadcast_admin_event, broadcast_to_channel, broadcast_to_user, ServerEvent};

// ============================================================================
// Handlers
// ============================================================================

/// List messages in a channel.
/// GET /`api/messages/channel/:channel_id`
///
/// Returns cursor-based pagination with `has_more` indicator.
/// Use the `next_cursor` value as `before` parameter to fetch the next page.
#[utoipa::path(
    get,
    path = "/api/messages/channel/{channel_id}",
    tag = "messages",
    params(("channel_id" = Uuid, Path, description = "Channel ID")),
    responses(
        (status = 200, body = CursorPaginatedResponse<MessageResponse>),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id, channel_id = %channel_id))]
pub async fn list(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<CursorPaginatedResponse<MessageResponse>>, ChatError> {
    // Check channel exists and user has VIEW_CHANNEL permission
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    // Load combined block set for filtering
    let blocked_ids = block_cache::load_blocked_users(&state.db, &state.redis, auth_user.id)
        .await
        .unwrap_or_else(|e| {
            warn!(user_id = %auth_user.id, error = %e, "Failed to load blocked users, message filtering may be incomplete");
            HashSet::default()
        });
    let blocked_by_ids = block_cache::load_blocked_by(&state.db, &state.redis, auth_user.id)
        .await
        .unwrap_or_else(|e| {
            warn!(user_id = %auth_user.id, error = %e, "Failed to load blocked-by users, message filtering may be incomplete");
            HashSet::default()
        });
    let combined_block_set: std::collections::HashSet<Uuid> =
        blocked_ids.union(&blocked_by_ids).copied().collect();

    // Limit between 1 and 100
    let limit = query.limit.clamp(1, 100);

    // Fetch messages — either around a specific message or before-based pagination
    let (mut messages, has_more) = if let Some(around_id) = query.around {
        let msgs = db::list_messages_around(&state.db, channel_id, around_id, limit).await?;
        let has_more = msgs.len() as i64 >= limit;
        (msgs, has_more)
    } else {
        // Fetch one extra message to determine if there are more
        let mut msgs = db::list_messages(&state.db, channel_id, query.before, limit + 1).await?;
        let has_more = msgs.len() as i64 > limit;
        if has_more {
            msgs.pop();
        }
        (msgs, has_more)
    };

    // Filter out messages from blocked users (application-layer filtering)
    if !combined_block_set.is_empty() {
        messages.retain(|m| {
            m.user_id
                .is_none_or(|uid| !combined_block_set.contains(&uid))
        });
    }

    // Build response with author info, attachments, and reactions
    let response = build_message_responses(&state.db, auth_user.id, messages).await?;

    // Get the cursor for the next page (oldest message ID)
    let next_cursor = if has_more {
        response.last().map(|m| m.id)
    } else {
        None
    };

    Ok(Json(CursorPaginatedResponse {
        items: response,
        has_more,
        next_cursor,
    }))
}

/// Create a new message.
/// POST /`api/messages/channel/:channel_id`
#[utoipa::path(
    post,
    path = "/api/messages/channel/{channel_id}",
    tag = "messages",
    params(("channel_id" = Uuid, Path, description = "Channel ID")),
    request_body = CreateMessageRequest,
    responses(
        (status = 201, body = MessageResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body), fields(user_id = %auth_user.id, channel_id = %channel_id))]
pub async fn create(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), ChatError> {
    // Validate input
    body.validate()
        .map_err(|e| ChatError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Check channel exists
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    // For guild channels, also check SEND_MESSAGES permission
    if channel.guild_id.is_some() && !ctx.has_permission(GuildPermissions::SEND_MESSAGES) {
        return Err(ChatError::Forbidden);
    }

    // For DM channels, check if any participant has blocked the other
    if channel.channel_type == db::ChannelType::Dm {
        let participants = queries::list_dm_participant_ids(&state.db, channel_id).await?;

        for &participant_id in &participants {
            if participant_id != auth_user.id {
                match block_cache::is_blocked_either_direction(
                    &state.redis,
                    auth_user.id,
                    participant_id,
                )
                .await
                {
                    Ok(true) => return Err(ChatError::Blocked),
                    Ok(false) => {}
                    Err(e) => {
                        warn!(
                            error = %e,
                            user_id = %auth_user.id,
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

    // Check for @everyone/@here mentions in guild channels
    if let Some(guild_id) = channel.guild_id {
        if body.content.contains("@everyone") || body.content.contains("@here") {
            // Load user's permissions in this guild
            if let Ok(Some(ctx)) =
                get_member_permission_context(&state.db, guild_id, auth_user.id).await
            {
                if !ctx.has_permission(GuildPermissions::MENTION_EVERYONE) {
                    return Err(ChatError::Validation(
                        "You do not have permission to mention @everyone or @here".to_string(),
                    ));
                }
            } else {
                // User is not a guild member, should not happen if channel access is correct
                return Err(ChatError::Forbidden);
            }
        }
    }

    // Validate encrypted messages have nonce
    if body.encrypted && body.nonce.is_none() {
        return Err(ChatError::Validation(
            "Encrypted messages require a nonce".to_string(),
        ));
    }

    // Content filtering: skip encrypted messages (can't inspect E2EE) and DMs (guild-scoped)
    if !body.encrypted {
        if let Some(guild_id) = channel.guild_id {
            if let Ok(engine) = state.filter_cache.get_or_build(&state.db, guild_id).await {
                let result = engine.check(&body.content);
                if result.blocked {
                    // Log all matches to moderation_actions table
                    for m in &result.matches {
                        filter_queries::log_moderation_action(
                            &state.db,
                            &filter_queries::LogActionParams {
                                guild_id,
                                user_id: auth_user.id,
                                channel_id,
                                action: m.action,
                                category: Some(m.category),
                                matched_pattern: &m.matched_pattern,
                                original_content: &body.content,
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
                                user_id: auth_user.id,
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
                            user_id: auth_user.id,
                            channel_id,
                            action: m.action,
                            category: Some(m.category),
                            matched_pattern: &m.matched_pattern,
                            original_content: &body.content,
                            custom_pattern_id: m.custom_pattern_id,
                        },
                    )
                    .await
                    .ok();
                }
            }
        }
    }

    // Validate reply_to exists if provided
    if let Some(reply_id) = body.reply_to {
        let reply_msg = db::find_message_by_id(&state.db, reply_id).await?;
        if reply_msg.is_none() {
            return Err(ChatError::Validation("Reply target not found".to_string()));
        }
    }

    // Route slash command invocations to installed bots in guild channels.
    if let Some(guild_id) = channel.guild_id {
        if let Some(command_input) = body.content.trim().strip_prefix('/') {
            let mut parts = command_input.split_whitespace();
            if let Some(command_name) = parts.next() {
                let command_name = command_name.to_lowercase();

                // Built-in /ping: responds directly without bot routing
                if command_name == "ping" {
                    let start = std::time::Instant::now();
                    let author = db::find_user_by_id(&state.db, auth_user.id)
                        .await?
                        .map(AuthorProfile::from)
                        .unwrap_or_else(|| AuthorProfile {
                            id: auth_user.id,
                            username: "unknown".to_string(),
                            display_name: "Unknown User".to_string(),
                            avatar_url: None,
                            status: "offline".to_string(),
                        });
                    let latency_ms = start.elapsed().as_millis();
                    let content = format!("Pong! (latency: {latency_ms}ms)");

                    let msg =
                        queries::insert_ping_message(&state.db, channel_id, auth_user.id, &content)
                            .await?;

                    let response = MessageResponse {
                        id: msg.0,
                        channel_id,
                        author,
                        content,
                        encrypted: false,
                        attachments: vec![],
                        reply_to: None,
                        parent_id: None,
                        thread_reply_count: 0,
                        thread_last_reply_at: None,
                        edited_at: None,
                        created_at: msg.1,
                        mention_type: None,
                        reactions: None,
                        thread_info: None,
                        pinned: false,
                        message_type: "user".to_string(),
                        nonce: None,
                        embeds: None,
                        components: None,
                    };

                    let message_json = serde_json::to_value(&response).unwrap_or_default();
                    if let Err(e) = broadcast_to_channel(
                        &state.redis,
                        channel_id,
                        &ServerEvent::MessageNew {
                            channel_id,
                            message: message_json,
                        },
                    )
                    .await
                    {
                        warn!(channel_id = %channel_id, error = %e, "Failed to broadcast ping response");
                    }

                    return Ok((StatusCode::OK, Json(response)));
                }

                let commands =
                    queries::list_slash_command_candidates(&state.db, guild_id, &command_name)
                        .await
                        .map_err(|e| {
                            tracing::error!("Failed to resolve slash command: {}", e);
                            e
                        })?;

                if let Some(command) = commands.first() {
                    let same_priority: Vec<_> = commands
                        .iter()
                        .filter(|c| c.guild_scoped == command.guild_scoped)
                        .collect();

                    if same_priority.len() > 1 {
                        let bot_ids: Vec<Uuid> =
                            same_priority.iter().filter_map(|c| c.bot_user_id).collect();
                        let bot_names = queries::list_bot_display_names(&state.db, &bot_ids)
                            .await
                            .unwrap_or_default();

                        let names = if bot_names.is_empty() {
                            "multiple bots".to_string()
                        } else {
                            bot_names.join(", ")
                        };
                        return Err(ChatError::Validation(format!(
                            "Command '/{command_name}' is ambiguous: provided by {names}"
                        )));
                    }

                    if let Some(bot_user_id) = command.bot_user_id {
                        let mut option_map = serde_json::Map::new();
                        let args: Vec<String> = parts.map(str::to_string).collect();

                        if let Some(options) = command.options.as_ref().and_then(|v| v.as_array()) {
                            for (idx, option_def) in options.iter().enumerate() {
                                if let Some(name) = option_def.get("name").and_then(|n| n.as_str())
                                {
                                    if let Some(arg) = args.get(idx) {
                                        option_map.insert(
                                            name.to_string(),
                                            serde_json::Value::String(arg.clone()),
                                        );
                                    }
                                }
                            }
                        }

                        let interaction_id = Uuid::new_v4();
                        let event = crate::ws::bot_gateway::BotServerEvent::CommandInvoked {
                            interaction_id,
                            command_name: command_name.clone(),
                            guild_id: Some(guild_id),
                            channel_id,
                            user_id: auth_user.id,
                            options: serde_json::Value::Object(option_map),
                        };

                        let payload = serde_json::to_string(&event).map_err(|e| {
                            warn!(error = %e, "Failed to serialize slash command payload");
                            ChatError::Validation("Invalid slash command payload".to_string())
                        })?;

                        let owner_key = format!("interaction:{interaction_id}:owner");
                        let owner_value = bot_user_id.to_string();
                        let routing_redis = db::create_redis_client(&state.config.redis_url)
                            .await
                            .map_err(|e| {
                                warn!(
                                    error = %e,
                                    "Failed to create Redis client for slash command routing"
                                );
                                ChatError::Validation("Bot command routing unavailable".to_string())
                            })?;

                        routing_redis
                            .set::<(), _, _>(
                                &owner_key,
                                owner_value,
                                Some(fred::types::Expiration::EX(300)),
                                None,
                                false,
                            )
                            .await
                            .map_err(|e| {
                                warn!(error = %e, "Failed to store command interaction owner");
                                ChatError::Validation("Bot command routing unavailable".to_string())
                            })?;

                        // Store interaction context for response delivery
                        let context_key = format!("interaction:{interaction_id}:context");
                        let context_data = serde_json::json!({
                            "user_id": auth_user.id,
                            "channel_id": channel_id,
                            "guild_id": guild_id,
                            "command_name": command_name,
                        });
                        routing_redis
                            .set::<(), _, _>(
                                &context_key,
                                context_data.to_string(),
                                Some(fred::types::Expiration::EX(300)),
                                None,
                                false,
                            )
                            .await
                            .map_err(|e| {
                                warn!(error = %e, "Failed to store interaction context");
                                ChatError::Validation("Bot command routing unavailable".to_string())
                            })?;

                        routing_redis
                            .publish::<(), _, _>(format!("bot:{bot_user_id}"), payload)
                            .await
                            .map_err(|e| {
                                warn!(error = %e, "Failed to publish slash command invocation");
                                ChatError::Validation("Bot command routing unavailable".to_string())
                            })?;

                        // Dispatch command.invoked to webhooks (non-blocking)
                        {
                            let wh_db = state.db.clone();
                            let wh_redis = state.redis.clone();
                            let wh_app_id = command.application_id;
                            let wh_payload = serde_json::json!({
                                "interaction_id": interaction_id,
                                "command_name": command_name,
                                "guild_id": guild_id,
                                "channel_id": channel_id,
                                "user_id": auth_user.id,
                            });
                            tokio::spawn(async move {
                                crate::webhooks::dispatch::dispatch_command_event(
                                    &wh_db, &wh_redis, wh_app_id, wh_payload,
                                )
                                .await;
                            });
                        }

                        // Spawn timeout relay
                        {
                            let timeout_redis = state.redis.clone();
                            let invoker_id = auth_user.id;
                            let cmd_name = command_name.clone();
                            let ch_id = channel_id;
                            let iid = interaction_id;
                            tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                let response_key = format!("interaction:{iid}:response");
                                let exists: bool =
                                    timeout_redis.exists(&response_key).await.unwrap_or(false);
                                if !exists {
                                    let event = crate::ws::ServerEvent::CommandResponseTimeout {
                                        interaction_id: iid,
                                        command_name: cmd_name,
                                        channel_id: ch_id,
                                    };
                                    let _ = crate::ws::broadcast_to_user(
                                        &timeout_redis,
                                        invoker_id,
                                        &event,
                                    )
                                    .await;
                                }
                            });
                        }

                        let author = db::find_user_by_id(&state.db, auth_user.id)
                            .await?
                            .map(AuthorProfile::from)
                            .unwrap_or_else(|| AuthorProfile {
                                id: auth_user.id,
                                username: "unknown".to_string(),
                                display_name: "Unknown User".to_string(),
                                avatar_url: None,
                                status: "offline".to_string(),
                            });

                        let accepted = MessageResponse {
                            id: Uuid::new_v4(),
                            channel_id,
                            author,
                            content: body.content.clone(),
                            encrypted: false,
                            attachments: vec![],
                            reply_to: None,
                            parent_id: None,
                            thread_reply_count: 0,
                            thread_last_reply_at: None,
                            edited_at: None,
                            created_at: Utc::now(),
                            mention_type: None,
                            reactions: None,
                            thread_info: None,
                            pinned: false,
                            message_type: "user".to_string(),
                            nonce: body.nonce.clone(),
                            embeds: None,
                            components: None,
                        };

                        return Ok((StatusCode::ACCEPTED, Json(accepted)));
                    }
                }
            }
        }
    }

    // Check threads_enabled for guild channels when creating a thread reply
    if body.parent_id.is_some() {
        if let Some(guild_id) = channel.guild_id {
            let threads_enabled = queries::is_guild_threads_enabled(&state.db, guild_id).await?;
            if !threads_enabled {
                return Err(ChatError::Forbidden);
            }
        }
    }

    // Validate parent_id if provided (thread reply)
    if let Some(parent_id) = body.parent_id {
        let parent_msg = db::find_message_by_id(&state.db, parent_id)
            .await?
            .ok_or_else(|| ChatError::Validation("Thread parent not found".to_string()))?;

        // Ensure parent is a top-level message (no nested threads)
        if parent_msg.parent_id.is_some() {
            return Err(ChatError::Validation(
                "Cannot reply to a thread reply (no nested threads)".to_string(),
            ));
        }

        // Ensure parent is in the same channel
        if parent_msg.channel_id != channel_id {
            return Err(ChatError::Validation(
                "Thread parent must be in the same channel".to_string(),
            ));
        }
    }

    // Create message (either regular or thread reply)
    let message = if let Some(parent_id) = body.parent_id {
        db::create_thread_reply(
            &state.db,
            db::CreateThreadReplyParams {
                parent_id,
                channel_id,
                user_id: auth_user.id,
                content: &body.content,
                encrypted: body.encrypted,
                nonce: body.nonce.as_deref(),
                reply_to: body.reply_to,
            },
        )
        .await?
    } else {
        db::create_message(
            &state.db,
            channel_id,
            auth_user.id,
            &body.content,
            body.encrypted,
            body.nonce.as_deref(),
            body.reply_to,
        )
        .await?
    };

    // Get author profile for response (capture the bot flag before converting).
    let author_user = db::find_user_by_id(&state.db, auth_user.id).await?;
    let author_is_bot = author_user.as_ref().map(|u| u.is_bot).unwrap_or(false);
    let author = author_user
        .map(AuthorProfile::from)
        .unwrap_or_else(|| AuthorProfile {
            id: auth_user.id,
            username: "unknown".to_string(),
            display_name: "Unknown User".to_string(),
            avatar_url: None,
            status: "offline".to_string(),
        });

    // Rich embeds: bot-authored only. Validate against size/URL caps, persist,
    // and reflect on the response. A human user sending embeds is rejected.
    let embeds_json = if let Some(mut embeds) = body.embeds.clone() {
        if !author_is_bot {
            return Err(ChatError::Forbidden);
        }
        crate::chat::embeds::validate_embeds(&mut embeds)
            .map_err(|e| ChatError::Validation(e.to_string()))?;
        let json = serde_json::to_value(&embeds).unwrap_or(serde_json::Value::Null);
        db::set_message_embeds(&state.db, message.id, Some(&json)).await?;
        Some(embeds)
    } else {
        None
    };

    // Interactive components: bot-authored only, same gating + validation.
    let components_json = if let Some(components) = body.components.clone() {
        if !author_is_bot {
            return Err(ChatError::Forbidden);
        }
        crate::chat::components::validate_components(&components)
            .map_err(|e| ChatError::Validation(e.to_string()))?;
        let json = serde_json::to_value(&components).unwrap_or(serde_json::Value::Null);
        db::set_message_components(&state.db, message.id, Some(&json)).await?;
        Some(components)
    } else {
        None
    };

    // Detect mentions (skip for encrypted messages)
    let mention_type = if message.encrypted {
        None
    } else {
        detect_mention_type(&message.content, Some(&author.username))
    };

    let mut response = MessageResponse {
        id: message.id,
        channel_id: message.channel_id,
        author: author.clone(),
        content: message.content,
        encrypted: message.encrypted,
        attachments: vec![],
        reply_to: message.reply_to,
        parent_id: message.parent_id,
        thread_reply_count: message.thread_reply_count,
        thread_last_reply_at: message.thread_last_reply_at,
        edited_at: message.edited_at,
        created_at: message.created_at,
        mention_type,
        reactions: None,
        thread_info: None,
        pinned: false,
        message_type: message.message_type,
        nonce: None,
        embeds: embeds_json.clone(),
        components: components_json.clone(),
    };

    // Broadcast via Redis pub-sub (nonce excluded — it's only for the sender)
    let message_json = serde_json::to_value(&response).unwrap_or_default();

    // Set nonce for the HTTP response to the sender
    response.nonce = message.nonce;

    if let Some(parent_id) = body.parent_id {
        // If this reply is on a forum post's root message, bump its activity so
        // the forum card list re-sorts by recency (no-op for non-forum threads).
        if let Err(e) = sqlx::query(
            "UPDATE forum_posts SET last_activity_at = NOW() WHERE root_message_id = $1",
        )
        .bind(parent_id)
        .execute(&state.db)
        .await
        {
            tracing::warn!("Failed to bump forum post activity: {e}");
        }

        // Thread reply: broadcast ThreadReplyNew with updated thread info
        let thread_info = build_thread_info(&state.db, parent_id).await;
        let thread_info_json = serde_json::to_value(&thread_info).unwrap_or_default();

        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::ThreadReplyNew {
                channel_id,
                parent_id,
                message: message_json,
                thread_info: thread_info_json,
            },
        )
        .await
        {
            warn!(channel_id = %channel_id, parent_id = %parent_id, error = %e, "Failed to broadcast thread reply event");
        }
    } else {
        // Regular message: broadcast MessageNew
        if let Err(e) = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::MessageNew {
                channel_id,
                message: message_json,
            },
        )
        .await
        {
            warn!(channel_id = %channel_id, error = %e, "Failed to broadcast new message event");
        }
    }

    // Dispatch to bot ecosystem (non-blocking, fire-and-forget)
    if let Some(guild_id) = channel.guild_id {
        if !body.encrypted {
            let db = state.db.clone();
            let redis = state.redis.clone();
            let msg_id = message.id;
            let ch_id = channel_id;
            let uid = auth_user.id;
            let content = body.content.clone();
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

    Ok((StatusCode::CREATED, Json(response)))
}

/// Update (edit) a message.
/// PATCH /api/messages/:id
#[utoipa::path(
    patch,
    path = "/api/messages/{id}",
    tag = "messages",
    params(("id" = Uuid, Path, description = "Message ID")),
    request_body = UpdateMessageRequest,
    responses(
        (status = 200, body = MessageResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body), fields(user_id = %auth_user.id, message_id = %id))]
pub async fn update(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateMessageRequest>,
) -> Result<Json<MessageResponse>, ChatError> {
    // Validate input
    body.validate()
        .map_err(|e| ChatError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Load message to check permissions
    let existing_message = db::find_message_by_id(&state.db, id)
        .await?
        .ok_or(ChatError::MessageNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(
        &state.db,
        auth_user.id,
        existing_message.channel_id,
    )
    .await
    .map_err(|_| ChatError::Forbidden)?;

    // Content filtering on edited content: skip encrypted messages and DMs
    if !existing_message.encrypted {
        let channel = db::find_channel_by_id(&state.db, existing_message.channel_id)
            .await?
            .ok_or(ChatError::ChannelNotFound)?;
        if let Some(guild_id) = channel.guild_id {
            if let Ok(engine) = state.filter_cache.get_or_build(&state.db, guild_id).await {
                let result = engine.check(&body.content);
                if result.blocked {
                    for m in &result.matches {
                        filter_queries::log_moderation_action(
                            &state.db,
                            &filter_queries::LogActionParams {
                                guild_id,
                                user_id: auth_user.id,
                                channel_id: existing_message.channel_id,
                                action: m.action,
                                category: Some(m.category),
                                matched_pattern: &m.matched_pattern,
                                original_content: &body.content,
                                custom_pattern_id: m.custom_pattern_id,
                            },
                        )
                        .await
                        .ok();
                    }
                    return Err(ChatError::ContentFiltered);
                }
                // For "log" and "warn" actions, still log but allow the edit
                for m in result
                    .matches
                    .iter()
                    .filter(|m| m.action == FilterAction::Log || m.action == FilterAction::Warn)
                {
                    filter_queries::log_moderation_action(
                        &state.db,
                        &filter_queries::LogActionParams {
                            guild_id,
                            user_id: auth_user.id,
                            channel_id: existing_message.channel_id,
                            action: m.action,
                            category: Some(m.category),
                            matched_pattern: &m.matched_pattern,
                            original_content: &body.content,
                            custom_pattern_id: m.custom_pattern_id,
                        },
                    )
                    .await
                    .ok();
                }
            }
        }
    }

    // Update message (only owner can edit)
    let message = db::update_message(&state.db, id, auth_user.id, &body.content)
        .await?
        .ok_or(ChatError::MessageNotFound)?;

    // Get author profile for response
    let author = db::find_user_by_id(&state.db, auth_user.id)
        .await?
        .map(AuthorProfile::from)
        .unwrap_or_else(|| AuthorProfile {
            id: auth_user.id,
            username: "unknown".to_string(),
            display_name: "Unknown User".to_string(),
            avatar_url: None,
            status: "offline".to_string(),
        });

    // Fetch existing attachments
    let attachments = db::list_file_attachments_by_message(&state.db, message.id)
        .await?
        .iter()
        .map(AttachmentInfo::from_db)
        .collect();

    let response = MessageResponse {
        id: message.id,
        channel_id: message.channel_id,
        author,
        content: message.content.clone(),
        encrypted: message.encrypted,
        attachments,
        reply_to: message.reply_to,
        parent_id: message.parent_id,
        thread_reply_count: message.thread_reply_count,
        thread_last_reply_at: message.thread_last_reply_at,
        edited_at: message.edited_at,
        created_at: message.created_at,
        mention_type: None, // Edits don't trigger new notifications
        reactions: None,
        thread_info: None,
        pinned: queries::is_message_pinned(&state.db, message.channel_id, message.id)
            .await
            .unwrap_or(false),
        message_type: message.message_type.clone(),
        nonce: None,
        embeds: message
            .embeds
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
        components: message
            .components
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok()),
    };

    // Broadcast edit via Redis pub-sub
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        message.channel_id,
        &ServerEvent::MessageEdit {
            channel_id: message.channel_id,
            message_id: message.id,
            content: message.content,
            edited_at: message
                .edited_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        },
    )
    .await
    {
        warn!(channel_id = %message.channel_id, message_id = %message.id, error = %e, "Failed to broadcast message edit event");
    }

    Ok(Json(response))
}

/// Delete a message (soft delete).
/// DELETE /api/messages/:id
#[utoipa::path(
    delete,
    path = "/api/messages/{id}",
    tag = "messages",
    params(("id" = Uuid, Path, description = "Message ID")),
    responses(
        (status = 204, description = "Message deleted"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id, message_id = %id))]
pub async fn delete(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ChatError> {
    // Get message to find channel_id before deleting
    let message = db::find_message_by_id(&state.db, id)
        .await?
        .ok_or(ChatError::MessageNotFound)?;

    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth_user.id, message.channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    // Check ownership (deleted/anonymized messages cannot be modified)
    if message.user_id != Some(auth_user.id) {
        return Err(ChatError::Forbidden);
    }

    let channel_id = message.channel_id;
    let parent_id = message.parent_id;

    // Delete message
    let deleted = db::delete_message(&state.db, id, auth_user.id).await?;

    if deleted {
        // Clean up channel pin if message was pinned
        if matches!(
            queries::delete_channel_pin(&state.db, channel_id, id).await,
            Ok(true)
        ) {
            if let Err(e) = broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::ChannelPinRemoved {
                    channel_id,
                    message_id: id,
                },
            )
            .await
            {
                warn!(channel_id = %channel_id, message_id = %id, error = %e, "Failed to broadcast channel_pin_removed on delete");
            }
        }

        if let Some(parent_id) = parent_id {
            // Thread reply deleted: decrement parent counters and broadcast ThreadReplyDelete
            if let Err(e) = db::decrement_thread_counters(&state.db, parent_id).await {
                warn!(parent_id = %parent_id, error = %e, "Failed to decrement thread counters");
            }

            let thread_info = build_thread_info(&state.db, parent_id).await;
            let thread_info_json = serde_json::to_value(&thread_info).unwrap_or_default();

            if let Err(e) = broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::ThreadReplyDelete {
                    channel_id,
                    parent_id,
                    message_id: id,
                    thread_info: thread_info_json,
                },
            )
            .await
            {
                warn!(channel_id = %channel_id, message_id = %id, error = %e, "Failed to broadcast thread reply delete event");
            }
        } else {
            // Regular message deleted: broadcast MessageDelete
            if let Err(e) = broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::MessageDelete {
                    channel_id,
                    message_id: id,
                },
            )
            .await
            {
                warn!(channel_id = %channel_id, message_id = %id, error = %e, "Failed to broadcast message delete event");
            }
        }

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChatError::MessageNotFound)
    }
}

// ============================================================================
// Shared Helpers
// ============================================================================

/// Bulk-fetch users, attachments, and reactions for a set of messages, then
/// map them into `MessageResponse` objects. Used by both `list` and
/// `list_thread_replies` to avoid duplicating the N+1 avoidance logic.
pub async fn build_message_responses(
    pool: &sqlx::PgPool,
    requesting_user_id: Uuid,
    messages: Vec<db::Message>,
) -> Result<Vec<MessageResponse>, ChatError> {
    if messages.is_empty() {
        return Ok(vec![]);
    }

    // Bulk fetch users (skip messages with no author)
    let user_ids: Vec<Uuid> = messages.iter().filter_map(|m| m.user_id).collect();
    let users = db::find_users_by_ids(pool, &user_ids).await?;
    let user_map: std::collections::HashMap<Uuid, db::User> =
        users.into_iter().map(|u| (u.id, u)).collect();

    // Bulk fetch attachments
    let message_ids: Vec<Uuid> = messages.iter().map(|m| m.id).collect();
    let all_attachments = db::list_file_attachments_by_messages(pool, &message_ids).await?;
    let mut attachment_map: std::collections::HashMap<Uuid, Vec<AttachmentInfo>> =
        std::collections::HashMap::new();
    for attachment in all_attachments {
        attachment_map
            .entry(attachment.message_id)
            .or_default()
            .push(AttachmentInfo::from_db(&attachment));
    }

    // Bulk fetch reactions
    let reactions_data =
        queries::list_reactions_for_messages(pool, requesting_user_id, &message_ids).await?;

    let mut reactions_map: std::collections::HashMap<Uuid, Vec<ReactionInfo>> =
        std::collections::HashMap::new();
    for row in reactions_data {
        reactions_map
            .entry(row.message_id)
            .or_default()
            .push(ReactionInfo {
                emoji: row.emoji,
                count: row.count,
                me: row.me,
                users: row.users,
            });
    }

    // Bulk fetch pin status
    let pinned_ids = queries::list_pinned_message_ids(pool, &message_ids)
        .await
        .unwrap_or_default();

    // Batch-fetch thread info for parent messages with replies
    let parent_ids_with_threads: Vec<Uuid> = messages
        .iter()
        .filter(|m| m.parent_id.is_none() && m.thread_reply_count > 0)
        .map(|m| m.id)
        .collect();

    let mut thread_infos = build_batch_thread_infos(
        pool,
        requesting_user_id,
        &parent_ids_with_threads,
        &messages,
    )
    .await;

    // Build response objects
    let response = messages
        .into_iter()
        .map(|msg| {
            let author = msg
                .user_id
                .and_then(|uid| user_map.get(&uid))
                .map(|u| AuthorProfile::from(u.clone()))
                .unwrap_or_else(|| AuthorProfile {
                    id: msg.user_id.unwrap_or(Uuid::nil()),
                    username: "deleted".to_string(),
                    display_name: "Deleted User".to_string(),
                    avatar_url: None,
                    status: "offline".to_string(),
                });

            let attachments = attachment_map.remove(&msg.id).unwrap_or_default();
            let reactions = reactions_map.remove(&msg.id);
            let mention_type = if msg.encrypted {
                None
            } else {
                detect_mention_type(&msg.content, Some(&author.username))
            };

            let thread_info = thread_infos.remove(&msg.id);

            MessageResponse {
                id: msg.id,
                channel_id: msg.channel_id,
                author,
                content: msg.content,
                encrypted: msg.encrypted,
                attachments,
                reply_to: msg.reply_to,
                parent_id: msg.parent_id,
                thread_reply_count: msg.thread_reply_count,
                thread_last_reply_at: msg.thread_last_reply_at,
                edited_at: msg.edited_at,
                created_at: msg.created_at,
                mention_type,
                reactions,
                thread_info,
                pinned: pinned_ids.contains(&msg.id),
                message_type: msg.message_type.clone(),
                nonce: None,
                embeds: msg
                    .embeds
                    .as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                components: msg
                    .components
                    .as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
            }
        })
        .collect();

    Ok(response)
}

// ============================================================================
// Thread Handlers
// ============================================================================

/// Build thread info for a parent message (participants + counters).
async fn build_thread_info(pool: &sqlx::PgPool, parent_id: Uuid) -> ThreadInfoResponse {
    let participant_ids = db::get_thread_participants(pool, parent_id, 5)
        .await
        .unwrap_or_default();

    // Fetch avatar URLs for participants
    let participants = if participant_ids.is_empty() {
        vec![]
    } else {
        db::find_users_by_ids(pool, &participant_ids)
            .await
            .unwrap_or_default()
    };

    let participant_avatars: Vec<Option<String>> = participant_ids
        .iter()
        .map(|uid| {
            participants
                .iter()
                .find(|u| u.id == *uid)
                .and_then(|u| super::files::maybe_file_url(u.avatar_url.clone()))
        })
        .collect();

    // Re-fetch parent to get updated counters
    let parent = db::find_message_by_id(pool, parent_id).await.ok().flatten();

    ThreadInfoResponse {
        reply_count: parent.as_ref().map_or(0, |p| p.thread_reply_count),
        last_reply_at: parent.and_then(|p| p.thread_last_reply_at),
        participant_ids,
        participant_avatars,
        has_unread: None,
    }
}

/// Batch-build thread info for multiple parent messages.
///
/// Efficiently fetches participants, avatars, read states, and latest replies
/// in bulk queries to avoid N+1 patterns.
async fn build_batch_thread_infos(
    pool: &sqlx::PgPool,
    requesting_user_id: Uuid,
    parent_ids: &[Uuid],
    messages: &[db::Message],
) -> std::collections::HashMap<Uuid, ThreadInfoResponse> {
    if parent_ids.is_empty() {
        return std::collections::HashMap::new();
    }

    // Batch fetch participants (up to 5 per thread)
    let participants_map = match db::get_batch_thread_participants(pool, parent_ids, 5).await {
        Ok(map) => map,
        Err(e) => {
            tracing::warn!(
                parent_count = parent_ids.len(),
                error = %e,
                "Failed to batch-fetch thread participants, avatars will be missing"
            );
            std::collections::HashMap::new()
        }
    };

    // Collect all unique participant user IDs for avatar lookup
    let all_participant_ids: Vec<Uuid> = participants_map
        .values()
        .flatten()
        .copied()
        .collect::<std::collections::HashSet<Uuid>>()
        .into_iter()
        .collect();

    // Single bulk user lookup for all participant avatars
    let users = if all_participant_ids.is_empty() {
        vec![]
    } else {
        match db::find_users_by_ids(pool, &all_participant_ids).await {
            Ok(users) => users,
            Err(e) => {
                tracing::warn!(
                    user_count = all_participant_ids.len(),
                    error = %e,
                    "Failed to fetch thread participant users, avatars will be missing"
                );
                vec![]
            }
        }
    };
    let user_map: std::collections::HashMap<Uuid, &db::User> =
        users.iter().map(|u| (u.id, u)).collect();

    // Batch fetch read states and latest reply IDs for unread detection
    let read_states =
        match db::get_batch_thread_read_states(pool, requesting_user_id, parent_ids).await {
            Ok(states) => states,
            Err(e) => {
                tracing::warn!(
                    parent_count = parent_ids.len(),
                    error = %e,
                    "Failed to batch-fetch thread read states, unread indicators may be inaccurate"
                );
                std::collections::HashMap::new()
            }
        };
    let latest_replies = match db::get_batch_thread_latest_reply_ids(pool, parent_ids).await {
        Ok(replies) => replies,
        Err(e) => {
            tracing::warn!(
                parent_count = parent_ids.len(),
                error = %e,
                "Failed to batch-fetch latest thread replies, unread indicators may be inaccurate"
            );
            std::collections::HashMap::new()
        }
    };

    // Build a map of parent_id -> message data for counters
    let msg_map: std::collections::HashMap<Uuid, &db::Message> =
        messages.iter().map(|m| (m.id, m)).collect();

    // Assemble ThreadInfoResponse per parent
    let mut result = std::collections::HashMap::new();

    for &parent_id in parent_ids {
        let participant_ids = participants_map
            .get(&parent_id)
            .cloned()
            .unwrap_or_default();

        let participant_avatars: Vec<Option<String>> = participant_ids
            .iter()
            .map(|uid| {
                user_map
                    .get(uid)
                    .and_then(|u| super::files::maybe_file_url(u.avatar_url.clone()))
            })
            .collect();

        // Determine unread status
        let has_unread = if let Some(latest_reply_id) = latest_replies.get(&parent_id) {
            match read_states.get(&parent_id) {
                Some(Some(last_read_id)) => Some(last_read_id != latest_reply_id),
                Some(None) => Some(true), // Has read state but no message ID → unread
                None => Some(true),       // No read state at all → unread
            }
        } else {
            None // No replies → no unread state
        };

        let msg = msg_map.get(&parent_id);

        result.insert(
            parent_id,
            ThreadInfoResponse {
                reply_count: msg.map_or(0, |m| m.thread_reply_count),
                last_reply_at: msg.and_then(|m| m.thread_last_reply_at),
                participant_ids,
                participant_avatars,
                has_unread,
            },
        );
    }

    result
}

/// List thread replies for a parent message.
/// `GET /api/messages/{parent_id}/thread`
#[utoipa::path(
    get,
    path = "/api/messages/{parent_id}/thread",
    tag = "messages",
    params(("parent_id" = Uuid, Path, description = "Parent message ID")),
    responses(
        (status = 200, body = CursorPaginatedResponse<MessageResponse>),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id, message_id = %parent_id))]
pub async fn list_thread_replies(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(parent_id): Path<Uuid>,
    Query(query): Query<ListThreadRepliesQuery>,
) -> Result<Json<CursorPaginatedResponse<MessageResponse>>, ChatError> {
    // Verify parent message exists
    let parent = db::find_message_by_id(&state.db, parent_id)
        .await?
        .ok_or(ChatError::MessageNotFound)?;

    // Check channel access
    crate::permissions::require_channel_access(&state.db, auth_user.id, parent.channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    // Load block set for filtering
    let blocked_ids = block_cache::load_blocked_users(&state.db, &state.redis, auth_user.id)
        .await
        .unwrap_or_else(|e| {
            warn!(user_id = %auth_user.id, error = %e, "Failed to load blocked users, message filtering may be incomplete");
            HashSet::default()
        });
    let blocked_by_ids = block_cache::load_blocked_by(&state.db, &state.redis, auth_user.id)
        .await
        .unwrap_or_else(|e| {
            warn!(user_id = %auth_user.id, error = %e, "Failed to load blocked-by users, message filtering may be incomplete");
            HashSet::default()
        });
    let combined_block_set: std::collections::HashSet<Uuid> =
        blocked_ids.union(&blocked_by_ids).copied().collect();

    let limit = query.limit.clamp(1, 100);
    let mut messages =
        db::list_thread_replies(&state.db, parent_id, query.after, limit + 1).await?;

    if !combined_block_set.is_empty() {
        messages.retain(|m| {
            m.user_id
                .is_none_or(|uid| !combined_block_set.contains(&uid))
        });
    }

    let has_more = messages.len() as i64 > limit;
    if has_more {
        messages.pop();
    }

    let response = build_message_responses(&state.db, auth_user.id, messages).await?;

    // For thread replies, cursor is the newest message (ascending order)
    let next_cursor = if has_more {
        response.last().map(|m| m.id)
    } else {
        None
    };

    Ok(Json(CursorPaginatedResponse {
        items: response,
        has_more,
        next_cursor,
    }))
}

/// Mark a thread as read.
/// `POST /api/messages/{parent_id}/thread/read`
#[utoipa::path(
    post,
    path = "/api/messages/{parent_id}/thread/read",
    tag = "messages",
    params(("parent_id" = Uuid, Path, description = "Parent message ID")),
    responses(
        (status = 204, description = "Thread marked as read"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id, message_id = %parent_id))]
pub async fn mark_thread_read(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(parent_id): Path<Uuid>,
) -> Result<StatusCode, ChatError> {
    // Verify parent exists
    let parent = db::find_message_by_id(&state.db, parent_id)
        .await?
        .ok_or(ChatError::MessageNotFound)?;

    // Check channel access
    crate::permissions::require_channel_access(&state.db, auth_user.id, parent.channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    // Get the latest reply to use as last_read_message_id
    let last_reply_id = queries::latest_thread_reply_id(&state.db, parent_id).await?;

    db::update_thread_read_state(&state.db, auth_user.id, parent_id, last_reply_id).await?;

    // Broadcast to user's other sessions
    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth_user.id,
        &ServerEvent::ThreadRead {
            thread_parent_id: parent_id,
            last_read_message_id: last_reply_id,
        },
    )
    .await
    {
        warn!(user_id = %auth_user.id, error = %e, "Failed to broadcast thread read event");
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use sqlx::PgPool;

    use super::*;
    use crate::api::{AppState, AppStateConfig};
    use crate::auth::AuthUser;
    use crate::chat::types::validate_message_content;
    use crate::config::Config;

    #[test]
    fn test_validate_message_content_empty_rejected() {
        let result = validate_message_content("");

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_default()
            .contains("cannot be empty"));
    }

    #[test]
    fn test_validate_message_content_exact_normal_limit_passes() {
        let content = "a".repeat(4000);

        let result = validate_message_content(&content);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_message_content_normal_limit_plus_one_fails() {
        let content = "a".repeat(4001);

        let result = validate_message_content(&content);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_default()
            .contains("cannot exceed 4000"));
    }

    #[test]
    fn test_validate_message_content_with_code_blocks_within_extended_limit_passes() {
        let regular_text = "a".repeat(4000);
        let code_block = format!("```{} ```", "b".repeat(2000));
        let content = format!("{regular_text}{code_block}");

        let result = validate_message_content(&content);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_message_content_with_code_blocks_exceeding_total_limit_fails() {
        let content = format!("{}{}", "a".repeat(2000), "```b```".repeat(1200));

        let result = validate_message_content(&content);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_default()
            .contains("cannot exceed 10000"));
    }

    #[test]
    fn test_validate_message_content_nested_code_blocks_handled() {
        let content = "before ```outer ```inner``` outer``` after";

        let result = validate_message_content(content);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_message_content_unclosed_code_block_counts_as_regular_text() {
        // With an odd number of triple backticks, the regex does not strip the open code block.
        // This means the payload is validated against the 4,000 regular-text limit.
        let content = format!("```{}", "a".repeat(4001));

        let result = validate_message_content(&content);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .message
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_default()
            .contains("cannot exceed 4000"));
    }

    #[test]
    fn test_validate_message_content_only_code_blocks_passes() {
        let content = "```rust\nfn main() { println!(\"ok\"); }\n```";

        let result = validate_message_content(content);

        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_message_content_mixed_text_and_code_blocks_passes() {
        let content = format!(
            "{} ```{} ``` {}",
            "a".repeat(3990),
            "b".repeat(1500),
            "tail"
        );

        let result = validate_message_content(&content);

        assert!(result.is_ok());
    }

    fn test_auth_user(user: &db::User) -> AuthUser {
        AuthUser {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            email: user.email.clone(),
            avatar_url: user.avatar_url.clone(),
            mfa_enabled: false,
            deletion_scheduled_at: None,
        }
    }

    /// Helper to create test app state
    async fn create_test_state(pool: PgPool) -> AppState {
        let config = Config::default_for_test();
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to Redis");

        AppState::new(AppStateConfig {
            db: pool,
            redis,
            config,
            s3: None,
            sfu: crate::voice::SfuServer::new(
                std::sync::Arc::new(Config::default_for_test()),
                None,
            )
            .unwrap(),
            rate_limiter: None,
            screen_share_limiter: None,
            email: None,
            oidc_manager: None,
            http_client: reqwest::Client::new(),
        })
    }

    /// Helper to create a guild with proper permissions for testing
    async fn create_test_guild_with_permissions(
        pool: &PgPool,
        owner_id: Uuid,
        permissions: i64,
    ) -> Uuid {
        // Create guild
        let guild_id = Uuid::new_v4();
        sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
            .bind(guild_id)
            .bind("Test Guild")
            .bind(owner_id)
            .execute(pool)
            .await
            .expect("Failed to create test guild");

        // Add owner as member
        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(guild_id)
            .bind(owner_id)
            .execute(pool)
            .await
            .expect("Failed to add guild member");

        // Create @everyone role with specified permissions
        let everyone_role_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
             VALUES ($1, $2, '@everyone', $3, 0, true)",
        )
        .bind(everyone_role_id)
        .bind(guild_id)
        .bind(permissions)
        .execute(pool)
        .await
        .expect("Failed to create @everyone role");

        // Assign @everyone role to owner
        sqlx::query(
            "INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
        )
        .bind(guild_id)
        .bind(owner_id)
        .bind(everyone_role_id)
        .execute(pool)
        .await
        .expect("Failed to assign role to member");

        guild_id
    }

    /// Helper to add a user to an existing guild
    async fn add_user_to_guild(pool: &PgPool, guild_id: Uuid, user_id: Uuid) {
        // Add as guild member
        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(guild_id)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("Failed to add guild member");

        // Get @everyone role
        let everyone_role: (Uuid,) =
            sqlx::query_as("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default = true")
                .bind(guild_id)
                .fetch_one(pool)
                .await
                .expect("Failed to get @everyone role");

        // Assign @everyone role
        sqlx::query(
            "INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(everyone_role.0)
        .execute(pool)
        .await
        .expect("Failed to assign role to member");
    }

    #[sqlx::test]
    async fn test_list_messages_with_multiple_users(pool: PgPool) {
        use crate::permissions::GuildPermissions;
        let state = create_test_state(pool.clone()).await;

        // Create two users
        let user1 = db::create_user(&pool, "user1", "User One", None, "hash1")
            .await
            .expect("Failed to create user1");

        let user2 = db::create_user(&pool, "user2", "User Two", None, "hash2")
            .await
            .expect("Failed to create user2");

        // Create guild with VIEW_CHANNEL permission (user1 as owner)
        let guild_id = create_test_guild_with_permissions(
            &pool,
            user1.id,
            GuildPermissions::VIEW_CHANNEL.bits() as i64,
        )
        .await;

        // Add user2 to the guild
        add_user_to_guild(&pool, guild_id, user2.id).await;

        // Create a channel
        let channel = db::create_channel(
            &pool,
            db::CreateChannelParams {
                name: "test-channel",
                channel_type: &db::ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        // Create 5 messages: 3 from user1, 2 from user2
        let msg1 = db::create_message(&pool, channel.id, user1.id, "Message 1", false, None, None)
            .await
            .expect("Failed to create message 1");

        let msg2 = db::create_message(&pool, channel.id, user2.id, "Message 2", false, None, None)
            .await
            .expect("Failed to create message 2");

        let msg3 = db::create_message(&pool, channel.id, user1.id, "Message 3", false, None, None)
            .await
            .expect("Failed to create message 3");

        let msg4 = db::create_message(&pool, channel.id, user1.id, "Message 4", false, None, None)
            .await
            .expect("Failed to create message 4");

        let msg5 = db::create_message(&pool, channel.id, user2.id, "Message 5", false, None, None)
            .await
            .expect("Failed to create message 5");

        // Call the list handler
        let query = ListMessagesQuery {
            before: None,
            around: None,
            limit: 50,
        };

        let result = list(
            State(state),
            test_auth_user(&user1),
            Path(channel.id),
            Query(query),
        )
        .await
        .expect("Handler failed");

        let messages = &result.0.items;

        // Assert correct number of messages
        assert_eq!(messages.len(), 5, "Should return 5 messages");

        // Verify messages are in reverse chronological order (newest first)
        assert_eq!(messages[0].id, msg5.id);
        assert_eq!(messages[1].id, msg4.id);
        assert_eq!(messages[2].id, msg3.id);
        assert_eq!(messages[3].id, msg2.id);
        assert_eq!(messages[4].id, msg1.id);

        // Verify author information is correctly populated
        // Message 5 (from user2)
        assert_eq!(messages[0].author.id, user2.id);
        assert_eq!(messages[0].author.username, "user2");
        assert_eq!(messages[0].author.display_name, "User Two");
        assert_eq!(messages[0].content, "Message 5");

        // Message 4 (from user1)
        assert_eq!(messages[1].author.id, user1.id);
        assert_eq!(messages[1].author.username, "user1");
        assert_eq!(messages[1].author.display_name, "User One");
        assert_eq!(messages[1].content, "Message 4");

        // Message 3 (from user1)
        assert_eq!(messages[2].author.id, user1.id);
        assert_eq!(messages[2].author.username, "user1");

        // Message 2 (from user2)
        assert_eq!(messages[3].author.id, user2.id);
        assert_eq!(messages[3].author.username, "user2");

        // Message 1 (from user1)
        assert_eq!(messages[4].author.id, user1.id);
        assert_eq!(messages[4].author.username, "user1");

        // Verify no N+1 query issue - this test would timeout if there was one
        // The handler should use bulk fetching, making only 2 queries total:
        // 1. Fetch messages
        // 2. Bulk fetch users
    }

    #[sqlx::test]
    async fn test_list_messages_with_deleted_user(pool: PgPool) {
        use crate::permissions::GuildPermissions;
        let state = create_test_state(pool.clone()).await;

        // Create user
        let user = db::create_user(&pool, "deleteuser", "Delete User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create guild with VIEW_CHANNEL permission
        let guild_id = create_test_guild_with_permissions(
            &pool,
            user.id,
            GuildPermissions::VIEW_CHANNEL.bits() as i64,
        )
        .await;

        // Create channel
        let channel = db::create_channel(
            &pool,
            db::CreateChannelParams {
                name: "test-channel",
                channel_type: &db::ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        // Create message
        let _msg = db::create_message(
            &pool,
            channel.id,
            user.id,
            "Message before delete",
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create message");

        // Delete the user (CASCADE will also delete messages)
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .expect("Failed to delete user");

        // Call the list handler
        let query = ListMessagesQuery {
            before: None,
            around: None,
            limit: 50,
        };

        let result = list(
            State(state),
            test_auth_user(&user),
            Path(channel.id),
            Query(query),
        )
        .await;

        // Due to CASCADE DELETE, when user is deleted:
        // 1. Guild is deleted (user is owner)
        // 2. Channel is deleted (guild is deleted)
        // 3. Messages are deleted (channel is deleted)
        // So the handler should return ChannelNotFound error
        assert!(result.is_err(), "Should fail with ChannelNotFound");
        match result.unwrap_err() {
            ChatError::ChannelNotFound => {
                // Expected - channel was CASCADE deleted
            }
            other => panic!("Expected ChannelNotFound, got: {other:?}"),
        }
    }

    #[sqlx::test]
    async fn test_list_messages_pagination(pool: PgPool) {
        use crate::permissions::GuildPermissions;
        let state = create_test_state(pool.clone()).await;

        // Create user
        let user = db::create_user(&pool, "paguser", "Pag User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create guild with VIEW_CHANNEL permission
        let guild_id = create_test_guild_with_permissions(
            &pool,
            user.id,
            GuildPermissions::VIEW_CHANNEL.bits() as i64,
        )
        .await;

        // Create channel
        let channel = db::create_channel(
            &pool,
            db::CreateChannelParams {
                name: "test-channel",
                channel_type: &db::ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        // Create 10 messages
        for i in 1..=10 {
            db::create_message(
                &pool,
                channel.id,
                user.id,
                &format!("Message {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("Failed to create message");
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }

        // Fetch first page (limit 3)
        let query1 = ListMessagesQuery {
            before: None,
            around: None,
            limit: 3,
        };

        let auth = test_auth_user(&user);
        let result1 = list(
            State(state.clone()),
            auth.clone(),
            Path(channel.id),
            Query(query1),
        )
        .await
        .expect("First page failed");

        let page1 = &result1.0.items;
        assert_eq!(page1.len(), 3, "First page should have 3 messages");
        assert!(result1.0.has_more, "Should indicate more messages exist");
        assert!(
            result1.0.next_cursor.is_some(),
            "Should provide next cursor"
        );

        // Fetch second page using cursor
        let oldest_from_page1 = page1.last().unwrap().id;
        assert_eq!(
            result1.0.next_cursor,
            Some(oldest_from_page1),
            "next_cursor should be the oldest message ID"
        );

        let query2 = ListMessagesQuery {
            before: Some(oldest_from_page1),
            around: None,
            limit: 3,
        };

        let result2 = list(
            State(state.clone()),
            auth.clone(),
            Path(channel.id),
            Query(query2),
        )
        .await
        .expect("Second page failed");

        let page2 = &result2.0.items;

        // Should have at least some messages
        assert!(!page2.is_empty(), "Second page should have messages");
        assert!(page2.len() <= 3, "Second page should respect limit");

        // Verify no overlap - oldest_from_page1 should not appear in page2
        let page2_ids: Vec<Uuid> = page2.iter().map(|m| m.id).collect();
        assert!(
            !page2_ids.contains(&oldest_from_page1),
            "Cursor message should not appear in next page"
        );

        // Verify we can fetch all messages eventually
        let query_all = ListMessagesQuery {
            before: None,
            around: None,
            limit: 100,
        };

        let result_all = list(State(state), auth, Path(channel.id), Query(query_all))
            .await
            .expect("Fetch all failed");

        assert_eq!(
            result_all.0.items.len(),
            10,
            "Should have 10 total messages"
        );
        assert!(
            !result_all.0.has_more,
            "All messages fetched, should have no more"
        );
        assert!(
            result_all.0.next_cursor.is_none(),
            "No more pages, cursor should be None"
        );
    }

    #[sqlx::test]
    async fn test_list_messages_empty_channel(pool: PgPool) {
        use crate::permissions::GuildPermissions;
        let state = create_test_state(pool.clone()).await;

        let user = db::create_user(&pool, "emptyuser", "Empty User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create guild with VIEW_CHANNEL permission
        let guild_id = create_test_guild_with_permissions(
            &pool,
            user.id,
            GuildPermissions::VIEW_CHANNEL.bits() as i64,
        )
        .await;

        // Create channel with no messages
        let channel = db::create_channel(
            &pool,
            db::CreateChannelParams {
                name: "empty-channel",
                channel_type: &db::ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let query = ListMessagesQuery {
            before: None,
            around: None,
            limit: 50,
        };

        let result = list(
            State(state),
            test_auth_user(&user),
            Path(channel.id),
            Query(query),
        )
        .await
        .expect("Handler failed");

        let messages = &result.0.items;
        assert_eq!(messages.len(), 0, "Empty channel should return 0 messages");
    }

    #[sqlx::test]
    async fn test_authorprofile_from_user_status_formatting(pool: PgPool) {
        // Test that the From<User> impl correctly formats status

        // Create user with different status
        let user = db::create_user(&pool, "statususer", "Status User", None, "hash")
            .await
            .expect("Failed to create user");

        // User starts as Offline by default
        let profile = AuthorProfile::from(user.clone());
        assert_eq!(profile.status, "offline");
        assert_eq!(profile.username, "statususer");
        assert_eq!(profile.display_name, "Status User");
        assert_eq!(profile.id, user.id);

        // Update user status to Online
        sqlx::query("UPDATE users SET status = 'online' WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .expect("Failed to update status");

        let updated_user = db::find_user_by_id(&pool, user.id)
            .await
            .expect("Failed to find user")
            .expect("User not found");

        let profile2 = AuthorProfile::from(updated_user);
        assert_eq!(profile2.status, "online");
    }

    #[sqlx::test]
    async fn test_list_messages_limit_clamping(pool: PgPool) {
        use crate::permissions::GuildPermissions;
        let state = create_test_state(pool.clone()).await;

        let user = db::create_user(&pool, "clampuser", "Clamp User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create guild with VIEW_CHANNEL permission
        let guild_id = create_test_guild_with_permissions(
            &pool,
            user.id,
            GuildPermissions::VIEW_CHANNEL.bits() as i64,
        )
        .await;

        let channel = db::create_channel(
            &pool,
            db::CreateChannelParams {
                name: "test-channel",
                channel_type: &db::ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        // Create 10 messages
        for i in 1..=10 {
            db::create_message(
                &pool,
                channel.id,
                user.id,
                &format!("Msg {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("Failed to create message");
        }

        // Test limit = 0 (should clamp to 1)
        let query_zero = ListMessagesQuery {
            before: None,
            around: None,
            limit: 0,
        };

        let auth = test_auth_user(&user);
        let result_zero = list(
            State(state.clone()),
            auth.clone(),
            Path(channel.id),
            Query(query_zero),
        )
        .await
        .expect("Handler failed");

        assert_eq!(result_zero.0.items.len(), 1, "Limit 0 should clamp to 1");

        // Test limit = 200 (should clamp to 100)
        let query_large = ListMessagesQuery {
            before: None,
            around: None,
            limit: 200,
        };

        let result_large = list(State(state), auth, Path(channel.id), Query(query_large))
            .await
            .expect("Handler failed");

        // Should return all 10 messages (max available), not more than 100
        assert_eq!(
            result_large.0.items.len(),
            10,
            "Should return all available messages"
        );
    }
}
