//! WebSocket Connection Manager
//!
//! Manages real-time connection to the server with automatic reconnection.

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// Client events sent to the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    Ping,
    Subscribe {
        channel_id: String,
    },
    Unsubscribe {
        channel_id: String,
    },
    Typing {
        channel_id: String,
    },
    StopTyping {
        channel_id: String,
    },
    VoiceJoin {
        channel_id: String,
    },
    VoiceLeave {
        channel_id: String,
    },
    VoiceAnswer {
        channel_id: String,
        sdp: String,
    },
    VoiceIceCandidate {
        channel_id: String,
        candidate: String,
    },
    VoiceMute {
        channel_id: String,
    },
    VoiceUnmute {
        channel_id: String,
    },
    SetActivity {
        activity: Option<serde_json::Value>,
    },
}

/// Server events received from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Ready {
        user_id: String,
    },
    Pong,
    Subscribed {
        channel_id: String,
    },
    Unsubscribed {
        channel_id: String,
    },
    MessageNew {
        channel_id: String,
        message: serde_json::Value,
    },
    MessageEdit {
        channel_id: String,
        message_id: String,
        content: String,
        edited_at: String,
    },
    MessageDelete {
        channel_id: String,
        message_id: String,
    },
    TypingStart {
        channel_id: String,
        user_id: String,
    },
    TypingStop {
        channel_id: String,
        user_id: String,
    },
    PresenceUpdate {
        user_id: String,
        status: String,
    },
    RichPresenceUpdate {
        user_id: String,
        activity: Option<serde_json::Value>,
    },
    VoiceOffer {
        channel_id: String,
        sdp: String,
    },
    VoiceIceCandidate {
        channel_id: String,
        candidate: String,
    },
    VoiceUserJoined {
        channel_id: String,
        user_id: String,
    },
    VoiceUserLeft {
        channel_id: String,
        user_id: String,
    },
    VoiceUserMuted {
        channel_id: String,
        user_id: String,
    },
    VoiceUserUnmuted {
        channel_id: String,
        user_id: String,
    },
    VoiceRoomState {
        channel_id: String,
        participants: Vec<serde_json::Value>,
    },
    VoiceError {
        code: String,
        message: String,
    },
    Error {
        code: String,
        message: String,
    },
    // Call events
    IncomingCall {
        channel_id: String,
        initiator: String,
        initiator_name: String,
    },
    CallStarted {
        channel_id: String,
    },
    CallEnded {
        channel_id: String,
        reason: String,
        duration_secs: Option<u64>,
    },
    CallParticipantJoined {
        channel_id: String,
        user_id: String,
        username: String,
    },
    CallParticipantLeft {
        channel_id: String,
        user_id: String,
    },
    CallDeclined {
        channel_id: String,
        user_id: String,
    },
    // Read sync events
    ChannelRead {
        channel_id: String,
    },
    DmRead {
        channel_id: String,
    },
    DmNameUpdated {
        channel_id: String,
        name: String,
    },
    // Screen share events
    ScreenShareStarted {
        channel_id: String,
        user_id: String,
        username: String,
        source_label: String,
        has_audio: bool,
        quality: String,
    },
    ScreenShareStopped {
        channel_id: String,
        user_id: String,
        reason: String,
    },
    ScreenShareQualityChanged {
        channel_id: String,
        user_id: String,
        new_quality: String,
        reason: String,
    },
    // Reaction events
    ReactionAdd {
        channel_id: String,
        message_id: String,
        user_id: String,
        emoji: String,
    },
    ReactionRemove {
        channel_id: String,
        message_id: String,
        user_id: String,
        emoji: String,
    },
    // Voice stats
    VoiceUserStats {
        channel_id: String,
        user_id: String,
        latency: f64,
        packet_loss: f64,
        jitter: f64,
        quality: f64,
    },
    // Admin events
    AdminUserBanned {
        user_id: String,
        username: String,
    },
    AdminUserUnbanned {
        user_id: String,
        username: String,
    },
    AdminGuildSuspended {
        guild_id: String,
        guild_name: String,
    },
    AdminGuildUnsuspended {
        guild_id: String,
        guild_name: String,
    },
    AdminReportCreated {
        report_id: String,
        category: String,
        target_type: String,
    },
    AdminReportResolved {
        report_id: String,
    },
    // Friend events
    FriendRequestReceived {
        friendship_id: String,
        from_user_id: String,
        from_username: String,
        from_display_name: String,
        from_avatar_url: Option<String>,
    },
    FriendRequestAccepted {
        friendship_id: String,
        user_id: String,
        username: String,
        display_name: String,
        avatar_url: Option<String>,
    },
    // Block events
    UserBlocked {
        user_id: String,
    },
    UserUnblocked {
        user_id: String,
    },
    // Thread events
    ThreadReplyNew {
        channel_id: String,
        parent_id: String,
        message: serde_json::Value,
        thread_info: serde_json::Value,
    },
    ThreadReplyDelete {
        channel_id: String,
        parent_id: String,
        message_id: String,
        thread_info: serde_json::Value,
    },
    ThreadRead {
        thread_parent_id: String,
        last_read_message_id: Option<String>,
    },
    // Preferences sync
    PreferencesUpdated {
        preferences: serde_json::Value,
        updated_at: String,
    },
    // State sync
    Patch {
        entity_type: String,
        entity_id: String,
        diff: serde_json::Value,
    },
}

