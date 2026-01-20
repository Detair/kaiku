//! WebSocket Handler
//!
//! Real-time communication for chat and voice signaling.

use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::{
    extract::{Query, State, WebSocketUpgrade},
    response::Response,
};
use fred::prelude::*;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::{api::AppState, auth::jwt, db, voice::ScreenShareInfo, voice::Quality};

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
    Subscribe {
        /// Channel to subscribe to.
        channel_id: Uuid,
    },
    /// Unsubscribe from channel events
    Unsubscribe {
        /// Channel to unsubscribe from.
        channel_id: Uuid,
    },
    /// Send typing indicator
    Typing {
        /// Channel user is typing in.
        channel_id: Uuid,
    },
    /// Stop typing indicator
    StopTyping {
        /// Channel user stopped typing in.
        channel_id: Uuid,
    },

    // Voice events
    /// Join a voice channel
    VoiceJoin {
        /// Voice channel to join.
        channel_id: Uuid,
    },
    /// Leave a voice channel
    VoiceLeave {
        /// Voice channel to leave.
        channel_id: Uuid,
    },
    /// Send SDP answer to server
    VoiceAnswer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP answer.
        sdp: String,
    },
    /// Send ICE candidate to server
    VoiceIceCandidate {
        /// Voice channel.
        channel_id: Uuid,
        /// ICE candidate string.
        candidate: String,
    },
    /// Mute self in voice channel
    VoiceMute {
        /// Voice channel.
        channel_id: Uuid,
    },
    /// Unmute self in voice channel
    VoiceUnmute {
        /// Voice channel.
        channel_id: Uuid,
    },
    /// Report voice quality statistics
    VoiceStats {
        /// Voice channel.
        channel_id: Uuid,
        /// Voice session ID.
        session_id: Uuid,
        /// Round-trip latency in milliseconds.
        latency: i16,
        /// Packet loss percentage (0.0-100.0).
        packet_loss: f32,
        /// Jitter in milliseconds.
        jitter: i16,
        /// Quality score (0-100).
        quality: u8,
        /// Timestamp when stats were collected (Unix epoch ms).
        timestamp: i64,
    },
}

/// Participant info for voice room state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceParticipant {
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Whether the user is muted.
    pub muted: bool,
    /// Whether this participant is currently screen sharing.
    #[serde(default)]
    pub screen_sharing: bool,
}

/// Server-to-client events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// Connection authenticated successfully
    Ready {
        /// Authenticated user ID.
        user_id: Uuid,
    },
    /// Pong response
    Pong,
    /// Subscribed to channel
    Subscribed {
        /// Channel subscribed to.
        channel_id: Uuid,
    },
    /// Unsubscribed from channel
    Unsubscribed {
        /// Channel unsubscribed from.
        channel_id: Uuid,
    },
    /// New message in channel
    MessageNew {
        /// Channel containing the message.
        channel_id: Uuid,
        /// Full message object.
        message: serde_json::Value,
    },
    /// Message edited
    MessageEdit {
        /// Channel containing the message.
        channel_id: Uuid,
        /// Message ID.
        message_id: Uuid,
        /// New content.
        content: String,
        /// Edit timestamp (RFC3339).
        edited_at: String,
    },
    /// Message deleted
    MessageDelete {
        /// Channel containing the message.
        channel_id: Uuid,
        /// Deleted message ID.
        message_id: Uuid,
    },
    /// User typing
    TypingStart {
        /// Channel user is typing in.
        channel_id: Uuid,
        /// User who is typing.
        user_id: Uuid,
    },
    /// User stopped typing
    TypingStop {
        /// Channel user stopped typing in.
        channel_id: Uuid,
        /// User who stopped typing.
        user_id: Uuid,
    },
    /// Presence update
    PresenceUpdate {
        /// User whose presence changed.
        user_id: Uuid,
        /// New status (online, away, busy, offline).
        status: String,
    },
    /// Error
    Error {
        /// Error code.
        code: String,
        /// Error message.
        message: String,
    },

    // Voice events
    /// SDP offer from server (after `VoiceJoin`)
    VoiceOffer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP offer.
        sdp: String,
    },
    /// ICE candidate from server
    VoiceIceCandidate {
        /// Voice channel.
        channel_id: Uuid,
        /// ICE candidate string.
        candidate: String,
    },
    /// User joined voice channel
    VoiceUserJoined {
        /// Voice channel.
        channel_id: Uuid,
        /// User who joined.
        user_id: Uuid,
        /// User's username.
        username: String,
        /// User's display name.
        display_name: String,
    },
    /// User left voice channel
    VoiceUserLeft {
        /// Voice channel.
        channel_id: Uuid,
        /// User who left.
        user_id: Uuid,
    },
    /// User muted in voice channel
    VoiceUserMuted {
        /// Voice channel.
        channel_id: Uuid,
        /// User who muted.
        user_id: Uuid,
    },
    /// User unmuted in voice channel
    VoiceUserUnmuted {
        /// Voice channel.
        channel_id: Uuid,
        /// User who unmuted.
        user_id: Uuid,
    },
    /// Current voice room state (sent on join)
    VoiceRoomState {
        /// Voice channel.
        channel_id: Uuid,
        /// Current participants.
        participants: Vec<VoiceParticipant>,
        /// Active screen shares.
        #[serde(default)]
        screen_shares: Vec<ScreenShareInfo>,
    },
    /// Voice error
    VoiceError {
        /// Error code.
        code: String,
        /// Error message.
        message: String,
    },
    /// Voice quality statistics for a user (broadcast to channel)
    VoiceUserStats {
        /// Voice channel.
        channel_id: Uuid,
        /// User whose stats are reported.
        user_id: Uuid,
        /// Round-trip latency in milliseconds.
        latency: i16,
        /// Packet loss percentage (0.0-100.0).
        packet_loss: f32,
        /// Jitter in milliseconds.
        jitter: i16,
        /// Quality score (0-100).
        quality: u8,
    },

    // Screen Share events
    /// Screen share started
    ScreenShareStarted {
        /// Channel ID.
        channel_id: Uuid,
        /// User who started sharing.
        user_id: Uuid,
        /// Username of sharer.
        username: String,
        /// Label of shared source.
        source_label: String,
        /// Whether audio is included.
        has_audio: bool,
        /// Quality tier.
        quality: Quality,
    },
    /// Screen share stopped
    ScreenShareStopped {
        /// Channel ID.
        channel_id: Uuid,
        /// User who stopped sharing.
        user_id: Uuid,
        /// Reason for stop.
        reason: String,
    },
    /// Screen share quality changed
    ScreenShareQualityChanged {
        /// Channel ID.
        channel_id: Uuid,
        /// User whose quality changed.
        user_id: Uuid,
        /// New quality tier.
        new_quality: Quality,
        /// Reason for change (e.g. "bandwidth").
        reason: String,
    },

    // Call events (DM voice calls)
    /// Incoming call notification (sent to recipient)
    IncomingCall {
        /// DM channel ID.
        channel_id: Uuid,
        /// User who initiated the call.
        initiator: Uuid,
        /// Initiator's username.
        initiator_name: String,
        /// Call capabilities (e.g., `["audio", "video"]`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capabilities: Vec<String>,
    },
    /// Call started (acknowledgement for the initiator)
    CallStarted {
        /// DM channel ID.
        channel_id: Uuid,
        /// User who initiated the call.
        initiator: Uuid,
        /// Initiator's username.
        initiator_name: String,
        /// Call capabilities (e.g., `["audio", "video"]`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        capabilities: Vec<String>,
    },
    /// Call ended
    CallEnded {
        /// DM channel ID.
        channel_id: Uuid,
        /// End reason.
        reason: String,
        /// Call duration in seconds (if call was connected).
        duration_secs: Option<u32>,
    },
    /// Participant joined the call
    CallParticipantJoined {
        /// DM channel ID.
        channel_id: Uuid,
        /// User who joined.
        user_id: Uuid,
        /// User's username.
        username: String,
    },
    /// Participant left the call
    CallParticipantLeft {
        /// DM channel ID.
        channel_id: Uuid,
        /// User who left.
        user_id: Uuid,
    },
    /// Someone declined the call
    CallDeclined {
        /// DM channel ID.
        channel_id: Uuid,
        /// User who declined.
        user_id: Uuid,
    },
}

