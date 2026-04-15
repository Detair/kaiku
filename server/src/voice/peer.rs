//! WebRTC Peer Connection Management
//!
//! Each participant has two `PeerConnection`s:
//! - **`publisher_pc`**: receives tracks FROM the client (mic, screen, webcam). The client creates
//!   offers for this connection.
//! - **`subscriber_pc`**: sends tracks TO the client (other users' audio, screen shares, webcams).
//!   The server creates offers for this connection.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

use super::track_types::TrackSource;
use crate::ws::OutboundMsg;

/// Represents a user's WebRTC connection to the SFU.
///
/// Uses two `PeerConnection`s (dual-PC architecture) to cleanly separate
/// upstream (publish) and downstream (subscribe) media flows.
pub struct Peer {
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Channel ID the peer is connected to.
    pub channel_id: Uuid,

    /// Receives tracks FROM client (mic, screen, webcam). Client creates offers.
    pub publisher_pc: Arc<RTCPeerConnection>,
    /// Sends tracks TO client (other users' audio, screen shares). Server creates offers.
    pub subscriber_pc: Arc<RTCPeerConnection>,

    /// Tracks received from this peer (on `publisher_pc`).
    /// Map: `TrackSource` -> remote track
    pub incoming_tracks: RwLock<HashMap<TrackSource, Arc<TrackRemote>>>,
    /// Tracks being forwarded to this peer (on `subscriber_pc`).
    /// Map: `(source_user_id, source_type)` -> local track
    outgoing_tracks: RwLock<HashMap<(Uuid, TrackSource), Arc<TrackLocalStaticRTP>>>,

    /// Whether the user muted themselves.
    self_muted: RwLock<bool>,
    /// Whether a moderator muted the user. Set only by future moderation events.
    #[allow(dead_code)]
    server_muted: RwLock<bool>,
    /// Channel to send signaling messages back to the user.
    pub signal_tx: mpsc::Sender<OutboundMsg>,
    /// Unique session identifier for this connection.
    pub session_id: Uuid,
    /// Timestamp when this peer connected.
    pub connected_at: DateTime<Utc>,
    /// Pending track sources queued by the client before tracks arrive.
    /// The client sends e.g. `VoiceWebcamStart` before `addTrack()`, so the
    /// server can pop from this queue when `on_track` fires to identify the source.
    pending_track_sources: RwLock<Vec<TrackSource>>,
    /// Whether this peer has already subscribed to existing tracks.
    /// Used for robust first-offer detection instead of checking `outgoing_tracks_count`.
    pub has_subscribed: AtomicBool,
}

impl Peer {
    /// Create a new peer with two WebRTC connections (publisher + subscriber).
    pub fn new(
        user_id: Uuid,
        username: String,
        display_name: String,
        channel_id: Uuid,
        publisher_pc: Arc<RTCPeerConnection>,
        subscriber_pc: Arc<RTCPeerConnection>,
        signal_tx: mpsc::Sender<OutboundMsg>,
    ) -> Self {
        Self {
            user_id,
            username,
            display_name,
            channel_id,
            publisher_pc,
            subscriber_pc,
            incoming_tracks: RwLock::new(HashMap::new()),
            outgoing_tracks: RwLock::new(HashMap::new()),
            self_muted: RwLock::new(false),
            server_muted: RwLock::new(false),
            signal_tx,
            session_id: Uuid::now_v7(),
            connected_at: Utc::now(),
            pending_track_sources: RwLock::new(Vec::new()),
            has_subscribed: AtomicBool::new(false),
        }
    }

    /// Set an incoming track from this peer.
    pub async fn set_incoming_track(&self, source: TrackSource, track: Arc<TrackRemote>) {
        let mut incoming = self.incoming_tracks.write().await;
        incoming.insert(source, track);
    }

    /// Add an outgoing track to forward media from another user.
    /// The track is added to the subscriber `PeerConnection`.
    /// Returns the `RTCRtpSender` so callers can read RTCP feedback (e.g. REMB).
    pub async fn add_outgoing_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        track: Arc<TrackLocalStaticRTP>,
    ) -> Result<Arc<RTCRtpSender>, super::error::VoiceError> {
        // Add track to subscriber peer connection
        let sender = self
            .subscriber_pc
            .add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        // Store reference
        let mut tracks = self.outgoing_tracks.write().await;
        tracks.insert((source_user_id, source_type), track);

        Ok(sender)
    }

    /// Remove an outgoing track, also removing it from the subscriber peer connection.
    pub async fn remove_outgoing_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
    ) -> bool {
        let mut tracks = self.outgoing_tracks.write().await;
        if let Some(track) = tracks.remove(&(source_user_id, source_type)) {
            // Remove from subscriber PeerConnection so the subscriber stops receiving it
            let senders = self.subscriber_pc.get_senders().await;
            for sender in senders {
                if let Some(t) = sender.track().await {
                    if t.id() == track.id() {
                        if let Err(e) = self.subscriber_pc.remove_track(&sender).await {
                            tracing::warn!(
                                user_id = %self.user_id,
                                error = %e,
                                "failed to remove track from subscriber PC"
                            );
                        }
                        break;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Returns the number of outgoing tracks on the subscriber `PeerConnection`.
    pub async fn outgoing_tracks_count(&self) -> usize {
        self.outgoing_tracks.read().await.len()
    }

    /// Enqueue an expected track source. Called before the client's `addTrack()`
    /// so that `on_track` can identify the source correctly.
    pub async fn push_pending_source(&self, source: TrackSource) {
        let mut pending = self.pending_track_sources.write().await;
        pending.push(source);
    }

    /// Dequeue the first pending video source, if any.
    pub async fn pop_pending_video_source(&self) -> Option<TrackSource> {
        let mut pending = self.pending_track_sources.write().await;
        pending
            .iter()
            .position(|s| s.is_video())
            .map(|pos| pending.remove(pos))
    }

    /// Dequeue the first pending audio source, if any.
    pub async fn pop_pending_audio_source(&self) -> Option<TrackSource> {
        let mut pending = self.pending_track_sources.write().await;
        pending
            .iter()
            .position(|s| s.is_audio())
            .map(|pos| pending.remove(pos))
    }

    /// Check if either peer connection is connected.
    pub fn is_connected(&self) -> bool {
        self.publisher_pc.connection_state() == RTCPeerConnectionState::Connected
            || self.subscriber_pc.connection_state() == RTCPeerConnectionState::Connected
    }

    /// Set the user's own mute state (does not affect `server_muted`).
    pub async fn set_self_muted(&self, muted: bool) {
        let mut m = self.self_muted.write().await;
        *m = muted;
    }

    /// Returns true if the user has muted themselves (ignores moderator mute).
    pub async fn is_self_muted(&self) -> bool {
        *self.self_muted.read().await
    }

    /// Returns true if either the user self-muted or a moderator muted them.
    pub async fn is_effectively_muted(&self) -> bool {
        *self.self_muted.read().await || *self.server_muted.read().await
    }

    /// Close both peer connections.
    pub async fn close(&self) {
        if let Err(e) = self.publisher_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close publisher PC");
        }
        if let Err(e) = self.subscriber_pc.close().await {
            tracing::warn!(user_id = %self.user_id, error = %e, "failed to close subscriber PC");
        }
    }
}
