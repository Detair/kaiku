//! WebSocket Handler
//!
//! Real-time communication for chat and voice signaling.

use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use axum::extract::ws::{Message, WebSocket};
use fred::prelude::*;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{api::AppState, auth::jwt, db};

/// WebSocket connection query params.
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    /// JWT access token for authentication
    pub token: String,
}

/// Client-to-server events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    /// Ping for keepalive
    Ping,
    /// Subscribe to channel events
    Subscribe { channel_id: Uuid },
    /// Unsubscribe from channel events
    Unsubscribe { channel_id: Uuid },
    /// Send typing indicator
    Typing { channel_id: Uuid },
    /// Stop typing indicator
    StopTyping { channel_id: Uuid },
}

/// Server-to-client events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// Connection authenticated successfully
    Ready { user_id: Uuid },
    /// Pong response
    Pong,
    /// Subscribed to channel
    Subscribed { channel_id: Uuid },
    /// Unsubscribed from channel
    Unsubscribed { channel_id: Uuid },
    /// New message in channel
    MessageNew {
        channel_id: Uuid,
        message: serde_json::Value,
    },
    /// Message edited
    MessageEdit {
        channel_id: Uuid,
        message_id: Uuid,
        content: String,
        edited_at: String,
    },
    /// Message deleted
    MessageDelete {
        channel_id: Uuid,
        message_id: Uuid,
    },
    /// User typing
    TypingStart { channel_id: Uuid, user_id: Uuid },
    /// User stopped typing
    TypingStop { channel_id: Uuid, user_id: Uuid },
    /// Presence update
    PresenceUpdate { user_id: Uuid, status: String },
    /// Error
    Error { code: String, message: String },
}

/// Redis pub/sub channels.
pub mod channels {
    use uuid::Uuid;

    pub fn channel_events(channel_id: Uuid) -> String {
        format!("channel:{}", channel_id)
    }

    pub fn user_presence(user_id: Uuid) -> String {
        format!("presence:{}", user_id)
    }

    pub const GLOBAL_EVENTS: &str = "global";
}

/// Broadcast a server event to a channel via Redis.
pub async fn broadcast_to_channel(
    redis: &RedisClient,
    channel_id: Uuid,
    event: &ServerEvent,
) -> Result<(), RedisError> {
    let payload = serde_json::to_string(event).map_err(|e| {
        RedisError::new(RedisErrorKind::Parse, format!("JSON error: {}", e))
    })?;

    redis
        .publish::<(), _, _>(channels::channel_events(channel_id), payload)
        .await?;

    Ok(())
}

/// WebSocket upgrade handler.
pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
) -> Response {
    // Validate token before upgrade
    let claims = match jwt::validate_access_token(&query.token, &state.config.jwt_secret) {
        Ok(claims) => claims,
        Err(_) => {
            return Response::builder()
                .status(401)
                .body("Invalid token".into())
                .unwrap();
        }
    };

    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return Response::builder()
                .status(401)
                .body("Invalid user ID in token".into())
                .unwrap();
        }
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state, user_id))
}

