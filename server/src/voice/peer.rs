//! WebRTC Peer Connection Management
//!
//! Wraps RTCPeerConnection for each participant in a voice channel.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;
use webrtc::{
    api::API,
    ice_transport::ice_connection_state::RTCIceConnectionState,
    peer_connection::{configuration::RTCConfiguration, RTCPeerConnection},
    rtp_transceiver::{
        rtp_codec::RTPCodecType, rtp_transceiver_direction::RTCRtpTransceiverDirection,
        RTCRtpTransceiverInit,
    },
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
    track::track_remote::TrackRemote,
};

use super::error::VoiceError;
use crate::ws::ServerEvent;

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
    /// The audio track this user is sending (their microphone).
    pub incoming_track: RwLock<Option<Arc<TrackRemote>>>,
    /// Audio tracks forwarded to this user (other participants' audio).
    /// Map: source_user_id -> local track
    pub outgoing_tracks: RwLock<HashMap<Uuid, Arc<TrackLocalStaticRTP>>>,
    /// Whether the user is muted.
    pub muted: RwLock<bool>,
    /// Channel to send signaling messages back to the user.
    pub signal_tx: mpsc::Sender<ServerEvent>,
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
        signal_tx: mpsc::Sender<ServerEvent>,
    ) -> Result<Self, VoiceError> {
        let peer_connection = api.new_peer_connection(config).await?;

        Ok(Self {
            user_id,
            username,
            display_name,
            channel_id,
            peer_connection: Arc::new(peer_connection),
            incoming_track: RwLock::new(None),
            outgoing_tracks: RwLock::new(HashMap::new()),
            muted: RwLock::new(false),
            signal_tx,
        })
    }

    /// Add a recvonly transceiver for receiving audio from the client.
    pub async fn add_recv_transceiver(&self) -> Result<(), VoiceError> {
        self.peer_connection
            .add_transceiver_from_kind(
                RTPCodecType::Audio,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Recvonly,
                    send_encodings: vec![],
                }),
            )
            .await?;
        Ok(())
    }

    /// Set the incoming track from this peer.
    pub async fn set_incoming_track(&self, track: Arc<TrackRemote>) {
        let mut incoming = self.incoming_track.write().await;
        *incoming = Some(track);
    }

    /// Add an outgoing track to forward audio from another user.
    pub async fn add_outgoing_track(
        &self,
        source_user_id: Uuid,
        track: Arc<TrackLocalStaticRTP>,
    ) -> Result<(), VoiceError> {
        // Add track to peer connection
        self.peer_connection
            .add_track(track.clone() as Arc<dyn webrtc::track::track_local::TrackLocal + Send + Sync>)
            .await?;

        // Store reference
        let mut tracks = self.outgoing_tracks.write().await;
        tracks.insert(source_user_id, track);

        Ok(())
    }

    /// Remove an outgoing track when a user leaves.
    pub async fn remove_outgoing_track(&self, source_user_id: Uuid) {
        let mut tracks = self.outgoing_tracks.write().await;
        tracks.remove(&source_user_id);
        // Note: Track removal from PeerConnection requires renegotiation
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
