//! Bot Gateway WebSocket
//!
//! Dedicated WebSocket endpoint for bot applications with separate event handling
//! and rate limiting from the user gateway.

use argon2::password_hash::{PasswordHash, PasswordVerifier};
use argon2::Argon2;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use fred::interfaces::{ClientLike, EventInterface, KeysInterface, PubsubInterface};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::ratelimit::RateLimitCategory;

/// Events that bots can send to the server.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BotClientEvent {
    /// Send a message to a channel.
    MessageCreate {
        /// Channel ID to send to.
        channel_id: Uuid,
        /// Message content.
        content: String,
    },
    /// Respond to a slash command invocation.
    CommandResponse {
        /// Interaction ID (from `CommandInvoked` event).
        interaction_id: Uuid,
        /// Response content.
        content: String,
        /// Whether the response is ephemeral (only visible to invoker).
        ephemeral: bool,
    },
}

/// Events that the server sends to bots.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BotServerEvent {
    /// A slash command was invoked.
    CommandInvoked {
        /// Unique interaction ID for this invocation.
        interaction_id: Uuid,
        /// Command name.
        command_name: String,
        /// Guild where command was invoked (null for DM commands).
        guild_id: Option<Uuid>,
        /// Channel where command was invoked.
        channel_id: Uuid,
        /// User who invoked the command.
        user_id: Uuid,
        /// Command options/arguments.
        options: serde_json::Value,
    },
    /// A message was created in a channel the bot has access to.
    MessageCreated {
        /// Message ID.
        message_id: Uuid,
        /// Channel ID.
        channel_id: Uuid,
        /// Guild ID (null for DMs).
        guild_id: Option<Uuid>,
        /// Author user ID.
        user_id: Uuid,
        /// Message content.
        content: String,
    },
    /// Bot was added to a guild.
    GuildJoined {
        /// Guild ID.
        guild_id: Uuid,
        /// Guild name.
        guild_name: String,
    },
    /// Bot was removed from a guild.
    GuildLeft {
        /// Guild ID.
        guild_id: Uuid,
    },
    /// A member joined a guild the bot is installed in.
    MemberJoined {
        /// Guild ID.
        guild_id: Uuid,
        /// User ID of the new member.
        user_id: Uuid,
        /// Username of the new member.
        username: String,
        /// Display name of the new member.
        display_name: String,
    },
    /// A member left a guild the bot is installed in.
    MemberLeft {
        /// Guild ID.
        guild_id: Uuid,
        /// User ID of the departing member.
        user_id: Uuid,
    },
    /// Error occurred.
    Error {
        /// Error code.
        code: String,
        /// Error message.
        message: String,
    },
}

/// Authenticate bot token and return bot user ID and application ID.
///
/// Token format: `bot_user_id.secret` to enable indexed lookup
#[instrument(skip(pool, token))]
async fn authenticate_bot_token(
    pool: &PgPool,
    token: &str,
) -> Result<(Uuid, Uuid), (StatusCode, String)> {
    // Parse token format: "bot_user_id.secret"
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 2 {
        return Err((StatusCode::UNAUTHORIZED, "Invalid token format".to_string()));
    }

    let bot_user_id = Uuid::parse_str(parts[0])
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid token format".to_string()))?;

    // Look up the specific bot application (indexed query)
    let app = sqlx::query!(
        r#"
        SELECT id, token_hash
        FROM bot_applications
        WHERE bot_user_id = $1 AND token_hash IS NOT NULL
        "#,
        bot_user_id
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        error!("Database error during bot auth: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error".to_string(),
        )
    })?
    .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid bot token".to_string()))?;

    // Verify the token hash (constant-time operation)
    let token_hash_str = app
        .token_hash
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Invalid bot token".to_string()))?;

    let parsed_hash = PasswordHash::new(&token_hash_str).map_err(|e| {
        error!("Failed to parse token hash: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal server error".to_string(),
        )
    })?;

    if Argon2::default()
        .verify_password(token.as_bytes(), &parsed_hash)
        .is_err()
    {
        return Err((StatusCode::UNAUTHORIZED, "Invalid bot token".to_string()));
    }

    Ok((bot_user_id, app.id))
}

/// Extract bot token from WebSocket upgrade request.
fn extract_bot_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bot "))
        .map(|s| s.to_string())
}

/// Query parameters for the bot gateway.
#[derive(Debug, Deserialize)]
pub struct BotGatewayQuery {
    /// Comma-separated list of intents (e.g., `messages,members,commands`).
    #[serde(default)]
    pub intents: Option<String>,
}

