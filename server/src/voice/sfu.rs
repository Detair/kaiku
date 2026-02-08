//! Selective Forwarding Unit Implementation
//!
//! Manages voice rooms and WebRTC peer connections for real-time audio.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::rtp_transceiver::RTCPFeedback;

use super::error::VoiceError;
use super::peer::Peer;
use super::rate_limit::VoiceStatsLimiter;
use super::screen_share::ScreenShareInfo;
use super::track::{spawn_rtp_forwarder, TrackRouter};
use super::track_types::TrackSource;
use super::webcam::WebcamInfo;
use crate::config::Config;
use crate::ratelimit::{RateLimitCategory, RateLimiter};
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
    /// Whether the user is screen sharing.
    #[serde(default)]
    pub screen_sharing: bool,
    /// Whether the user has their webcam active.
    #[serde(default)]
    pub webcam_active: bool,
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
    /// Active screen shares.
    pub screen_shares: RwLock<HashMap<Uuid, ScreenShareInfo>>,
    /// Active webcams.
    pub webcams: RwLock<HashMap<Uuid, WebcamInfo>>,
}

impl Room {
    /// Create a new room.
    #[must_use]
    pub fn new(channel_id: Uuid, max_participants: usize) -> Self {
        Self {
            channel_id,
            peers: RwLock::new(HashMap::new()),
            track_router: Arc::new(TrackRouter::new()),
            max_participants,
            screen_shares: RwLock::new(HashMap::new()),
            webcams: RwLock::new(HashMap::new()),
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

    /// Add a screen share session.
    pub async fn add_screen_share(&self, info: ScreenShareInfo) {
        let mut shares = self.screen_shares.write().await;
        shares.insert(info.user_id, info);
    }

    /// Remove a screen share session.
    pub async fn remove_screen_share(&self, user_id: Uuid) -> Option<ScreenShareInfo> {
        let mut shares = self.screen_shares.write().await;
        shares.remove(&user_id)
    }

    /// Get all screen shares.
    pub async fn get_screen_shares(&self) -> Vec<ScreenShareInfo> {
        let shares = self.screen_shares.read().await;
        shares.values().cloned().collect()
    }

    /// Add a webcam session.
    pub async fn add_webcam(&self, info: WebcamInfo) {
        let mut webcams = self.webcams.write().await;
        webcams.insert(info.user_id, info);
    }

    /// Remove a webcam session.
    pub async fn remove_webcam(&self, user_id: Uuid) -> Option<WebcamInfo> {
        let mut webcams = self.webcams.write().await;
        webcams.remove(&user_id)
    }

    /// Get all active webcams.
    pub async fn get_webcams(&self) -> Vec<WebcamInfo> {
        let webcams = self.webcams.read().await;
        webcams.values().cloned().collect()
    }

    /// Get participant info for all peers.
    pub async fn get_participant_info(&self) -> Vec<ParticipantInfo> {
        let peers = self.peers.read().await;
        let shares = self.screen_shares.read().await;
        let webcams = self.webcams.read().await;
        let mut info = Vec::with_capacity(peers.len());

        for (user_id, peer) in peers.iter() {
            info.push(ParticipantInfo {
                user_id: *user_id,
                username: Some(peer.username.clone()),
                display_name: Some(peer.display_name.clone()),
                muted: peer.is_muted().await,
                screen_sharing: shares.contains_key(user_id),
                webcam_active: webcams.contains_key(user_id),
            });
        }

        info
    }

    /// Broadcast an event to all peers except one.
    ///
    /// Clones the peer list before sending to avoid holding the lock during I/O,
    /// which could delay peer additions/removals during broadcasts.
    pub async fn broadcast_except(&self, exclude_user_id: Uuid, event: ServerEvent) {
        // Clone sender handles to release lock before I/O
        let senders: Vec<(Uuid, mpsc::Sender<ServerEvent>)> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .filter(|(id, _)| **id != exclude_user_id)
                .map(|(id, peer)| (*id, peer.signal_tx.clone()))
                .collect()
        };

        // Send without holding the lock
        for (user_id, tx) in senders {
            if let Err(e) = tx.send(event.clone()).await {
                warn!(user_id = %user_id, error = %e, "Failed to send event to peer");
            }
        }
    }

    /// Broadcast an event to all peers.
    ///
    /// Clones the peer list before sending to avoid holding the lock during I/O.
    #[allow(dead_code)]
    pub async fn broadcast_all(&self, event: ServerEvent) {
        // Clone sender handles to release lock before I/O
        let senders: Vec<(Uuid, mpsc::Sender<ServerEvent>)> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .map(|(id, peer)| (*id, peer.signal_tx.clone()))
                .collect()
        };

        // Send without holding the lock
        for (user_id, tx) in senders {
            if let Err(e) = tx.send(event.clone()).await {
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
    /// Rate limiter for voice operations (global/redis).
    rate_limiter: Option<Arc<RateLimiter>>,
    /// Rate limiter for voice stats (local/memory).
    stats_limiter: Arc<VoiceStatsLimiter>,
}

impl SfuServer {
    /// Create a new SFU server.
    pub fn new(config: Arc<Config>, rate_limiter: Option<RateLimiter>) -> Result<Self, VoiceError> {
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

        // Register VP9 video codec (preferred)
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/VP9".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line: "profile-id=0".to_string(),
                        rtcp_feedback: vec![
                            RTCPFeedback {
                                typ: "goog-remb".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "ccm".to_string(),
                                parameter: "fir".to_string(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: "pli".to_string(),
                            },
                        ],
                    },
                    payload_type: 98,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Register VP8 video codec (fallback)
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/VP8".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line: String::new(),
                        rtcp_feedback: vec![
                            RTCPFeedback {
                                typ: "goog-remb".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "ccm".to_string(),
                                parameter: "fir".to_string(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: "pli".to_string(),
                            },
                        ],
                    },
                    payload_type: 96,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Register H.264 video codec (for desktop clients with hardware encoding)
        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/H264".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line:
                            "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                                .to_string(),
                        rtcp_feedback: vec![
                            RTCPFeedback {
                                typ: "goog-remb".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "ccm".to_string(),
                                parameter: "fir".to_string(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: String::new(),
                            },
                            RTCPFeedback {
                                typ: "nack".to_string(),
                                parameter: "pli".to_string(),
                            },
                        ],
                    },
                    payload_type: 102,
                    ..Default::default()
                },
                RTPCodecType::Video,
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
            rate_limiter: rate_limiter.map(Arc::new),
            stats_limiter: Arc::new(VoiceStatsLimiter::default()),
        })
    }

