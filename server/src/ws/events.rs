//! WebSocket event types and broadcast helpers.
//!
//! Contains the `ClientEvent` (client → server) and `ServerEvent` (server → client)
//! enums, per-connection state types used by the message dispatcher, and the
//! Redis-backed broadcast helpers that other modules call to publish events.

use std::collections::HashMap;
use std::time::Instant;

use chrono::{DateTime, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;
use vc_common::protocol::PcType;

use crate::api::AppState;
use crate::voice::{Quality, ScreenShareInfo, WebcamInfo};

/// State for activity rate limiting and deduplication.
///
/// **Internal:** Exposed for integration tests only.
#[derive(Default)]
pub struct ActivityState {
    /// Last activity update timestamp.
    pub(super) last_update: Option<Instant>,
    /// Last activity data for deduplication.
    pub(super) last_activity: Option<crate::presence::Activity>,
}

/// State for custom status rate limiting and deduplication.
///
/// **Internal:** Exposed for integration tests only.
#[derive(Default)]
pub struct CustomStatusState {
    /// Last custom status update timestamp.
    pub(super) last_update: Option<Instant>,
    /// Last custom status data for deduplication.
    /// `None` = never sent, `Some(None)` = cleared, `Some(Some(..))` = active status.
    #[allow(clippy::option_option)]
    pub(super) last_custom_status: Option<Option<crate::presence::CustomStatus>>,
}

/// Bundled per-connection mutable state for client message handling.
///
/// **Internal:** Exposed for integration tests only.
#[derive(Default)]
pub struct ClientMessageState {
    /// Activity rate limiting and deduplication state.
    pub activity: ActivityState,
    /// Custom status rate limiting and deduplication state.
    pub custom_status: CustomStatusState,
    /// Per-channel typing throttle (`channel_id` → last typing broadcast time).
    pub last_typing: HashMap<Uuid, Instant>,
}

/// Default `pc_type` for backward compatibility with old clients.
pub(super) const fn default_pc_type() -> PcType {
    PcType::Publisher
}

/// Client-to-server events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    /// Post-connect authentication (replaces header-based auth for new clients).
    Authenticate {
        /// JWT access token.
        token: String,
    },
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
    /// Client sends SDP offer for publisher `PeerConnection` (mic, screen, webcam tracks)
    VoicePublisherOffer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP offer.
        sdp: String,
    },
    /// Client sends SDP answer for subscriber `PeerConnection` (receiving other users' tracks)
    VoiceSubscriberAnswer {
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
        /// Which `PeerConnection` this candidate belongs to.
        #[serde(default = "default_pc_type")]
        pc_type: PcType,
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
        /// Unique identifier for this screen share stream.
        stream_id: Uuid,
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
        /// Unique identifier for the screen share stream to stop.
        stream_id: Uuid,
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

    /// Set layer preference for a simulcast track
    VoiceSetLayerPreference {
        /// Voice channel.
        channel_id: Uuid,
        /// User whose track to adjust.
        target_user_id: Uuid,
        /// Track source identifier (e.g., `webcam`, `screen_video:<uuid>`).
        track_source: crate::voice::TrackSource,
        /// Desired layer preference.
        preferred_layer: crate::voice::LayerPreference,
    },

    /// Set rich presence activity (game, music, etc).
    SetActivity {
        activity: Option<crate::presence::Activity>,
    },

    /// Set user status (online, away, busy, offline).
    SetStatus { status: crate::db::UserStatus },

    /// Set or clear custom status (text + emoji + optional expiry).
    SetCustomStatus {
        custom_status: Option<crate::presence::CustomStatus>,
    },

    /// Subscribe to admin events (requires elevated admin).
    AdminSubscribe,
    /// Unsubscribe from admin events.
    AdminUnsubscribe,

    /// A user clicked a button / picked a select option on a bot message.
    ComponentInteraction {
        /// Message carrying the component.
        message_id: Uuid,
        /// The clicked component's `custom_id`.
        custom_id: String,
        /// Selected values (for select menus); empty for buttons.
        #[serde(default)]
        values: Vec<String>,
    },
}