/// Redis pub/sub channels.
pub mod channels {
    use uuid::Uuid;

    /// Redis channel for channel events.
    #[must_use]
    pub fn channel_events(channel_id: Uuid) -> String {
        format!("channel:{channel_id}")
    }

    /// Redis channel for user presence updates (future feature).
    #[allow(dead_code)]
    #[must_use]
    pub fn user_presence(user_id: Uuid) -> String {
        format!("presence:{user_id}")
    }

    /// Redis channel for global events (future feature).
    #[allow(dead_code)]
    pub const GLOBAL_EVENTS: &str = "global";
}

/// Broadcast a server event to a channel via Redis.
pub async fn broadcast_to_channel(
    redis: &RedisClient,
    channel_id: Uuid,
    event: &ServerEvent,
) -> Result<(), RedisError> {
    let payload = serde_json::to_string(event)
        .map_err(|e| RedisError::new(RedisErrorKind::Parse, format!("JSON error: {e}")))?;

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
                if let Err(e) =
                    handle_client_message(&text, user_id, &state, &tx, &subscribed_channels).await
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
            Ok(Message::Ping(_data)) => {
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
            if db::find_channel_by_id(&state.db, channel_id)
                .await?
                .is_none()
            {
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
                &ServerEvent::TypingStart {
                    channel_id,
                    user_id,
                },
            )
            .await?;
        }

        ClientEvent::StopTyping { channel_id } => {
            // Broadcast stop typing
            broadcast_to_channel(
                &state.redis,
                channel_id,
                &ServerEvent::TypingStop {
                    channel_id,
                    user_id,
                },
            )
            .await?;
        }

        // Voice events - delegate to voice handler
        ClientEvent::VoiceJoin { .. }
        | ClientEvent::VoiceLeave { .. }
        | ClientEvent::VoiceAnswer { .. }
        | ClientEvent::VoiceIceCandidate { .. }
        | ClientEvent::VoiceMute { .. }
        | ClientEvent::VoiceUnmute { .. }
        | ClientEvent::VoiceStats { .. } => {
            if let Err(e) = crate::voice::ws_handler::handle_voice_event(
                &state.sfu, &state.db, &state.redis, user_id, event, tx,
            )
            .await
            {
                warn!("Voice event error: {}", e);
                tx.send(ServerEvent::VoiceError {
                    code: "voice_error".to_string(),
                    message: e.to_string(),
                })
                .await?;
            }
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
async fn update_presence(state: &AppState, user_id: Uuid, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET status = $1::user_status WHERE id = $2")
        .bind(status)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    Ok(())
}