/// Connection status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
}

/// WebSocket manager state.
pub struct WebSocketManager {
    /// Channel to send events to the WebSocket.
    tx: mpsc::Sender<ClientEvent>,
    /// Connection status.
    status: Arc<RwLock<ConnectionStatus>>,
    /// Handle for shutdown.
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl WebSocketManager {
    /// Create a new WebSocket manager and start connection.
    pub async fn connect(
        app: AppHandle,
        server_url: String,
        token: String,
    ) -> Result<Self, String> {
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>(100);
        let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
        let status = Arc::new(RwLock::new(ConnectionStatus::Connecting));

        // Spawn the connection task
        let status_clone = status.clone();
        tokio::spawn(async move {
            connection_loop(app, server_url, token, event_rx, shutdown_rx, status_clone).await;
        });

        Ok(Self {
            tx: event_tx,
            status,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Send an event to the server.
    pub async fn send(&self, event: ClientEvent) -> Result<(), String> {
        self.tx
            .send(event)
            .await
            .map_err(|e| format!("Failed to send event: {e}"))
    }

    /// Get the current connection status.
    pub async fn status(&self) -> ConnectionStatus {
        self.status.read().await.clone()
    }

    /// Disconnect from the server.
    pub async fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

/// Main connection loop with reconnection logic.
async fn connection_loop(
    app: AppHandle,
    server_url: String,
    token: String,
    mut event_rx: mpsc::Receiver<ClientEvent>,
    mut shutdown_rx: mpsc::Receiver<()>,
    status: Arc<RwLock<ConnectionStatus>>,
) {
    let mut attempt = 0u32;
    let max_backoff = Duration::from_secs(30);

    loop {
        // Check for shutdown
        if shutdown_rx.try_recv().is_ok() {
            info!("WebSocket shutdown requested");
            *status.write().await = ConnectionStatus::Disconnected;
            let _ = app.emit("ws:disconnected", ());
            return;
        }

        // Build WebSocket URL
        let ws_url = build_ws_url(&server_url, &token);
        info!(
            "Connecting to WebSocket: {}",
            ws_url.split('?').next().unwrap_or(&ws_url)
        );

        if attempt > 0 {
            *status.write().await = ConnectionStatus::Reconnecting { attempt };
            let _ = app.emit("ws:reconnecting", attempt);
        } else {
            *status.write().await = ConnectionStatus::Connecting;
            let _ = app.emit("ws:connecting", ());
        }

        // Try to connect
        match connect_async(&ws_url).await {
            Ok((ws_stream, _)) => {
                info!("WebSocket connected");
                attempt = 0;
                *status.write().await = ConnectionStatus::Connected;
                let _ = app.emit("ws:connected", ());

                // Split the stream
                let (mut write, mut read) = ws_stream.split();

                // Handle messages until disconnected
                loop {
                    tokio::select! {
                        // Handle incoming messages
                        msg = read.next() => {
                            match msg {
                                Some(Ok(Message::Text(text))) => {
                                    handle_server_message(&app, &text);
                                }
                                Some(Ok(Message::Ping(data))) => {
                                    if let Err(e) = write.send(Message::Pong(data)).await {
                                        warn!("Failed to send pong: {}", e);
                                    }
                                }
                                Some(Ok(Message::Close(_))) => {
                                    info!("Server closed connection");
                                    break;
                                }
                                Some(Err(e)) => {
                                    error!("WebSocket error: {}", e);
                                    break;
                                }
                                None => {
                                    info!("WebSocket stream ended");
                                    break;
                                }
                                _ => {} // Ignore other message types
                            }
                        }

                        // Handle outgoing events
                        event = event_rx.recv() => {
                            if let Some(ev) = event {
                                if let Ok(json) = serde_json::to_string(&ev) {
                                    debug!("Sending: {}", json);
                                    if let Err(e) = write.send(Message::Text(json.into())).await {
                                        error!("Failed to send message: {}", e);
                                        break;
                                    }
                                }
                            } else {
                                info!("Event channel closed");
                                break;
                            }
                        }

                        // Handle shutdown
                        _ = shutdown_rx.recv() => {
                            info!("Shutdown received during connection");
                            let _ = write.send(Message::Close(None)).await;
                            *status.write().await = ConnectionStatus::Disconnected;
                            let _ = app.emit("ws:disconnected", ());
                            return;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect: {}", e);
            }
        }

        // Connection lost or failed - attempt reconnection
        *status.write().await = ConnectionStatus::Disconnected;
        let _ = app.emit("ws:disconnected", ());

        attempt += 1;
        let backoff = std::cmp::min(Duration::from_secs(2u64.pow(attempt.min(5))), max_backoff);
        info!("Reconnecting in {:?} (attempt {})", backoff, attempt);

        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            _ = shutdown_rx.recv() => {
                info!("Shutdown during reconnect backoff");
                return;
            }
        }
    }
}

/// Build the WebSocket URL with authentication token.
fn build_ws_url(server_url: &str, token: &str) -> String {
    let base = server_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    format!("{}/ws?token={}", base.trim_end_matches('/'), token)
}

/// Handle a message from the server.
fn handle_server_message(app: &AppHandle, text: &str) {
    match serde_json::from_str::<ServerEvent>(text) {
        Ok(event) => {
            debug!("Received: {:?}", event);

            // Emit the event to the frontend
            let event_name = match &event {
                ServerEvent::Ready { .. } => "ws:ready",
                ServerEvent::Pong => "ws:pong",
                ServerEvent::Subscribed { .. } => "ws:subscribed",
                ServerEvent::Unsubscribed { .. } => "ws:unsubscribed",
                ServerEvent::MessageNew { .. } => "ws:message_new",
                ServerEvent::MessageEdit { .. } => "ws:message_edit",
                ServerEvent::MessageDelete { .. } => "ws:message_delete",
                ServerEvent::TypingStart { .. } => "ws:typing_start",
                ServerEvent::TypingStop { .. } => "ws:typing_stop",
                ServerEvent::PresenceUpdate { .. } => "ws:presence_update",
                ServerEvent::RichPresenceUpdate { .. } => "ws:rich_presence_update",
                ServerEvent::VoiceOffer { .. } => "ws:voice_offer",
                ServerEvent::VoiceIceCandidate { .. } => "ws:voice_ice_candidate",
                ServerEvent::VoiceUserJoined { .. } => "ws:voice_user_joined",
                ServerEvent::VoiceUserLeft { .. } => "ws:voice_user_left",
                ServerEvent::VoiceUserMuted { .. } => "ws:voice_user_muted",
                ServerEvent::VoiceUserUnmuted { .. } => "ws:voice_user_unmuted",
                ServerEvent::VoiceRoomState { .. } => "ws:voice_room_state",
                ServerEvent::VoiceError { .. } => "ws:voice_error",
                ServerEvent::Error { .. } => "ws:error",
                // Call events
                ServerEvent::IncomingCall { .. } => "ws:incoming_call",
                ServerEvent::CallStarted { .. } => "ws:call_started",
                ServerEvent::CallEnded { .. } => "ws:call_ended",
                ServerEvent::CallParticipantJoined { .. } => "ws:call_participant_joined",
                ServerEvent::CallParticipantLeft { .. } => "ws:call_participant_left",
                ServerEvent::CallDeclined { .. } => "ws:call_declined",
                // Read sync events
                ServerEvent::ChannelRead { .. } => "ws:channel_read",
                ServerEvent::DmRead { .. } => "ws:dm_read",
                ServerEvent::DmNameUpdated { .. } => "ws:dm_name_updated",
                // Screen share events
                ServerEvent::ScreenShareStarted { .. } => "ws:screen_share_started",
                ServerEvent::ScreenShareStopped { .. } => "ws:screen_share_stopped",
                ServerEvent::ScreenShareQualityChanged { .. } => "ws:screen_share_quality_changed",
                // Reaction events
                ServerEvent::ReactionAdd { .. } => "ws:reaction_add",
                ServerEvent::ReactionRemove { .. } => "ws:reaction_remove",
                // Voice stats
                ServerEvent::VoiceUserStats { .. } => "ws:voice_user_stats",
                // Admin events
                ServerEvent::AdminUserBanned { .. } => "ws:admin_user_banned",
                ServerEvent::AdminUserUnbanned { .. } => "ws:admin_user_unbanned",
                ServerEvent::AdminGuildSuspended { .. } => "ws:admin_guild_suspended",
                ServerEvent::AdminGuildUnsuspended { .. } => "ws:admin_guild_unsuspended",
                ServerEvent::AdminReportCreated { .. } => "ws:admin_report_created",
                ServerEvent::AdminReportResolved { .. } => "ws:admin_report_resolved",
                // Friend events
                ServerEvent::FriendRequestReceived { .. } => "ws:friend_request_received",
                ServerEvent::FriendRequestAccepted { .. } => "ws:friend_request_accepted",
                // Block events
                ServerEvent::UserBlocked { .. } => "ws:user_blocked",
                ServerEvent::UserUnblocked { .. } => "ws:user_unblocked",
                // Thread events
                ServerEvent::ThreadReplyNew { .. } => "ws:thread_reply_new",
                ServerEvent::ThreadReplyDelete { .. } => "ws:thread_reply_delete",
                ServerEvent::ThreadRead { .. } => "ws:thread_read",
                // Preferences sync
                ServerEvent::PreferencesUpdated { .. } => "ws:preferences_updated",
                // State sync
                ServerEvent::Patch { .. } => "ws:patch",
            };

            if let Err(e) = app.emit(event_name, &event) {
                error!("Failed to emit event: {}", e);
            }
        }
        Err(e) => {
            warn!("Failed to parse server message: {} - {}", e, text);
        }
    }
}
