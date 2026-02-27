//! WebSocket Handler
//!
//! Real-time communication for chat and voice signaling.
//!
//! ## Authentication
//!
//! WebSocket authentication uses the `Sec-WebSocket-Protocol` header instead of
//! query parameters to avoid token exposure in logs, browser history, and referrer
//! headers.
//!
//! Client should connect with:
//! ```text
//! Sec-WebSocket-Protocol: access_token.<jwt_token>
//! ```
//!
//! Server responds with:
//! ```text
//! Sec-WebSocket-Protocol: access_token
//! ```

pub mod bot_events;
pub mod bot_gateway;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::Response;
use chrono::{DateTime, Utc};
use fred::prelude::*;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::jwt;
use crate::db;
use crate::social::block_cache;
use crate::voice::{Quality, ScreenShareInfo, WebcamInfo};

/// Minimum interval between activity updates (10 seconds).
const ACTIVITY_UPDATE_INTERVAL: Duration = Duration::from_secs(10);

/// State for activity rate limiting and deduplication.
///
/// **Internal:** Exposed for integration tests only.
#[derive(Default)]
pub struct ActivityState {
    /// Last activity update timestamp.
    last_update: Option<Instant>,
    /// Last activity data for deduplication.
    last_activity: Option<crate::presence::Activity>,
}

/// WebSocket protocol header name for authentication.
const WS_PROTOCOL_PREFIX: &str = "access_token.";

