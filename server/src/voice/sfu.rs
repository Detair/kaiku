//! Selective Forwarding Unit Implementation
//!
//! Manages voice rooms and WebRTC peer connections for real-time audio.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;
use webrtc::{
    api::{interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder, API},
    ice_transport::ice_server::RTCIceServer,
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
};

use super::error::VoiceError;
use super::peer::Peer;
use super::track::{spawn_rtp_forwarder, TrackRouter};
use crate::config::Config;
use crate::ws::ServerEvent;

/// Default maximum participants per room.
const DEFAULT_MAX_PARTICIPANTS: usize = 25;

/// Participant info for room state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParticipantInfo {
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
}

/// Voice channel room with all participants.
pub struct Room {
    /// Channel ID.
    pub channel_id: Uuid,
    /// Connected peers.
    pub peers: RwLock<HashMap<Uuid, Arc<Peer>>>,
    /// Track router for RTP forwarding.
    pub track_router: Arc<TrackRouter>,
    /// Maximum participants allowed.
    pub max_participants: usize,
}

impl Room {
    /// Create a new room.
    pub fn new(channel_id: Uuid, max_participants: usize) -> Self {
        Self {
            channel_id,
            peers: RwLock::new(HashMap::new()),
            track_router: Arc::new(TrackRouter::new()),
            max_participants,
        }
    }

    /// Add a peer to the room.
    pub async fn add_peer(&self, peer: Arc<Peer>) -> Result<(), VoiceError> {
        let mut peers = self.peers.write().await;

        if peers.len() >= self.max_participants {
            return Err(VoiceError::ChannelFull {
                max_participants: self.max_participants,
            });
        }

        if peers.contains_key(&peer.user_id) {
            return Err(VoiceError::AlreadyJoined);
        }

        peers.insert(peer.user_id, peer);
        Ok(())
    }

    /// Remove a peer from the room.
    pub async fn remove_peer(&self, user_id: Uuid) -> Option<Arc<Peer>> {
        let mut peers = self.peers.write().await;
        let peer = peers.remove(&user_id);

        if peer.is_some() {
            // Clean up track subscriptions
            self.track_router.remove_source(user_id).await;
            self.track_router.remove_subscriber_from_all(user_id).await;
        }

        peer
    }

    /// Get a peer by user ID.
    pub async fn get_peer(&self, user_id: Uuid) -> Option<Arc<Peer>> {
        let peers = self.peers.read().await;
        peers.get(&user_id).cloned()
    }

    /// Get all peers except one.
    pub async fn get_other_peers(&self, exclude_user_id: Uuid) -> Vec<Arc<Peer>> {
        let peers = self.peers.read().await;
        peers
            .iter()
            .filter(|(id, _)| **id != exclude_user_id)
            .map(|(_, peer)| peer.clone())
            .collect()
    }

    /// Get participant info for all peers.
    pub async fn get_participant_info(&self) -> Vec<ParticipantInfo> {
        let peers = self.peers.read().await;
        let mut info = Vec::with_capacity(peers.len());

        for (user_id, peer) in peers.iter() {
            info.push(ParticipantInfo {
                user_id: *user_id,
                username: Some(peer.username.clone()),
                display_name: Some(peer.display_name.clone()),
                muted: peer.is_muted().await,
            });
        }

        info
    }

    /// Broadcast an event to all peers except one.
    pub async fn broadcast_except(&self, exclude_user_id: Uuid, event: ServerEvent) {
        let peers = self.peers.read().await;

        for (user_id, peer) in peers.iter() {
            if *user_id != exclude_user_id {
                if let Err(e) = peer.signal_tx.send(event.clone()).await {
                    warn!(user_id = %user_id, error = %e, "Failed to send event to peer");
                }
            }
        }
    }

    /// Broadcast an event to all peers.
    #[allow(dead_code)]
    pub async fn broadcast_all(&self, event: ServerEvent) {
        let peers = self.peers.read().await;

        for (user_id, peer) in peers.iter() {
            if let Err(e) = peer.signal_tx.send(event.clone()).await {
                warn!(user_id = %user_id, error = %e, "Failed to send event to peer");
            }
        }
    }

