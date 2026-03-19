//! WebRTC Peer Connection Management
//!
//! Wraps `RTCPeerConnection` for each participant in a voice channel.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;
use webrtc::api::API;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::RTCRtpTransceiverInit;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

use super::error::VoiceError;
use super::track_types::TrackSource;
use crate::ws::OutboundMsg;

/// Represents a user's WebRTC connection to the SFU.
pub struct Peer {
    /// User ID.
    pub user_id: Uuid,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Channel ID the peer is connected to.
    pub channel_id: Uuid,
    /// The WebRTC peer connection.
    pub peer_connection: Arc<RTCPeerConnection>,
    /// The incoming tracks from this peer (mic, screen).
    /// Map: `TrackSource` -> remote track
    pub incoming_tracks: RwLock<HashMap<TrackSource, Arc<TrackRemote>>>,
    /// Tracks forwarded to this user (other participants' media).
    /// Map: `(source_user_id, source_type)` -> local track
    pub outgoing_tracks: RwLock<HashMap<(Uuid, TrackSource), Arc<TrackLocalStaticRTP>>>,
    /// Whether the user is muted.
    pub muted: RwLock<bool>,
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
}

impl Peer {
    /// Create a new peer with a WebRTC connection.
    pub async fn new(
        user_id: Uuid,
        username: String,
        display_name: String,
        channel_id: Uuid,
        api: &API,
        config: RTCConfiguration,
        signal_tx: mpsc::Sender<OutboundMsg>,
    ) -> Result<Self, VoiceError> {
        let peer_connection = api.new_peer_connection(config).await?;

        Ok(Self {
            user_id,
            username,
            display_name,
            channel_id,
            peer_connection: Arc::new(peer_connection),
            incoming_tracks: RwLock::new(HashMap::new()),
            outgoing_tracks: RwLock::new(HashMap::new()),
            muted: RwLock::new(false),
            signal_tx,
            session_id: Uuid::now_v7(),
            connected_at: Utc::now(),
            pending_track_sources: RwLock::new(Vec::new()),
        })
    }

    /// Add a recvonly transceiver for receiving media from the client.
    /// Used for pre-negotiating slots (e.g. for initial mic).
    pub async fn add_recv_transceiver(&self, kind: RTPCodecType) -> Result<(), VoiceError> {
        self.peer_connection
            .add_transceiver_from_kind(
                kind,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await?;
        Ok(())
    }

    /// Set an incoming track from this peer.
    pub async fn set_incoming_track(&self, source: TrackSource, track: Arc<TrackRemote>) {
        let mut incoming = self.incoming_tracks.write().await;
        incoming.insert(source, track);
    }

    /// Add an outgoing track to forward media from another user.
    /// Returns the `RTCRtpSender` so callers can read RTCP feedback (e.g. REMB).
    pub async fn add_outgoing_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
        track: Arc<TrackLocalStaticRTP>,
    ) -> Result<Arc<RTCRtpSender>, VoiceError> {
        // Add track to peer connection
        let sender = self
            .peer_connection
            .add_track(
                track.clone() as Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync>
            )
            .await?;

        // Store reference
        let mut tracks = self.outgoing_tracks.write().await;
        tracks.insert((source_user_id, source_type), track);

        Ok(sender)
    }

    /// Remove an outgoing track, also removing it from the peer connection.
    pub async fn remove_outgoing_track(
        &self,
        source_user_id: Uuid,
        source_type: TrackSource,
    ) -> bool {
        let mut tracks = self.outgoing_tracks.write().await;
        if let Some(track) = tracks.remove(&(source_user_id, source_type)) {
            // Remove from PeerConnection so the subscriber stops receiving it
            let senders = self.peer_connection.get_senders().await;
            for sender in senders {
                if let Some(t) = sender.track().await {
                    if t.id() == track.id() {
                        let _ = self.peer_connection.remove_track(&sender).await;
                        break;
                    }
                }
            }
            true
        } else {
            false
        }
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

    /// Check if the peer connection is connected.
    pub fn is_connected(&self) -> bool {
        matches!(
            self.peer_connection.ice_connection_state(),
            RTCIceConnectionState::Connected | RTCIceConnectionState::Completed
        )
    }

    /// Set mute state.
    pub async fn set_muted(&self, muted: bool) {
        let mut m = self.muted.write().await;
        *m = muted;
    }

    /// Get mute state.
    pub async fn is_muted(&self) -> bool {
        *self.muted.read().await
    }

    /// Close the peer connection.
    pub async fn close(&self) -> Result<(), VoiceError> {
        self.peer_connection.close().await?;
        Ok(())
    }
}