impl ClientEvent {
    /// Return a low-cardinality static name for this event variant (for metrics).
    pub const fn variant_name(&self) -> &'static str {
        match self {
            Self::Authenticate { .. } => "authenticate",
            Self::Ping => "ping",
            Self::Subscribe { .. } => "subscribe",
            Self::Unsubscribe { .. } => "unsubscribe",
            Self::Typing { .. } => "typing",
            Self::StopTyping { .. } => "stop_typing",
            Self::VoiceJoin { .. } => "voice_join",
            Self::VoiceLeave { .. } => "voice_leave",
            Self::VoicePublisherOffer { .. } => "voice_publisher_offer",
            Self::VoiceSubscriberAnswer { .. } => "voice_subscriber_answer",
            Self::VoiceIceCandidate { .. } => "voice_ice_candidate",
            Self::VoiceMute { .. } => "voice_mute",
            Self::VoiceUnmute { .. } => "voice_unmute",
            Self::VoiceStats { .. } => "voice_stats",
            Self::VoiceScreenShareStart { .. } => "voice_screen_share_start",
            Self::VoiceScreenShareStop { .. } => "voice_screen_share_stop",
            Self::VoiceWebcamStart { .. } => "voice_webcam_start",
            Self::VoiceWebcamStop { .. } => "voice_webcam_stop",
            Self::VoiceSetLayerPreference { .. } => "voice_set_layer_preference",
            Self::SetActivity { .. } => "set_activity",
            Self::SetStatus { .. } => "set_status",
            Self::SetCustomStatus { .. } => "set_custom_status",
            Self::AdminSubscribe => "admin_subscribe",
            Self::AdminUnsubscribe => "admin_unsubscribe",
            Self::ComponentInteraction { .. } => "component_interaction",
        }
    }
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
    /// Message pinned in channel
    ChannelPinAdded {
        /// Channel containing the pinned message.
        channel_id: Uuid,
        /// Message that was pinned.
        message_id: Uuid,
        /// User who pinned the message.
        pinned_by: Uuid,
        /// When the message was pinned (RFC3339).
        pinned_at: String,
    },
    /// Message unpinned from channel
    ChannelPinRemoved {
        /// Channel the message was unpinned from.
        channel_id: Uuid,
        /// Message that was unpinned.
        message_id: Uuid,
    },
    /// A member's role set changed (self-assign or admin assign/remove).
    MemberRolesUpdated {
        /// Guild the member belongs to.
        guild_id: Uuid,
        /// The member whose roles changed.
        user_id: Uuid,
        /// The member's full role-ID set after the change (idempotent).
        role_ids: Vec<Uuid>,
    },
    /// A forum post was created in a forum channel.
    ForumPostCreated {
        /// Forum channel ID.
        channel_id: Uuid,
        /// The created post (serialized `ForumPostResponse`).
        post: serde_json::Value,
    },
    /// A forum post was updated (pinned/locked/activity).
    ForumPostUpdated {
        /// Forum channel ID.
        channel_id: Uuid,
        /// The updated post (serialized `ForumPostResponse`).
        post: serde_json::Value,
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
    /// Server sends SDP answer to client's publisher offer
    VoicePublisherAnswer {
        /// Voice channel.
        channel_id: Uuid,
        /// SDP answer.
        sdp: String,
    },
    /// Server sends SDP offer for subscriber `PeerConnection`
    VoiceSubscriberOffer {
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
        /// Which `PeerConnection` this candidate belongs to.
        pc_type: PcType,
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
        /// Unique identifier for this screen share stream.
        stream_id: Uuid,
        /// Username of sharer.
        username: String,
        /// Label of shared source.
        source_label: String,
        /// Whether audio is included.
        has_audio: bool,
        /// Quality tier.
        quality: Quality,
        /// When the screen share started (ISO 8601).
        started_at: String,
    },
    /// Screen share stopped
    ScreenShareStopped {
        /// Channel ID.
        channel_id: Uuid,
        /// User who stopped sharing.
        user_id: Uuid,
        /// Unique identifier for the stopped screen share stream.
        stream_id: Uuid,
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

    /// Active simulcast layer changed for a track subscription
    VoiceLayerChanged {
        /// Voice channel.
        channel_id: Uuid,
        /// User who owns the track.
        source_user_id: Uuid,
        /// Track source identifier.
        track_source: crate::voice::TrackSource,
        /// New active layer.
        active_layer: crate::voice::Layer,
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

    /// Custom status update for a user.
    CustomStatusUpdate {
        user_id: Uuid,
        custom_status: Option<crate::presence::CustomStatus>,
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

    /// Friend request rejected or cancelled (sent to the other party).
    FriendRequestRejected {
        /// Friendship ID that was rejected/cancelled.
        friendship_id: Uuid,
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

/// Internal outbound message envelope for the WebSocket sender task.
/// Separates serializable server events from raw WS control frames (Ping).
#[derive(Debug)]
pub enum OutboundMsg {
    Event(ServerEvent),
    Ping,
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

/// Broadcast a server event to all of a guild's subscribers via Redis.
#[tracing::instrument(skip(redis, event), fields(guild_id = %guild_id))]
pub async fn broadcast_to_guild(
    redis: &Client,
    guild_id: Uuid,
    event: &ServerEvent,
) -> Result<(), Error> {
    let payload = serde_json::to_string(event)
        .map_err(|e| Error::new(ErrorKind::Parse, format!("JSON error: {e}")))?;
    redis
        .publish::<(), _, _>(channels::guild_events(guild_id), payload)
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
pub(super) async fn broadcast_presence_update(
    state: &AppState,
    user_id: Uuid,
    event: &ServerEvent,
) {
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