/// Handle WebSocket connection.
async fn handle_socket(socket: WebSocket, state: AppState, user_id: Uuid) {
    use futures::stream::{SplitSink, SplitStream};
    let (mut ws_sender, mut ws_receiver): (SplitSink<WebSocket, Message>, SplitStream<WebSocket>) =
        socket.split();

    // Channel for sending messages to the WebSocket
    let (tx, mut rx) = mpsc::channel::<ServerEvent>(100);

    // Track subscribed channels
    let subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>> =
        Arc::new(tokio::sync::RwLock::new(HashSet::new()));

    // Update user presence to online
    if let Err(e) = update_presence(&state, user_id, "online").await {
        warn!("Failed to update presence: {}", e);
    }

    info!("WebSocket connected: user={}", user_id);

    // Send ready event
    let _ = tx.send(ServerEvent::Ready { user_id }).await;

    // Spawn task to handle Redis pub/sub
    let redis_client = state.redis.clone();
    let tx_clone = tx.clone();
    let subscribed_clone = subscribed_channels.clone();
    let pubsub_handle = tokio::spawn(async move {
        handle_pubsub(redis_client, tx_clone, subscribed_clone).await;
    });

    // Spawn task to forward events to WebSocket
    let sender_handle: tokio::task::JoinHandle<()> = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let msg = match serde_json::to_string(&event) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize event: {}", e);
                    continue;
                }
            };

            let send_result: Result<(), axum::Error> = ws_sender.send(Message::Text(msg)).await;
            if send_result.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Err(e) = handle_client_message(
                    &text,
                    user_id,
                    &state,
                    &tx,
                    &subscribed_channels,
                )
                .await
                {
                    warn!("Error handling message: {}", e);
                    let _ = tx
                        .send(ServerEvent::Error {
                            code: "message_error".to_string(),
                            message: e.to_string(),
                        })
                        .await;
                }
            }
            Ok(Message::Ping(data)) => {
                // Axum handles pong automatically, but we can respond too
                debug!("Received ping from user={}", user_id);
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket closed: user={}", user_id);
                break;
            }
            Err(e) => {
                warn!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    pubsub_handle.abort();
    sender_handle.abort();

    // Update user presence to offline
    if let Err(e) = update_presence(&state, user_id, "offline").await {
        warn!("Failed to update presence on disconnect: {}", e);
    }

    info!("WebSocket disconnected: user={}", user_id);
}

/// Handle a client message.
async fn handle_client_message(
    text: &str,
    user_id: Uuid,
    state: &AppState,
    tx: &mpsc::Sender<ServerEvent>,
    subscribed_channels: &Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let event: ClientEvent = serde_json::from_str(text)?;

    match event {
        ClientEvent::Ping => {
            tx.send(ServerEvent::Pong).await?;
        }

        ClientEvent::Subscribe { channel_id } => {
            // Verify channel exists
            if db::find_channel_by_id(&state.db, channel_id).await?.is_none() {
                tx.send(ServerEvent::Error {
                    code: "channel_not_found".to_string(),
                    message: "Channel not found".to_string(),
                })
                .await?;
                return Ok(());
            }

            // Add to subscribed channels
            subscribed_channels.write().await.insert(channel_id);

            tx.send(ServerEvent::Subscribed { channel_id }).await?;
            debug!("User {} subscribed to channel {}", user_id, channel_id);
        }

        ClientEvent::Unsubscribe { channel_id } => {
            subscribed_channels.write().await.remove(&channel_id);
            tx.send(ServerEvent::Unsubscribed { channel_id }).await?;
            debug!("User {} unsubscribed from channel {}", user_id, channel_id);
        }

        ClientEvent::Typing { channel_id } => {
            // Broadcast typing indicator
            broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::TypingStart { channel_id, user_id },
            )
            .await?;
        }

        ClientEvent::StopTyping { channel_id } => {
            // Broadcast stop typing
            broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::TypingStop { channel_id, user_id },
            )
            .await?;
        }
    }

    Ok(())
}

/// Handle Redis pub/sub messages.
async fn handle_pubsub(
    redis: RedisClient,
    tx: mpsc::Sender<ServerEvent>,
    subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
) {
    // Create a subscriber client
    let subscriber = redis.clone_new();

    // Connect (fred 8.x returns JoinHandle)
    let _connect_handle = subscriber.connect();

    if let Err(e) = subscriber.wait_for_connect().await {
        error!("Subscriber connection failed: {}", e);
        return;
    }

    // Subscribe to pattern for all channel events
    let mut pubsub_stream = subscriber.message_rx();

    // Subscribe to channel pattern
    if let Err(e) = subscriber.psubscribe("channel:*").await {
        error!("Failed to psubscribe: {}", e);
        return;
    }

    while let Ok(message) = pubsub_stream.recv().await {
        // Extract channel ID from the channel name (channel:{uuid})
        let channel_name = message.channel.to_string();
        if let Some(uuid_str) = channel_name.strip_prefix("channel:") {
            if let Ok(channel_id) = Uuid::parse_str(uuid_str) {
                // Check if we're subscribed to this channel
                if subscribed_channels.read().await.contains(&channel_id) {
                    // Parse and forward the event
                    if let Some(payload) = message.value.as_str() {
                        if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Update user presence in the database.
async fn update_presence(
    state: &AppState,
    user_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET status = $1::user_status WHERE id = $2",
    )
    .bind(status)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(())
}