    /// Start background cleanup task for voice stats rate limiter.
    /// This should be called once after server initialization to prevent memory leaks.
    /// Returns a handle to the spawned task.
    pub fn start_cleanup_task(&self) -> tokio::task::JoinHandle<()> {
        self.stats_limiter.start_cleanup_task()
    }

    /// Get `RTCConfiguration` with ICE servers from config.
    #[must_use]
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
        let peer = Peer::new(
            user_id,
            username,
            display_name,
            channel_id,
            &self.api,
            config,
            signal_tx,
        )
        .await?;
        let peer = Arc::new(peer);

        // Add recvonly transceivers
        // Always add Audio (mic)
        peer.add_recv_transceiver(RTPCodecType::Audio).await?;
        // Always add Video (screen) to prepare m-lines
        peer.add_recv_transceiver(RTPCodecType::Video).await?;

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

        peer.peer_connection
            .on_track(Box::new(move |track, _receiver, _transceiver| {
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

                    // Upgrade weak references once — use for both source resolution and track setup
                    let (peer, room) = match (pw.upgrade(), rw.upgrade()) {
                        (Some(p), Some(r)) => (p, r),
                        _ => return,
                    };

                    // Determine source type: check pending queue first, fall back to defaults
                    let source_type = match track.kind() {
                        RTPCodecType::Audio => peer
                            .pop_pending_audio_source()
                            .await
                            .unwrap_or(TrackSource::Microphone),
                        RTPCodecType::Video => peer
                            .pop_pending_video_source()
                            .await
                            .unwrap_or(TrackSource::ScreenVideo),
                        RTPCodecType::Unspecified => {
                            warn!("Unspecified track kind: {:?}", track.kind());
                            return;
                        }
                    };

                    // Store incoming track
                    peer.set_incoming_track(source_type, track.clone()).await;

                    // Start RTP forwarder
                    spawn_rtp_forwarder(uid, source_type, track.clone(), room.track_router.clone());

                    // Create subscriber tracks for all existing peers
                    let other_peers = room.get_other_peers(uid).await;
                    for other_peer in other_peers {
                        if let Ok(local_track) = room
                            .track_router
                            .create_subscriber_track(uid, source_type, &other_peer, &track)
                            .await
                        {
                            if let Err(e) = other_peer
                                .add_outgoing_track(uid, source_type, local_track)
                                .await
                            {
                                warn!(
                                    source = %uid,
                                    subscriber = %other_peer.user_id,
                                    error = %e,
                                    "Failed to add outgoing track"
                                );
                            } else {
                                // Renegotiate so subscriber receives updated SDP
                                if let Err(e) = Self::renegotiate(&other_peer).await {
                                    warn!(
                                        subscriber = %other_peer.user_id,
                                        error = %e,
                                        "Renegotiation failed after track add"
                                    );
                                }
                            }
                        }
                    }
                })
            }));
    }

    /// Set up ICE candidate handler for a peer.
    pub fn setup_ice_handler(&self, peer: &Arc<Peer>) {
        let signal_tx = peer.signal_tx.clone();
        let channel_id = peer.channel_id;

        peer.peer_connection
            .on_ice_candidate(Box::new(move |candidate| {
                let tx = signal_tx.clone();
                let cid = channel_id;

                Box::pin(async move {
                    if let Some(c) = candidate {
                        match c.to_json() {
                            Ok(json) => {
                                if let Ok(candidate_str) = serde_json::to_string(&json) {
                                    if let Err(e) = tx
                                        .send(ServerEvent::VoiceIceCandidate {
                                            channel_id: cid,
                                            candidate: candidate_str,
                                        })
                                        .await
                                    {
                                        tracing::error!(
                                            channel_id = %cid,
                                            error = %e,
                                            "Failed to send ICE candidate - connection may fail"
                                        );
                                    }
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

    /// Trigger renegotiation by creating a new offer and sending it to the peer.
    /// Used after dynamically adding/removing tracks mid-session.
    pub async fn renegotiate(peer: &Peer) -> Result<(), VoiceError> {
        let offer = peer.peer_connection.create_offer(None).await?;
        peer.peer_connection
            .set_local_description(offer.clone())
            .await?;
        peer.signal_tx
            .send(ServerEvent::VoiceOffer {
                channel_id: peer.channel_id,
                sdp: offer.sdp,
            })
            .await
            .map_err(|e| VoiceError::Signaling(e.to_string()))?;
        Ok(())
    }

    /// Create an offer for a peer.
    pub async fn create_offer(&self, peer: &Peer) -> Result<RTCSessionDescription, VoiceError> {
        let offer = peer.peer_connection.create_offer(None).await?;
        peer.peer_connection
            .set_local_description(offer.clone())
            .await?;
        Ok(offer)
    }

    /// Handle an answer from a peer.
    pub async fn handle_answer(&self, peer: &Peer, sdp: &str) -> Result<(), VoiceError> {
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
                .map_err(|e| VoiceError::Signaling(format!("Invalid ICE candidate: {e}")))?;

        peer.peer_connection.add_ice_candidate(candidate).await?;

        Ok(())
    }

    /// Check if a user can join voice (rate limit check).
    pub async fn check_rate_limit(&self, user_id: Uuid) -> Result<(), VoiceError> {
        if let Some(limiter) = &self.rate_limiter {
            // Note: We use user_id as identifier (stringified)
            let result = limiter
                .check(RateLimitCategory::VoiceJoin, &user_id.to_string())
                .await
                .map_err(|e| VoiceError::Internal(e.to_string()))?;

            if !result.allowed {
                return Err(VoiceError::RateLimited);
            }
        }
        Ok(())
    }

    /// Check if a user can report voice stats (rate limit check).
    pub async fn check_stats_rate_limit(&self, user_id: Uuid) -> Result<(), VoiceError> {
        self.stats_limiter.check_stats(user_id).await
    }

    /// Get active room count.
    #[allow(dead_code)]
    pub async fn room_count(&self) -> usize {
        self.rooms.read().await.len()
    }
}