/// Bot gateway WebSocket handler.
#[instrument(skip(state, ws, headers, query))]
pub async fn bot_gateway_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<BotGatewayQuery>,
) -> Result<Response, (StatusCode, String)> {
    // Extract token from headers
    let token = extract_bot_token(&headers).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization header (expected: `Bot <token>`)".to_string(),
        )
    })?;

    // Authenticate bot
    let (bot_user_id, application_id) = authenticate_bot_token(&state.db, &token).await?;

    // Parse intents from query parameter
    let intents: Vec<String> = if let Some(intents_str) = query.intents {
        intents_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        // Default: commands only (backward compatible)
        vec!["commands".to_string()]
    };

    info!(
        bot_user_id = %bot_user_id,
        application_id = %application_id,
        ?intents,
        "Bot authenticated for gateway"
    );

    // Upgrade to WebSocket
    Ok(ws.on_upgrade(move |socket| handle_bot_socket(socket, state, bot_user_id, intents)))
}

/// Check if an event should be forwarded based on declared intents.
fn intent_permits_event(intents: &[String], event: &BotServerEvent) -> bool {
    match event {
        // Commands are always forwarded (default intent)
        BotServerEvent::CommandInvoked { .. } => true,
        // Messages require "messages" intent
        BotServerEvent::MessageCreated { .. } => intents.iter().any(|i| i == "messages"),
        // Member events require "members" intent
        BotServerEvent::MemberJoined { .. } | BotServerEvent::MemberLeft { .. } => {
            intents.iter().any(|i| i == "members")
        }
        // Lifecycle and error events are always forwarded
        BotServerEvent::GuildJoined { .. }
        | BotServerEvent::GuildLeft { .. }
        | BotServerEvent::Error { .. } => true,
    }
}

/// Handle bot WebSocket connection.
async fn handle_bot_socket(
    socket: WebSocket,
    state: AppState,
    bot_user_id: Uuid,
    intents: Vec<String>,
) {
    let (mut sender, mut receiver) = socket.split();

    // Create Redis subscriber for bot events
    let redis_client = state.redis.clone();
    let bot_channel = format!("bot:{bot_user_id}");

    // Spawn subscriber task
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<BotServerEvent>();
    let subscriber_intents = intents.clone();
    let subscriber_handle = tokio::spawn(async move {
        let subscriber = redis_client.clone_new();
        let _connect_handle = subscriber.connect();

        if let Err(e) = subscriber.wait_for_connect().await {
            error!("Bot subscriber connection failed: {}", e);
            return;
        }

        let mut pubsub_stream = subscriber.message_rx();

        // Subscribe to bot's Redis channel
        if let Err(e) = subscriber.subscribe(&bot_channel).await {
            error!("Failed to subscribe to bot channel: {}", e);
            return;
        }

        info!("Bot subscribed to Redis channel: {}", bot_channel);

        // Listen for messages
        while let Ok(message) = pubsub_stream.recv().await {
            if let Some(raw) = message.value.as_bytes() {
                match String::from_utf8(raw.to_vec()) {
                    Ok(payload) => match serde_json::from_str::<BotServerEvent>(&payload) {
                        Ok(event) => {
                            // Filter events based on declared intents
                            if !intent_permits_event(&subscriber_intents, &event) {
                                continue;
                            }
                            if tx.send(event).is_err() {
                                break; // Receiver dropped, connection closed
                            }
                        }
                        Err(e) => {
                            warn!("Failed to deserialize bot event: {}", e);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to decode bot event payload as UTF-8: {}", e);
                    }
                }
            }
        }
    });

    // Handle incoming messages from bot
    let state_clone = state.clone();
    let (error_tx, mut error_rx) = tokio::sync::mpsc::unbounded_channel::<BotServerEvent>();
    let bot_receiver = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                match serde_json::from_str::<BotClientEvent>(&text) {
                    Ok(event) => {
                        if let Err(e) = handle_bot_event(event, &state_clone, bot_user_id).await {
                            error!("Error handling bot event: {}", e);
                            let _ = error_tx.send(BotServerEvent::Error {
                                code: "handler_error".to_string(),
                                message: e,
                            });
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse bot message: {}", e);
                        let _ = error_tx.send(BotServerEvent::Error {
                            code: "invalid_json".to_string(),
                            message: format!("Failed to parse message: {e}"),
                        });
                    }
                }
            }
        }
    });

    // Forward events to bot (both Redis events and error events)
    loop {
        let event = tokio::select! {
            Some(evt) = rx.recv() => evt,
            Some(err_evt) = error_rx.recv() => err_evt,
            else => break,
        };

        match serde_json::to_string(&event) {
            Ok(json) => {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break; // Connection closed
                }
            }
            Err(e) => {
                error!("Failed to serialize bot event: {}", e);
            }
        }
    }

    // Cleanup
    bot_receiver.abort();
    subscriber_handle.abort();
    info!(bot_user_id = %bot_user_id, "Bot disconnected from gateway");
}