/// Extract JWT token from Sec-WebSocket-Protocol header.
///
/// Expected format: `access_token.<jwt_token>`
///
/// Returns `None` if the header is missing or malformed.
fn extract_token_from_protocol(headers: &HeaderMap) -> Option<String> {
    headers
        .get("sec-websocket-protocol")
        .and_then(|h| h.to_str().ok())
        .and_then(|protocols| {
            // The header may contain multiple protocols separated by commas
            protocols
                .split(',')
                .map(str::trim)
                .find(|p| p.starts_with(WS_PROTOCOL_PREFIX))
                .map(|p| p[WS_PROTOCOL_PREFIX.len()..].to_string())
        })
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
    /// Start screen sharing in voice channel
    VoiceScreenShareStart {
        /// Voice channel.
        channel_id: Uuid,
        /// Requested quality tier.
        quality: Quality,
        /// Whether to include system audio.
        has_audio: bool,
        /// Label of the shared source (e.g., "Display 1", "Firefox").
        source_label: String,
    },
    /// Stop screen sharing in voice channel
    VoiceScreenShareStop {
        /// Voice channel.
        channel_id: Uuid,
    },

    /// Start webcam in voice channel
    VoiceWebcamStart {
        /// Voice channel.
        channel_id: Uuid,
        /// Requested quality tier.
        quality: Quality,
    },
    /// Stop webcam in voice channel
    VoiceWebcamStop {
        /// Voice channel.
        channel_id: Uuid,
    },

    /// Set rich presence activity (game, music, etc).
    SetActivity {
        activity: Option<crate::presence::Activity>,
    },

    /// Subscribe to admin events (requires elevated admin).
    AdminSubscribe,
    /// Unsubscribe from admin events.
    AdminUnsubscribe,
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
    /// Whether this participant has their webcam active.
    #[serde(default)]
    pub webcam_active: bool,
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
    /// Reaction added to a message
    ReactionAdd {
        /// Channel containing the message.
        channel_id: Uuid,
        /// Message the reaction was added to.
        message_id: Uuid,
        /// User who added the reaction.
        user_id: Uuid,
        /// Emoji that was added.
        emoji: String,
    },
    /// Reaction removed from a message
    ReactionRemove {
        /// Channel containing the message.
        channel_id: Uuid,
        /// Message the reaction was removed from.
        message_id: Uuid,
        /// User who removed the reaction.
        user_id: Uuid,
        /// Emoji that was removed.
        emoji: String,
    },
    /// Guild custom emojis updated
    GuildEmojiUpdated {
        /// Guild ID.
        guild_id: Uuid,
        /// Updated emojis list.
        emojis: Vec<crate::guild::types::GuildEmoji>,
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
        /// Active webcams.
        #[serde(default)]
        webcams: Vec<WebcamInfo>,
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
    // Webcam events
    /// Webcam started
    WebcamStarted {
        /// Channel ID.
        channel_id: Uuid,
        /// User who started their webcam.
        user_id: Uuid,
        /// Username of the user.
        username: String,
        /// Quality tier.
        quality: Quality,
    },
    /// Webcam stopped
    WebcamStopped {
        /// Channel ID.
        channel_id: Uuid,
        /// User who stopped their webcam.
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

    // DM read sync events
    /// DM read position updated (sent to other sessions of the same user)
    DmRead {
        /// DM channel ID.
        channel_id: Uuid,
        /// Last read message ID (None if no messages read).
        last_read_message_id: Option<Uuid>,
    },

    /// Guild channel read position updated (sent to other sessions of the same user)
    ChannelRead {
        /// Guild channel ID.
        channel_id: Uuid,
        /// Last read message ID (None if no messages read).
        last_read_message_id: Option<Uuid>,
    },

    /// Rich presence activity update.
    RichPresenceUpdate {
        user_id: Uuid,
        activity: Option<crate::presence::Activity>,
    },

    /// Generic entity patch for efficient state sync.
    /// Instead of sending full objects, only changed fields are sent.
    Patch {
        /// Entity type: "user", "guild", "member", "channel".
        entity_type: String,
        /// Entity ID.
        entity_id: Uuid,
        /// Partial update containing only changed fields.
        diff: serde_json::Value,
    },

    // User-specific events (broadcast to user's devices)
    /// User preferences were updated on another device.
    PreferencesUpdated {
        /// Updated preferences JSON.
        preferences: serde_json::Value,
        /// When the preferences were updated.
        updated_at: DateTime<Utc>,
    },

    // Friend events
    /// Friend request received (sent to the addressee).
    FriendRequestReceived {
        /// Friendship ID.
        friendship_id: Uuid,
        /// User who sent the request.
        from_user_id: Uuid,
        /// Requester's username.
        from_username: String,
        /// Requester's display name.
        from_display_name: String,
        /// Requester's avatar URL.
        from_avatar_url: Option<String>,
    },
    /// Friend request accepted (sent to the original requester).
    FriendRequestAccepted {
        /// Friendship ID.
        friendship_id: Uuid,
        /// User who accepted the request.
        user_id: Uuid,
        /// Accepter's username.
        username: String,
        /// Accepter's display name.
        display_name: String,
        /// Accepter's avatar URL.
        avatar_url: Option<String>,
    },

    // Block events (broadcast to blocker's sessions)
    /// A user was blocked (sent to blocker's sessions to update local state)
    UserBlocked {
        /// Blocked user ID.
        user_id: Uuid,
    },
    /// A user was unblocked (sent to blocker's sessions to update local state)
    UserUnblocked {
        /// Unblocked user ID.
        user_id: Uuid,
    },

    // Workspace events (broadcast to workspace owner's sessions)
    /// New workspace created.
    WorkspaceCreated {
        /// Created workspace.
        workspace: serde_json::Value,
    },
    /// Workspace updated (name/icon).
    WorkspaceUpdated {
        /// Updated workspace.
        workspace: serde_json::Value,
    },
    /// Workspace deleted.
    WorkspaceDeleted {
        /// Deleted workspace ID.
        workspace_id: Uuid,
    },
    /// Workspaces reordered.
    WorkspaceReordered {
        /// New ordering: list of `{ id, sort_order }`.
        workspaces: Vec<serde_json::Value>,
    },
    /// Entry added to workspace.
    WorkspaceEntryAdded {
        /// Workspace ID.
        workspace_id: Uuid,
        /// Added entry.
        entry: serde_json::Value,
    },
    /// Entry removed from workspace.
    WorkspaceEntryRemoved {
        /// Workspace ID.
        workspace_id: Uuid,
        /// Removed entry ID.
        entry_id: Uuid,
    },
    /// Workspace entries reordered.
    WorkspaceEntriesReordered {
        /// Workspace ID.
        workspace_id: Uuid,
        /// New ordering: list of { id, position }.
        entries: Vec<serde_json::Value>,
    },

    // Thread events
    /// New reply in a thread (broadcast to channel for indicator updates)
    ThreadReplyNew {
        /// Channel containing the thread.
        channel_id: Uuid,
        /// Thread parent message ID.
        parent_id: Uuid,
        /// Full reply message object.
        message: serde_json::Value,
        /// Updated thread info for the parent.
        thread_info: serde_json::Value,
    },
    /// Thread reply deleted (broadcast to channel for indicator updates)
    ThreadReplyDelete {
        /// Channel containing the thread.
        channel_id: Uuid,
        /// Thread parent message ID.
        parent_id: Uuid,
        /// Deleted reply message ID.
        message_id: Uuid,
        /// Updated thread info for the parent.
        thread_info: serde_json::Value,
    },
    /// Thread read position updated (sent to user's sessions only)
    ThreadRead {
        /// Thread parent message ID.
        thread_parent_id: Uuid,
        /// Last read message ID in the thread.
        last_read_message_id: Option<Uuid>,
    },

    // DM metadata events
    /// DM channel name was updated (broadcast to all participants)
    DmNameUpdated {
        /// DM channel ID.
        channel_id: Uuid,
        /// New name for the DM channel.
        name: String,
        /// User who changed the name.
        updated_by: Uuid,
    },

    // Admin events (broadcast to admin subscribers)
    /// User was banned
    AdminUserBanned {
        /// User ID that was banned.
        user_id: Uuid,
        /// Username for display.
        username: String,
    },
    /// User was unbanned
    AdminUserUnbanned {
        /// User ID that was unbanned.
        user_id: Uuid,
        /// Username for display.
        username: String,
    },
    /// Guild was suspended
    AdminGuildSuspended {
        /// Guild ID that was suspended.
        guild_id: Uuid,
        /// Guild name for display.
        guild_name: String,
    },
    /// Guild was unsuspended
    AdminGuildUnsuspended {
        /// Guild ID that was unsuspended.
        guild_id: Uuid,
        /// Guild name for display.
        guild_name: String,
    },
    /// User was permanently deleted
    AdminUserDeleted {
        /// User ID that was deleted.
        user_id: Uuid,
        /// Username for display.
        username: String,
    },
    /// Guild was permanently deleted
    AdminGuildDeleted {
        /// Guild ID that was deleted.
        guild_id: Uuid,
        /// Guild name for display.
        guild_name: String,
    },

    // Report events (broadcast to admin subscribers)
    /// New report created
    AdminReportCreated {
        /// Report ID.
        report_id: Uuid,
        /// Report category.
        category: String,
        /// Target type (user or message).
        target_type: String,
    },
    /// Report resolved
    AdminReportResolved {
        /// Report ID.
        report_id: Uuid,
    },
    /// Content filter blocked a message (broadcast to admin subscribers)
    AdminModerationBlocked {
        /// Guild where the message was blocked.
        guild_id: Uuid,
        /// User who sent the blocked message.
        user_id: Uuid,
        /// Channel where the message was attempted.
        channel_id: Uuid,
        /// Filter category that matched.
        category: String,
    },

    // Slash command response events
    /// Bot command response delivered to invoking user.
    CommandResponse {
        /// Interaction ID.
        interaction_id: Uuid,
        /// Response content from the bot.
        content: String,
        /// Command name that was invoked.
        command_name: String,
        /// Bot display name.
        bot_name: String,
        /// Channel where command was invoked.
        channel_id: Uuid,
        /// Whether response is ephemeral (only visible to invoker).
        ephemeral: bool,
    },
    /// Bot command response timed out.
    CommandResponseTimeout {
        /// Interaction ID.
        interaction_id: Uuid,
        /// Command name that timed out.
        command_name: String,
        /// Channel where command was invoked.
        channel_id: Uuid,
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

    /// Redis channel for user presence updates.
    #[must_use]
    pub fn user_presence(user_id: Uuid) -> String {
        format!("presence:{user_id}")
    }

    /// Redis channel for user-specific events (preferences sync, etc.).
    #[must_use]
    pub fn user_events(user_id: Uuid) -> String {
        format!("user:{user_id}")
    }

    /// Redis channel for guild-wide events (patches, updates).
    #[must_use]
    pub fn guild_events(guild_id: Uuid) -> String {
        format!("guild:{guild_id}")
    }

    /// Redis channel for admin events.
    pub const ADMIN_EVENTS: &str = "admin:events";
}

/// Broadcast a server event to a channel via Redis.
#[tracing::instrument(skip(redis, event), fields(channel_id = %channel_id))]
pub async fn broadcast_to_channel(
    redis: &Client,
    channel_id: Uuid,
    event: &ServerEvent,
) -> Result<(), Error> {
    let payload = serde_json::to_string(event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;

    redis
        .publish::<(), _, _>(channels::channel_events(channel_id), payload)
        .await?;

    Ok(())
}

/// Broadcast an admin event to all admin subscribers via Redis.
#[tracing::instrument(skip(redis, event))]
pub async fn broadcast_admin_event(redis: &Client, event: &ServerEvent) -> Result<(), Error> {
    let payload = serde_json::to_string(event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;

    redis
        .publish::<(), _, _>(channels::ADMIN_EVENTS, payload)
        .await?;

    Ok(())
}

/// Broadcast an event to all of a user's connected sessions via Redis.
#[tracing::instrument(skip(redis, event), fields(user_id = %user_id))]
pub async fn broadcast_to_user(
    redis: &Client,
    user_id: Uuid,
    event: &ServerEvent,
) -> Result<(), Error> {
    let payload = serde_json::to_string(event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;

    redis
        .publish::<(), _, _>(channels::user_events(user_id), payload)
        .await?;

    Ok(())
}

/// Broadcast a presence update to all users who should see it.
async fn broadcast_presence_update(state: &AppState, user_id: Uuid, event: &ServerEvent) {
    let json = match serde_json::to_string(event) {
        Ok(j) => j,
        Err(e) => {
            error!("Failed to serialize presence event: {}", e);
            return;
        }
    };

    // Broadcast on presence channel
    let channel = format!("presence:{user_id}");
    let result: Result<(), Error> = state.redis.publish(&channel, &json).await;
    if let Err(e) = result {
        error!("Failed to broadcast presence update: {}", e);
    }
}

/// Broadcast an entity patch to the presence channel.
///
/// This sends only the changed fields instead of full objects,
/// reducing bandwidth by up to 90% for partial updates.
#[tracing::instrument(skip(redis, diff), fields(user_id = %user_id))]
pub async fn broadcast_user_patch(
    redis: &Client,
    user_id: Uuid,
    diff: serde_json::Value,
) -> Result<(), Error> {
    if diff.as_object().is_none_or(|m| m.is_empty()) {
        return Ok(()); // Nothing to broadcast
    }

    let event = ServerEvent::Patch {
        entity_type: "user".to_string(),
        entity_id: user_id,
        diff,
    };

    let payload = serde_json::to_string(&event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;

    // Broadcast on presence channel so friends/guild members see it
    let channel = format!("presence:{user_id}");
    redis.publish::<(), _, _>(channel, payload).await?;

    Ok(())
}

/// Broadcast a guild patch to all guild members via Redis.
#[tracing::instrument(skip(redis, diff), fields(guild_id = %guild_id))]
pub async fn broadcast_guild_patch(
    redis: &Client,
    guild_id: Uuid,
    diff: serde_json::Value,
) -> Result<(), Error> {
    if diff.as_object().is_none_or(|m| m.is_empty()) {
        return Ok(()); // Nothing to broadcast
    }

    let event = ServerEvent::Patch {
        entity_type: "guild".to_string(),
        entity_id: guild_id,
        diff,
    };

    let payload = serde_json::to_string(&event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;

    // Broadcast to guild channel
    redis
        .publish::<(), _, _>(channels::guild_events(guild_id), payload)
        .await?;

    Ok(())
}

/// Broadcast a member patch to all guild members via Redis.
#[tracing::instrument(skip(redis, diff), fields(guild_id = %guild_id, user_id = %user_id))]
pub async fn broadcast_member_patch(
    redis: &Client,
    guild_id: Uuid,
    user_id: Uuid,
    diff: serde_json::Value,
) -> Result<(), Error> {
    if diff.as_object().is_none_or(|m| m.is_empty()) {
        return Ok(()); // Nothing to broadcast
    }

    let event = ServerEvent::Patch {
        entity_type: "member".to_string(),
        entity_id: user_id, // The member's user ID
        diff: serde_json::json!({
            "guild_id": guild_id,
            "updates": diff,
        }),
    };

    let payload = serde_json::to_string(&event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;

    // Broadcast to guild channel
    redis
        .publish::<(), _, _>(channels::guild_events(guild_id), payload)
        .await?;

    Ok(())
}

/// Build a plain-text HTTP error response without panicking.
///
/// Falls back to a 500 Internal Server Error if building the requested
/// status fails (which cannot happen with hardcoded status codes, but
/// avoids any `.expect` in the hot path).
fn error_response(status: u16, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .body(body.into())
        .unwrap_or_else(|_| {
            Response::builder()
                .status(500)
                .body("Internal Server Error".into())
                .expect("fallback response builder")
        })
}

/// WebSocket upgrade handler.
///
/// Authentication is performed via the `Sec-WebSocket-Protocol` header to avoid
/// token exposure in server logs and browser history (OWASP recommendation).
///
/// # Protocol
///
/// Client sends: `Sec-WebSocket-Protocol: access_token.<jwt_token>`
/// Server responds: `Sec-WebSocket-Protocol: access_token`
#[tracing::instrument(skip(ws, state, headers))]
pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // Extract token from Sec-WebSocket-Protocol header
    let token = match extract_token_from_protocol(&headers) {
        Some(t) => t,
        None => {
            return error_response(
                401,
                "Missing or invalid Sec-WebSocket-Protocol header. Expected: access_token.<jwt>",
            );
        }
    };

    // Validate token before upgrade
    let claims = match jwt::validate_access_token(&token, &state.config.jwt_public_key) {
        Ok(claims) => claims,
        Err(_) => {
            return error_response(401, "Invalid token");
        }
    };

    let user_id = match Uuid::parse_str(&claims.sub) {
        Ok(id) => id,
        Err(_) => {
            return error_response(401, "Invalid user ID in token");
        }
    };

    // Respond with the protocol to confirm (required for WebSocket handshake)
    ws.protocols(["access_token"])
        .on_upgrade(move |socket| handle_socket(socket, state, user_id))
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

    // Track admin event subscription
    let admin_subscribed: Arc<tokio::sync::RwLock<bool>> =
        Arc::new(tokio::sync::RwLock::new(false));

    // Update user presence to online
    if let Err(e) = update_presence(&state, user_id, "online").await {
        warn!("Failed to update presence: {}", e);
    }

    info!("WebSocket connected: user={}", user_id);

    // Send ready event
    let _ = tx.send(ServerEvent::Ready { user_id }).await;

    // Fetch user's friends for presence subscriptions
    let friend_ids = match get_user_friends(&state.db, user_id).await {
        Ok(friends) => {
            debug!(
                "User {} has {} friends for presence subscriptions",
                user_id,
                friends.len()
            );
            friends
        }
        Err(e) => {
            warn!("Failed to fetch friends for user {}: {}", user_id, e);
            Vec::new()
        }
    };

    // Fetch user's guild IDs for guild event subscriptions
    let guild_ids = match db::get_user_guild_ids(&state.db, user_id).await {
        Ok(guilds) => {
            debug!(
                "User {} is member of {} guilds for event subscriptions",
                user_id,
                guilds.len()
            );
            guilds
        }
        Err(e) => {
            warn!("Failed to fetch guilds for user {}: {}", user_id, e);
            Vec::new()
        }
    };

    // Load block sets for event filtering
    let blocked_ids = match block_cache::load_blocked_users(&state.db, &state.redis, user_id).await
    {
        Ok(ids) => {
            debug!("User {} has blocked {} users", user_id, ids.len());
            ids
        }
        Err(e) => {
            warn!("Failed to load blocked users for {}: {}", user_id, e);
            HashSet::new()
        }
    };
    let blocked_by_ids = match block_cache::load_blocked_by(&state.db, &state.redis, user_id).await
    {
        Ok(ids) => {
            debug!("User {} is blocked by {} users", user_id, ids.len());
            ids
        }
        Err(e) => {
            warn!("Failed to load blocked-by for {}: {}", user_id, e);
            HashSet::new()
        }
    };

    let blocked_users: Arc<tokio::sync::RwLock<HashSet<Uuid>>> = Arc::new(
        tokio::sync::RwLock::new(blocked_ids.union(&blocked_by_ids).copied().collect()),
    );

    // Spawn task to handle Redis pub/sub
    let redis_client = state.redis.clone();
    let tx_clone = tx.clone();
    let subscribed_clone = subscribed_channels.clone();
    let admin_subscribed_clone = admin_subscribed.clone();
    let blocked_clone = blocked_users.clone();
    let pubsub_handle = tokio::spawn(async move {
        handle_pubsub(
            redis_client,
            HandlePubsubParams {
                tx: tx_clone,
                subscribed_channels: subscribed_clone,
                admin_subscribed: admin_subscribed_clone,
                blocked_users: blocked_clone,
                user_id,
                friend_ids,
                guild_ids,
            },
        )
        .await;
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

            let send_result: Result<(), axum::Error> =
                ws_sender.send(Message::Text(msg.into())).await;
            if send_result.is_err() {
                break;
            }
        }
    });

    // Activity rate limiting state
    let mut activity_state = ActivityState::default();

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
                    &admin_subscribed,
                    &mut activity_state,
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
///
/// **Internal:** Exposed for integration tests only.
#[allow(clippy::implicit_hasher)]
#[tracing::instrument(
    skip(state, tx, subscribed_channels, admin_subscribed, activity_state, text),
    fields(user_id = %user_id)
)]
pub async fn handle_client_message(
    text: &str,
    user_id: Uuid,
    state: &AppState,
    tx: &mpsc::Sender<ServerEvent>,
    subscribed_channels: &Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    admin_subscribed: &Arc<tokio::sync::RwLock<bool>>,
    activity_state: &mut ActivityState,
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

            // Check if user has VIEW_CHANNEL permission
            if crate::permissions::require_channel_access(&state.db, user_id, channel_id)
                .await
                .is_err()
            {
                tx.send(ServerEvent::Error {
                    code: "forbidden".to_string(),
                    message: "You don't have permission to view this channel".to_string(),
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
            // Check if user has VIEW_CHANNEL permission
            let permission_result: Result<_, crate::permissions::PermissionError> =
                crate::permissions::require_channel_access(&state.db, user_id, channel_id).await;

            if permission_result.is_err() {
                warn!(
                    "User {} attempted to send typing indicator for channel {} without permission",
                    user_id, channel_id
                );
                return Ok(()); // Silently ignore unauthorized typing indicator
            }

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
            // Check if user has VIEW_CHANNEL permission
            let permission_result: Result<_, crate::permissions::PermissionError> =
                crate::permissions::require_channel_access(&state.db, user_id, channel_id).await;

            if permission_result.is_err() {
                warn!("User {} attempted to send stop typing indicator for channel {} without permission", user_id, channel_id);
                return Ok(()); // Silently ignore unauthorized stop typing indicator
            }

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
        | ClientEvent::VoiceStats { .. }
        | ClientEvent::VoiceScreenShareStart { .. }
        | ClientEvent::VoiceScreenShareStop { .. }
        | ClientEvent::VoiceWebcamStart { .. }
        | ClientEvent::VoiceWebcamStop { .. } => {
            if let Err(e) = crate::voice::ws_handler::handle_voice_event(
                &state.sfu,
                &state.db,
                &state.redis,
                user_id,
                event,
                tx,
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

        ClientEvent::SetActivity { activity } => {
            // Validate activity data if present
            if let Some(ref act) = activity {
                act.validate()
                    .map_err(|e| format!("Invalid activity: {e}"))?;
            }

            // Rate limiting: enforce minimum interval between updates
            let now = Instant::now();
            if let Some(last_update) = activity_state.last_update {
                let elapsed = now.duration_since(last_update);
                if elapsed < ACTIVITY_UPDATE_INTERVAL {
                    let remaining = ACTIVITY_UPDATE_INTERVAL.saturating_sub(elapsed);
                    return Err(format!(
                        "Rate limited: wait {} seconds before next activity update",
                        remaining.as_secs() + 1
                    )
                    .into());
                }
            }

            // Deduplication: skip update if activity is unchanged
            if activity == activity_state.last_activity {
                debug!("Skipping activity update: unchanged for user={}", user_id);
                return Ok(());
            }

            // Update database
            sqlx::query("UPDATE users SET activity = $1 WHERE id = $2")
                .bind(serde_json::to_value(&activity).ok())
                .bind(user_id)
                .execute(&state.db)
                .await
                .map_err(|e| format!("Failed to update activity: {e}"))?;

            // Update state for rate limiting and deduplication
            activity_state.last_update = Some(now);
            activity.clone_into(&mut activity_state.last_activity);

            // Broadcast to user's presence subscribers
            let event = ServerEvent::RichPresenceUpdate { user_id, activity };
            broadcast_presence_update(state, user_id, &event).await;
        }

        ClientEvent::AdminSubscribe => {
            // Check if user is an elevated admin
            let is_elevated =
                crate::admin::is_elevated_admin(&state.redis, &state.db, user_id).await;
            if !is_elevated {
                tx.send(ServerEvent::Error {
                    code: "admin_not_elevated".to_string(),
                    message: "Must be an elevated admin to subscribe to admin events".to_string(),
                })
                .await?;
                return Ok(());
            }

            *admin_subscribed.write().await = true;
            debug!("Admin {} subscribed to admin events", user_id);
        }

        ClientEvent::AdminUnsubscribe => {
            *admin_subscribed.write().await = false;
            debug!("Admin {} unsubscribed from admin events", user_id);
        }
    }

    Ok(())
}

/// Parameters for the Redis pub/sub handler.
struct HandlePubsubParams {
    tx: mpsc::Sender<ServerEvent>,
    subscribed_channels: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    admin_subscribed: Arc<tokio::sync::RwLock<bool>>,
    blocked_users: Arc<tokio::sync::RwLock<HashSet<Uuid>>>,
    user_id: Uuid,
    friend_ids: Vec<Uuid>,
    guild_ids: Vec<Uuid>,
}

/// Handle Redis pub/sub messages.
async fn handle_pubsub(redis: Client, params: HandlePubsubParams) {
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

    // Subscribe to user's own events channel (for preferences sync, etc.)
    let user_channel = channels::user_events(params.user_id);
    if let Err(e) = subscriber.subscribe(&user_channel).await {
        warn!("Failed to subscribe to user events channel: {}", e);
    } else {
        debug!("Subscribed to user events channel: {}", user_channel);
    }

    // Subscribe to admin events channel
    if let Err(e) = subscriber.subscribe(channels::ADMIN_EVENTS).await {
        warn!("Failed to subscribe to admin events: {}", e);
    } else {
        debug!("Subscribed to admin events channel");
    }

    // Subscribe to friends' presence channels
    for friend_id in &params.friend_ids {
        let presence_channel = channels::user_presence(*friend_id);
        if let Err(e) = subscriber.subscribe(&presence_channel).await {
            warn!(
                "Failed to subscribe to presence channel for friend {}: {}",
                friend_id, e
            );
        } else {
            debug!("Subscribed to presence channel: {}", presence_channel);
        }
    }

    // Subscribe to guild event channels for state sync
    for guild_id in &params.guild_ids {
        let guild_channel = channels::guild_events(*guild_id);
        if let Err(e) = subscriber.subscribe(&guild_channel).await {
            warn!(
                "Failed to subscribe to guild events channel for guild {}: {}",
                guild_id, e
            );
        } else {
            debug!("Subscribed to guild events channel: {}", guild_channel);
        }
    }

    while let Ok(message) = pubsub_stream.recv().await {
        let channel_name = message.channel.to_string();

        // Handle channel events (channel:{uuid})
        if let Some(uuid_str) = channel_name.strip_prefix("channel:") {
            if let Ok(channel_id) = Uuid::parse_str(uuid_str) {
                // Check if we're subscribed to this channel
                if params
                    .subscribed_channels
                    .read()
                    .await
                    .contains(&channel_id)
                {
                    // Parse and forward the event (with block filtering)
                    if let Some(payload) = message.value.as_str() {
                        if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                            // Filter events from blocked users
                            let blocked = params.blocked_users.read().await;
                            let should_filter = match &event {
                                ServerEvent::MessageNew { message, .. } => message
                                    .get("author")
                                    .and_then(|a| a.get("id"))
                                    .and_then(|id| id.as_str())
                                    .and_then(|id| Uuid::parse_str(id).ok())
                                    .is_some_and(|author_id| blocked.contains(&author_id)),
                                ServerEvent::TypingStart { user_id: uid, .. }
                                | ServerEvent::TypingStop { user_id: uid, .. }
                                | ServerEvent::VoiceUserJoined { user_id: uid, .. }
                                | ServerEvent::VoiceUserLeft { user_id: uid, .. }
                                | ServerEvent::CallParticipantJoined { user_id: uid, .. }
                                | ServerEvent::CallParticipantLeft { user_id: uid, .. } => {
                                    blocked.contains(uid)
                                }
                                _ => false,
                            };
                            drop(blocked);

                            if !should_filter && params.tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        // Handle user events (user:{uuid}) - for preferences sync across devices
        else if channel_name == user_channel {
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    // Handle block/unblock events to update in-memory set
                    match &event {
                        ServerEvent::UserBlocked {
                            user_id: blocked_id,
                        } => {
                            params.blocked_users.write().await.insert(*blocked_id);
                        }
                        ServerEvent::UserUnblocked {
                            user_id: unblocked_id,
                        } => {
                            params.blocked_users.write().await.remove(unblocked_id);
                        }
                        _ => {}
                    }

                    if params.tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }
        // Handle admin events
        else if channel_name == channels::ADMIN_EVENTS {
            // Only forward if user is subscribed to admin events
            if *params.admin_subscribed.read().await {
                if let Some(payload) = message.value.as_str() {
                    if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                        if params.tx.send(event).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
        // Handle presence events (presence:{uuid})
        else if channel_name.starts_with("presence:") {
            // Forward presence updates from friends (filter blocked users)
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    let should_filter = match &event {
                        ServerEvent::PresenceUpdate { user_id: uid, .. }
                        | ServerEvent::RichPresenceUpdate { user_id: uid, .. } => {
                            params.blocked_users.read().await.contains(uid)
                        }
                        _ => false,
                    };

                    if !should_filter && params.tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }
        // Handle user events (user:{uuid}) for cross-device sync
        else if channel_name.starts_with("user:") {
            // Forward all user-targeted events (read sync, etc.)
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    if params.tx.send(event).await.is_err() {
                        break;
                    }
                }
            }
        }
        // Handle guild events (guild:{uuid}) for state sync
        else if channel_name.starts_with("guild:") {
            // Forward guild/member patch events to all guild members
            if let Some(payload) = message.value.as_str() {
                if let Ok(event) = serde_json::from_str::<ServerEvent>(&payload) {
                    if params.tx.send(event).await.is_err() {
                        break;
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

/// Get list of user's accepted friend IDs.
async fn get_user_friends(db: &sqlx::PgPool, user_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    let friends: Vec<(Uuid,)> = sqlx::query_as(
        r"
        SELECT CASE
            WHEN requester_id = $1 THEN addressee_id
            ELSE requester_id
        END as friend_id
        FROM friendships
        WHERE (requester_id = $1 OR addressee_id = $1)
        AND status = 'accepted'
        ",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(friends.into_iter().map(|(id,)| id).collect())
}