    /// Get participant count.
    pub async fn participant_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Check if room is empty.
    pub async fn is_empty(&self) -> bool {
        self.peers.read().await.is_empty()
    }
}

/// SFU Server managing all voice rooms.
pub struct SfuServer {
    /// Active rooms.
    rooms: Arc<RwLock<HashMap<Uuid, Arc<Room>>>>,
    /// WebRTC API instance.
    api: Arc<API>,
    /// Server configuration.
    config: Arc<Config>,
}

impl SfuServer {
    /// Create a new SFU server.
    pub fn new(config: Arc<Config>) -> Result<Self, VoiceError> {
        // Configure MediaEngine with Opus audio codec
        let mut media_engine = MediaEngine::default();

        // Register Opus codec for audio
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "audio/opus".to_string(),
                        clock_rate: 48000,
                        channels: 2,
                        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                        rtcp_feedback: vec![],
                    },
                    payload_type: 111,
                    ..Default::default()
                },
                RTPCodecType::Audio,
            )
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Create interceptor registry
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Build WebRTC API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        info!("SFU server initialized");

        Ok(Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            api: Arc::new(api),
            config,
        })
    }

    /// Get RTCConfiguration with ICE servers from config.
    pub fn rtc_config(&self) -> RTCConfiguration {
        let mut ice_servers = vec![RTCIceServer {
            urls: vec![self.config.stun_server.clone()],
            ..Default::default()
        }];

        // Add TURN server if configured
        if let Some(turn) = &self.config.turn_server {
            ice_servers.push(RTCIceServer {
                urls: vec![turn.clone()],
                username: self.config.turn_username.clone().unwrap_or_default(),
                credential: self.config.turn_credential.clone().unwrap_or_default(),
                ..Default::default()
            });
        }

        RTCConfiguration {
            ice_servers,
            ..Default::default()
        }
    }

    /// Get or create a room for a channel.
    pub async fn get_or_create_room(&self, channel_id: Uuid) -> Arc<Room> {
        let mut rooms = self.rooms.write().await;

        if let Some(room) = rooms.get(&channel_id) {
            return room.clone();
        }

        let room = Arc::new(Room::new(channel_id, DEFAULT_MAX_PARTICIPANTS));
        rooms.insert(channel_id, room.clone());

        debug!(channel_id = %channel_id, "Created new voice room");

        room
    }

    /// Get a room by channel ID.
    pub async fn get_room(&self, channel_id: Uuid) -> Option<Arc<Room>> {
        let rooms = self.rooms.read().await;
        rooms.get(&channel_id).cloned()
    }

    /// Remove a room if empty.
    pub async fn cleanup_room_if_empty(&self, channel_id: Uuid) {
        let mut rooms = self.rooms.write().await;

        if let Some(room) = rooms.get(&channel_id) {
            if room.is_empty().await {
                rooms.remove(&channel_id);
                debug!(channel_id = %channel_id, "Removed empty voice room");
            }
        }
    }

    /// Create a new peer connection for a user.
    pub async fn create_peer(
        &self,
        user_id: Uuid,
        username: String,
        display_name: String,
        channel_id: Uuid,
        signal_tx: mpsc::Sender<ServerEvent>,
    ) -> Result<Arc<Peer>, VoiceError> {
        let config = self.rtc_config();
        let peer = Peer::new(user_id, username, display_name, channel_id, &self.api, config, signal_tx).await?;
        let peer = Arc::new(peer);

        // Set up connection state handler
        let peer_weak = Arc::downgrade(&peer);
        let uid = user_id;
        let cid = channel_id;

        peer.peer_connection
            .on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
                let pw = peer_weak.clone();
                Box::pin(async move {
                    debug!(
                        user_id = %uid,
                        channel_id = %cid,
                        state = ?state,
                        "Peer connection state changed"
                    );

                    match state {
                        RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected => {
                            if let Some(_peer) = pw.upgrade() {
                                // Peer will be cleaned up by the handler
                                warn!(user_id = %uid, "Peer connection failed/disconnected");
                            }
                        }
                        _ => {}
                    }
                })
            }));

        Ok(peer)
    }

    /// Set up track handling for a peer.
    pub fn setup_track_handler(&self, peer: &Arc<Peer>, room: &Arc<Room>) {
        let peer_weak = Arc::downgrade(peer);
        let room_weak = Arc::downgrade(room);
        let user_id = peer.user_id;
        let channel_id = peer.channel_id;

        peer.peer_connection.on_track(Box::new(
            move |track, _receiver, _transceiver| {
                let pw = peer_weak.clone();
                let rw = room_weak.clone();
                let uid = user_id;
                let cid = channel_id;

                Box::pin(async move {
                    info!(
                        user_id = %uid,
                        channel_id = %cid,
                        track_id = %track.id(),
                        kind = ?track.kind(),
                        "Received track from peer"
                    );

                    if let (Some(peer), Some(room)) = (pw.upgrade(), rw.upgrade()) {
                        // Store incoming track
                        peer.set_incoming_track(track.clone()).await;

                        // Start RTP forwarder
                        spawn_rtp_forwarder(uid, track.clone(), room.track_router.clone());

                        // Create subscriber tracks for all existing peers
                        let other_peers = room.get_other_peers(uid).await;
                        for other_peer in other_peers {
                            if let Ok(local_track) = room
                                .track_router
                                .create_subscriber_track(uid, &other_peer, &track)
                                .await
                            {
                                if let Err(e) = other_peer.add_outgoing_track(uid, local_track).await {
                                    warn!(
                                        source = %uid,
                                        subscriber = %other_peer.user_id,
                                        error = %e,
                                        "Failed to add outgoing track"
                                    );
                                }
                            }
                        }
                    }
                })
            },
        ));
    }

    /// Set up ICE candidate handler for a peer.
    pub fn setup_ice_handler(&self, peer: &Arc<Peer>) {
        let signal_tx = peer.signal_tx.clone();
        let channel_id = peer.channel_id;

        peer.peer_connection.on_ice_candidate(Box::new(move |candidate| {
            let tx = signal_tx.clone();
            let cid = channel_id;

            Box::pin(async move {
                if let Some(c) = candidate {
                    match c.to_json() {
                        Ok(json) => {
                            if let Ok(candidate_str) = serde_json::to_string(&json) {
                                let _ = tx
                                    .send(ServerEvent::VoiceIceCandidate {
                                        channel_id: cid,
                                        candidate: candidate_str,
                                    })
                                    .await;
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to serialize ICE candidate");
                        }
                    }
                }
            })
        }));
    }

    /// Create an offer for a peer.
    pub async fn create_offer(&self, peer: &Peer) -> Result<RTCSessionDescription, VoiceError> {
        let offer = peer.peer_connection.create_offer(None).await?;
        peer.peer_connection.set_local_description(offer.clone()).await?;
        Ok(offer)
    }

    /// Handle an answer from a peer.
    pub async fn handle_answer(
        &self,
        peer: &Peer,
        sdp: &str,
    ) -> Result<(), VoiceError> {
        let answer = RTCSessionDescription::answer(sdp.to_string())
            .map_err(|e| VoiceError::Signaling(e.to_string()))?;

        peer.peer_connection.set_remote_description(answer).await?;
        Ok(())
    }

    /// Handle an ICE candidate from a peer.
    pub async fn handle_ice_candidate(
        &self,
        peer: &Peer,
        candidate_str: &str,
    ) -> Result<(), VoiceError> {
        let candidate: webrtc::ice_transport::ice_candidate::RTCIceCandidateInit =
            serde_json::from_str(candidate_str)
                .map_err(|e| VoiceError::Signaling(format!("Invalid ICE candidate: {}", e)))?;

        peer.peer_connection
            .add_ice_candidate(candidate)
            .await?;

        Ok(())
    }

    /// Get active room count.
    #[allow(dead_code)]
    pub async fn room_count(&self) -> usize {
        self.rooms.read().await.len()
    }
}