/// Handle events from bot.
#[instrument(skip(state))]
async fn handle_bot_event(
    event: BotClientEvent,
    state: &AppState,
    bot_user_id: Uuid,
) -> Result<(), String> {
    if let Some(rate_limiter) = &state.rate_limiter {
        let identifier = format!("bot_ws:{bot_user_id}");
        let rate_result = rate_limiter
            .check(RateLimitCategory::WsMessage, &identifier)
            .await
            .map_err(|e| {
                warn!(error = %e, bot_user_id = %bot_user_id, "Bot WS rate limit check failed");
                "Bot rate limiting unavailable".to_string()
            })?;

        if !rate_result.allowed {
            return Err(format!(
                "Rate limit exceeded; retry after {} seconds",
                rate_result.retry_after
            ));
        }
    }

    match event {
        BotClientEvent::MessageCreate {
            channel_id,
            content,
        } => {
            // Validate content length
            if let Err(e) = crate::chat::messages::validate_message_content(&content) {
                return Err(e.message.as_ref().map(|m| m.to_string()).unwrap_or_else(|| "Invalid message content".to_string()));
            }

            info!(
                bot_user_id = %bot_user_id,
                channel_id = %channel_id,
                "Bot sending message"
            );

            // Check if bot has access to the channel (is a member)
            let has_access = sqlx::query!(
                r#"
                SELECT 1 as "exists"
                FROM channel_members
                WHERE channel_id = $1 AND user_id = $2
                "#,
                channel_id,
                bot_user_id
            )
            .fetch_optional(&state.db)
            .await
            .map_err(|e| {
                error!("Failed to check channel membership: {e}");
                format!("Failed to verify channel access: {e}")
            })?;

            if has_access.is_none() {
                warn!(
                    bot_user_id = %bot_user_id,
                    channel_id = %channel_id,
                    "Bot attempted to send message to unauthorized channel"
                );
                return Err("Bot is not a member of this channel".to_string());
            }

            // Create message as bot user
            let message = crate::db::create_message(
                &state.db,
                channel_id,
                bot_user_id,
                &content,
                false, // Not encrypted (bots send plain text)
                None,  // No nonce
                None,  // No reply_to
            )
            .await
            .map_err(|e| {
                error!("Failed to create bot message: {e}");
                format!("Failed to create message: {e}")
            })?;

            // Broadcast message to channel subscribers
            crate::ws::broadcast_to_channel(
                &state.redis,
                channel_id,
                &crate::ws::ServerEvent::MessageNew {
                    channel_id,
                    message: serde_json::json!({
                        "id": message.id,
                        "channel_id": channel_id,
                        "author": {
                            "id": bot_user_id,
                        },
                        "content": message.content,
                        "encrypted": message.encrypted,
                        "nonce": message.nonce,
                        "reply_to": message.reply_to,
                        "created_at": message.created_at.to_rfc3339(),
                    }),
                },
            )
            .await
            .map_err(|e| {
                warn!("Failed to broadcast bot message: {e}");
                format!("Failed to broadcast: {e}")
            })?;

            Ok(())
        }
        BotClientEvent::CommandResponse {
            interaction_id,
            content,
            ephemeral,
        } => {
            // Validate content length
            if let Err(e) = crate::chat::messages::validate_message_content(&content) {
                return Err(e.message.as_ref().map(|m| m.to_string()).unwrap_or_else(|| "Invalid response content".to_string()));
            }

            info!(
                interaction_id = %interaction_id,
                ephemeral = ephemeral,
                "Bot responding to command"
            );

            let owner_key = format!("interaction:{interaction_id}:owner");
            let expected_owner = state
                .redis
                .get::<Option<String>, _>(&owner_key)
                .await
                .map_err(|e| {
                    error!(
                        interaction_id = %interaction_id,
                        error = %e,
                        "Failed to fetch interaction owner"
                    );
                    "Failed to validate interaction ownership".to_string()
                })?
                .ok_or_else(|| "Interaction not found or expired".to_string())?;

            let bot_user_id_str = bot_user_id.to_string();
            if expected_owner != bot_user_id_str {
                warn!(
                    interaction_id = %interaction_id,
                    bot_user_id = %bot_user_id,
                    expected_owner = %expected_owner,
                    "Bot attempted to respond to interaction it does not own"
                );
                return Err("Interaction does not belong to this bot".to_string());
            }

            // Store command response in Redis with expiry (5 minutes)
            // The command invoker's WebSocket client will poll/listen for this
            let response_key = format!("interaction:{interaction_id}:response");
            let response_data = serde_json::json!({
                "content": content,
                "ephemeral": ephemeral,
                "bot_user_id": bot_user_id,
            });

            let was_set: bool = state
                .redis
                .set(
                    &response_key,
                    response_data.to_string(),
                    Some(fred::types::Expiration::EX(300)),
                    Some(fred::types::SetOptions::NX),
                    false,
                )
                .await
                .map_err(|e| {
                    error!("Failed to store command response: {e}");
                    format!("Failed to store response: {e}")
                })?;

            if !was_set {
                return Err("Response already provided for this interaction".to_string());
            }

            // Publish event to notify waiting clients
            state
                .redis
                .publish::<(), _, _>(
                    format!("interaction:{interaction_id}"),
                    response_data.to_string(),
                )
                .await
                .map_err(|e| {
                    error!("Failed to publish command response: {e}");
                    format!("Failed to publish response: {e}")
                })?;

            // Deliver response to the invoking user/channel
            let context_key = format!("interaction:{interaction_id}:context");
            let context_raw: Option<String> = state.redis.get(&context_key).await.map_err(|e| {
                error!("Failed to fetch interaction context: {e}");
                format!("Failed to fetch interaction context: {e}")
            })?;

            let Some(context_raw) = context_raw else {
                warn!(interaction_id = %interaction_id, "Interaction context not found, skipping delivery");
                return Ok(());
            };

            let context: serde_json::Value = serde_json::from_str(&context_raw).map_err(|e| {
                error!("Failed to parse interaction context: {e}");
                format!("Failed to parse interaction context: {e}")
            })?;

            let user_id = context["user_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or("Invalid user_id in interaction context")?;
            let channel_id = context["channel_id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or("Invalid channel_id in interaction context")?;
            let command_name = context["command_name"]
                .as_str()
                .ok_or("Invalid command_name in interaction context")?
                .to_string();

            let bot_user = crate::db::find_user_by_id(&state.db, bot_user_id)
                .await
                .map_err(|e| format!("Failed to look up bot user: {e}"))?;
            let bot_name = bot_user
                .as_ref()
                .map(|u| u.display_name.clone())
                .unwrap_or_else(|| "Bot".to_string());

            if ephemeral {
                // Ephemeral: only deliver to the invoking user
                crate::ws::broadcast_to_user(
                    &state.redis,
                    user_id,
                    &crate::ws::ServerEvent::CommandResponse {
                        interaction_id,
                        content,
                        command_name,
                        bot_name,
                        channel_id,
                        ephemeral: true,
                    },
                )
                .await
                .map_err(|e| {
                    warn!("Failed to deliver ephemeral command response: {e}");
                    format!("Failed to deliver response: {e}")
                })?;
            } else {
                // Non-ephemeral: insert a real message and broadcast to channel
                let message = crate::db::create_message(
                    &state.db,
                    channel_id,
                    bot_user_id,
                    &content,
                    false,
                    None,
                    None,
                )
                .await
                .map_err(|e| {
                    error!("Failed to create bot command response message: {e}");
                    format!("Failed to create message: {e}")
                })?;

                let author_json = if let Some(ref u) = bot_user {
                    serde_json::json!({
                        "id": u.id,
                        "username": u.username,
                        "display_name": u.display_name,
                        "avatar_url": u.avatar_url,
                        "status": format!("{:?}", u.status).to_lowercase(),
                    })
                } else {
                    serde_json::json!({
                        "id": bot_user_id,
                        "username": "bot",
                        "display_name": "Bot",
                        "avatar_url": null,
                        "status": "offline",
                    })
                };

                crate::ws::broadcast_to_channel(
                    &state.redis,
                    channel_id,
                    &crate::ws::ServerEvent::MessageNew {
                        channel_id,
                        message: serde_json::json!({
                            "id": message.id,
                            "channel_id": channel_id,
                            "author": author_json,
                            "content": message.content,
                            "encrypted": message.encrypted,
                            "nonce": message.nonce,
                            "reply_to": message.reply_to,
                            "created_at": message.created_at.to_rfc3339(),
                        }),
                    },
                )
                .await
                .map_err(|e| {
                    warn!("Failed to broadcast bot command response: {e}");
                    format!("Failed to broadcast: {e}")
                })?;
            }

            Ok(())
        }
    }
}
