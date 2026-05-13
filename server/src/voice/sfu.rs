//! Selective Forwarding Unit Implementation
//!
//! Manages voice rooms and WebRTC peer connections for real-time audio and video.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;
use vc_common::protocol::PcType;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice::network_type::NetworkType;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::rtp_transceiver::RTCPFeedback;

use super::error::VoiceError;
use super::peer::Peer;
use super::rate_limit::{VoiceRateLimiter, VoiceStatsLimiter};
use super::screen_share::ScreenShareInfo;
use super::track::{spawn_rtp_forwarder, spawn_subscriber_remb_reader, TrackRouter};
use super::track_types::{Layer, TrackSource};
use super::webcam::WebcamInfo;
use crate::config::Config;
use crate::ratelimit::{RateLimitCategory, RateLimiter};
use crate::ws::{OutboundMsg, ServerEvent};

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

        // If the user already has a peer (stale session from a previous connection),
        // remove it and replace with the new one instead of rejecting
        if let Some(old_peer) = peers.remove(&peer.user_id) {
            tracing::info!(user_id = %peer.user_id, "Replacing stale voice peer");
            // Close old peer connections
            old_peer.close().await;
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

    /// Add a screen share session (keyed by `stream_id`).
    pub async fn add_screen_share(&self, info: ScreenShareInfo) {
        let mut shares = self.screen_shares.write().await;
        shares.insert(info.stream_id, info);
    }

    /// Remove a single screen share session by `stream_id`.
    pub async fn remove_screen_share(&self, stream_id: Uuid) -> Option<ScreenShareInfo> {
        let mut shares = self.screen_shares.write().await;
        shares.remove(&stream_id)
    }

    /// Remove all screen share sessions belonging to a user.
    /// Returns the removed entries so callers can broadcast stop events.
    pub async fn remove_user_screen_shares(&self, user_id: Uuid) -> Vec<ScreenShareInfo> {
        let mut shares = self.screen_shares.write().await;
        let stream_ids: Vec<Uuid> = shares
            .values()
            .filter(|s| s.user_id == user_id)
            .map(|s| s.stream_id)
            .collect();
        stream_ids
            .iter()
            .filter_map(|id| shares.remove(id))
            .collect()
    }

    /// Count how many active screen share streams a user has.
    pub async fn get_user_stream_count(&self, user_id: Uuid) -> usize {
        let shares = self.screen_shares.read().await;
        shares.values().filter(|s| s.user_id == user_id).count()
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
                muted: peer.is_self_muted().await,
                screen_sharing: shares.values().any(|s| s.user_id == *user_id),
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
        let senders: Vec<(Uuid, mpsc::Sender<OutboundMsg>)> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .filter(|(id, _)| **id != exclude_user_id)
                .map(|(id, peer)| (*id, peer.signal_tx.clone()))
                .collect()
        };

        // Send without holding the lock
        for (user_id, tx) in senders {
            if let Err(e) = tx.send(OutboundMsg::Event(event.clone())).await {
                warn!(user_id = %user_id, error = %e, "Failed to send event to peer");
            }
        }
    }

    /// Broadcast an event to all peers.
    ///
    /// Clones the peer list before sending to avoid holding the lock during I/O.
    pub async fn broadcast_all(&self, event: ServerEvent) {
        // Clone sender handles to release lock before I/O
        let senders: Vec<(Uuid, mpsc::Sender<OutboundMsg>)> = {
            let peers = self.peers.read().await;
            peers
                .iter()
                .map(|(id, peer)| (*id, peer.signal_tx.clone()))
                .collect()
        };

        // Send without holding the lock
        for (user_id, tx) in senders {
            if let Err(e) = tx.send(OutboundMsg::Event(event.clone())).await {
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
    /// Per-peer, per-event-class token bucket limiter for voice signaling events.
    voice_rate_limiter: Arc<VoiceRateLimiter>,
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

        // Register VP8 video codec (only video codec — ensures consistent PT across sessions)
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

        // Create interceptor registry
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| VoiceError::WebRtc(e.to_string()))?;

        // Configure SettingEngine for NAT traversal behind Docker/VPS
        let mut setting_engine = SettingEngine::default();
        if let Some(ref public_ip) = config.public_ip {
            setting_engine.set_nat_1to1_ips(
                vec![public_ip.clone()],
                webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType::Host,
            );
            setting_engine.set_network_types(vec![NetworkType::Udp4, NetworkType::Udp6]);
            info!(public_ip = %public_ip, "SFU NAT 1:1 IP configured");
        }

        // Build WebRTC API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();

        info!("SFU server initialized");

        Ok(Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            api: Arc::new(api),
            config,
            rate_limiter: rate_limiter.map(Arc::new),
            stats_limiter: Arc::new(VoiceStatsLimiter::default()),
            voice_rate_limiter: Arc::new(VoiceRateLimiter::new()),
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

        // Add TURN server if configured.
        // webrtc-rs 0.17 removed `credential_type` from RTCIceServer; password is
        // the only credential type now per the W3C RTCIceServer dictionary.
        if let Some(turn) = &self.config.turn_server {
            ice_servers.push(RTCIceServer {
                urls: vec![turn.clone()],
                username: self.config.turn_username.clone().unwrap_or_default(),
                credential: self.config.turn_credential.clone().unwrap_or_default(),
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

    /// Create a new peer with two `PeerConnection`s (publisher + subscriber).
    pub async fn create_peer(
        &self,
        user_id: Uuid,
        username: String,
        display_name: String,
        channel_id: Uuid,
        signal_tx: mpsc::Sender<OutboundMsg>,
    ) -> Result<Arc<Peer>, VoiceError> {
        let config = self.rtc_config();

        // Create two PeerConnections: one for publishing, one for subscribing
        let publisher_pc = Arc::new(
            self.api
                .new_peer_connection(config.clone())
                .await
                .map_err(|e| VoiceError::WebRtc(e.to_string()))?,
        );
        let subscriber_pc = Arc::new(
            self.api
                .new_peer_connection(config)
                .await
                .map_err(|e| VoiceError::WebRtc(e.to_string()))?,
        );

        let peer = Arc::new(Peer::new(
            user_id,
            username,
            display_name,
            channel_id,
            publisher_pc.clone(),
            subscriber_pc.clone(),
            signal_tx,
        ));

        // Set up connection state handlers for both PCs
        Self::register_pc_state_handler(&publisher_pc, &peer, user_id, channel_id, "publisher");
        Self::register_pc_state_handler(&subscriber_pc, &peer, user_id, channel_id, "subscriber");

        Ok(peer)
    }

    /// Set up track handling for a peer.
    pub fn setup_track_handler(&self, peer: &Arc<Peer>, room: &Arc<Room>) {
        let peer_weak = Arc::downgrade(peer);
        let room_weak = Arc::downgrade(room);
        let user_id = peer.user_id;
        let channel_id = peer.channel_id;

        peer.publisher_pc
            .on_track(Box::new(move |track, _receiver, _transceiver| {
                let pw = peer_weak.clone();
                let rw = room_weak.clone();
                let uid = user_id;
                let cid = channel_id;

                Box::pin(async move {
                    // Parse simulcast RID — empty string means non-simulcast.
                    let rid = track.rid().to_string();
                    let layer = Layer::from_rid(&rid).unwrap_or(Layer::High);

                    info!(
                        user_id = %uid,
                        channel_id = %cid,
                        track_id = %track.id(),
                        kind = ?track.kind(),
                        rid = %rid,
                        layer = ?layer,
                        "Received track from peer"
                    );

                    // Upgrade weak references once — use for both source resolution and track setup
                    let (peer, room) = match (pw.upgrade(), rw.upgrade()) {
                        (Some(p), Some(r)) => (p, r),
                        _ => return,
                    };

                    // Determine source type: check pending queue first, fall back to defaults.
                    //
                    // For simulcast video: the browser sends 3 on_track calls (one per
                    // RID layer) for the same logical source. We only pop from the
                    // pending queue for the primary layer (High) or non-simulcast
                    // tracks. Secondary layers (Medium, Low) look up the source type
                    // from the already-registered High-layer entry in simulcast_tracks.
                    let is_secondary_simulcast = !rid.is_empty()
                        && layer != Layer::High
                        && track.kind() == RTPCodecType::Video;

                    let source_type = if is_secondary_simulcast {
                        // Find the source type from the High layer already stored.
                        if let Some(st) = room
                            .track_router
                            .find_source_type_for_user(uid, Layer::High)
                        {
                            st
                        } else {
                            // High layer hasn't arrived yet — stash and skip.
                            // When High arrives, store_simulcast_track will drain this.
                            room.track_router
                                .stash_pending_secondary(uid, layer, track.clone());
                            return;
                        }
                    } else {
                        match track.kind() {
                            RTPCodecType::Audio => peer
                                .pop_pending_audio_source()
                                .await
                                .unwrap_or(TrackSource::Microphone),
                            RTPCodecType::Video => peer
                                .pop_pending_video_source()
                                .await
                                // Fallback: if no pending source was queued, assume
                                // it is a screen-video track with a nil stream_id.
                                // This keeps backwards compatibility for the initial
                                // mic+screen two-track setup.
                                .unwrap_or(TrackSource::ScreenVideo(Uuid::nil())),
                            RTPCodecType::Unspecified => {
                                warn!("Unspecified track kind: {:?}", track.kind());
                                return;
                            }
                        }
                    };

                    // For video tracks with a valid RID, store in simulcast_tracks.
                    let is_simulcast = !rid.is_empty() && source_type.is_video();
                    if is_simulcast {
                        room.track_router.store_simulcast_track(
                            uid,
                            source_type,
                            layer,
                            track.clone(),
                        );
                        debug!(
                            source = %uid,
                            source_type = ?source_type,
                            layer = ?layer,
                            "Stored simulcast track"
                        );
                    }

                    // Start RTP forwarder for every layer (each sends packets
                    // tagged with its layer so forward_rtp can filter by active_layer).
                    spawn_rtp_forwarder(
                        uid,
                        source_type,
                        layer,
                        track.clone(),
                        room.track_router.clone(),
                        peer.clone(),
                    );

                    // For video tracks: send PLI every 3 seconds so subscribers
                    // always get a fresh keyframe. This is the standard Pion SFU
                    // pattern — webrtc-rs doesn't include an interval PLI interceptor
                    // by default, and PeerConnection::write_rtcp() doesn't reliably
                    // deliver PLI to the remote browser.
                    if source_type.is_video() && layer == Layer::High {
                        let publisher_pc_weak = Arc::downgrade(&peer.publisher_pc);
                        let track_ssrc = track.ssrc();
                        tokio::spawn(async move {
                            use webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
                            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3));
                            loop {
                                interval.tick().await;
                                let Some(publisher_pc) = publisher_pc_weak.upgrade() else {
                                    break; // PC dropped — stop PLI
                                };
                                let pli = PictureLossIndication {
                                    sender_ssrc: 0,
                                    media_ssrc: track_ssrc,
                                };
                                if publisher_pc.write_rtcp(&[Box::new(pli)]).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }

                    // Only create subscriber tracks for the primary layer (High)
                    // or non-simulcast tracks. Secondary simulcast layers (Medium,
                    // Low) share the same subscriber local track — they are just
                    // forwarded when the subscriber's active_layer matches.
                    if is_simulcast && layer != Layer::High {
                        return;
                    }

                    // Store incoming track (only for the primary / non-simulcast track)
                    peer.set_incoming_track(source_type, track.clone()).await;

                    // Create subscriber tracks for all existing peers
                    let other_peers = room.get_other_peers(uid).await;
                    info!(
                        source = %uid,
                        source_type = ?source_type,
                        other_peer_count = other_peers.len(),
                        "Forwarding track to subscribers"
                    );
                    for other_peer in other_peers {
                        if let Ok(local_track) = room
                            .track_router
                            .create_subscriber_track(uid, source_type, &other_peer, &track)
                            .await
                        {
                            match other_peer
                                .add_outgoing_track(uid, source_type, local_track)
                                .await
                            {
                                Ok(sender) => {
                                    if source_type.is_video() {
                                        spawn_subscriber_remb_reader(
                                            room.track_router.clone(),
                                            other_peer.user_id,
                                            uid,
                                            source_type,
                                            sender,
                                            other_peer.signal_tx.clone(),
                                            room.channel_id,
                                        );
                                    }
                                    // Renegotiate so subscriber receives updated SDP
                                    if let Err(e) = Self::renegotiate(&other_peer).await {
                                        warn!(
                                            subscriber = %other_peer.user_id,
                                            error = %e,
                                            "Renegotiation failed after track add"
                                        );
                                    }
                                }
                                Err(e) => {
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
            }));
    }

    /// Set up ICE candidate handlers for both publisher and subscriber PCs.
    pub fn setup_ice_handler(&self, peer: &Arc<Peer>) {
        Self::register_ice_callback(
            &peer.publisher_pc,
            peer.signal_tx.clone(),
            peer.channel_id,
            "publisher",
        );
        Self::register_ice_callback(
            &peer.subscriber_pc,
            peer.signal_tx.clone(),
            peer.channel_id,
            "subscriber",
        );
    }

    /// Register an ICE candidate callback on a single `PeerConnection`.
    fn register_ice_callback(
        pc: &Arc<RTCPeerConnection>,
        signal_tx: mpsc::Sender<OutboundMsg>,
        channel_id: Uuid,
        pc_label: &'static str,
    ) {
        pc.on_ice_candidate(Box::new(move |candidate| {
            let tx = signal_tx.clone();
            Box::pin(async move {
                let Some(c) = candidate else { return };
                let json = match c.to_json() {
                    Ok(j) => j,
                    Err(e) => {
                        warn!(pc = pc_label, error = %e, "Failed to convert ICE candidate to JSON");
                        return;
                    }
                };
                let candidate_str = match serde_json::to_string(&json) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(pc = pc_label, error = %e, "Failed to serialize ICE candidate");
                        return;
                    }
                };
                if let Err(e) = tx
                    .send(OutboundMsg::Event(ServerEvent::VoiceIceCandidate {
                        channel_id,
                        candidate: candidate_str,
                        pc_type: if pc_label == "subscriber" { PcType::Subscriber } else { PcType::Publisher },
                    }))
                    .await
                {
                    tracing::error!(
                        channel_id = %channel_id,
                        pc = pc_label,
                        error = %e,
                        "Failed to send ICE candidate - connection may fail"
                    );
                }
            })
        }));
    }

    /// Register a connection state change handler on a single `PeerConnection`.
    fn register_pc_state_handler(
        pc: &Arc<RTCPeerConnection>,
        peer: &Arc<Peer>,
        user_id: Uuid,
        channel_id: Uuid,
        pc_label: &'static str,
    ) {
        let peer_weak = Arc::downgrade(peer);
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let pw = peer_weak.clone();
            Box::pin(async move {
                debug!(
                    user_id = %user_id,
                    channel_id = %channel_id,
                    pc = pc_label,
                    state = ?state,
                    "Peer connection state changed"
                );

                if matches!(
                    state,
                    RTCPeerConnectionState::Failed | RTCPeerConnectionState::Disconnected
                ) && pw.upgrade().is_some()
                {
                    warn!(user_id = %user_id, pc = pc_label, "Peer connection failed/disconnected");
                }
            })
        }));
    }

    /// Handle a publisher offer from the client -- server creates answer.
    pub async fn handle_publisher_offer(
        peer: &Arc<Peer>,
        sdp: String,
    ) -> Result<String, VoiceError> {
        let offer =
            RTCSessionDescription::offer(sdp).map_err(|e| VoiceError::Signaling(e.to_string()))?;
        peer.publisher_pc.set_remote_description(offer).await?;
        let answer = peer.publisher_pc.create_answer(None).await?;
        peer.publisher_pc
            .set_local_description(answer.clone())
            .await?;
        Ok(answer.sdp)
    }

    /// Handle a subscriber answer from the client.
    pub async fn handle_subscriber_answer(peer: &Arc<Peer>, sdp: String) -> Result<(), VoiceError> {
        let answer =
            RTCSessionDescription::answer(sdp).map_err(|e| VoiceError::Signaling(e.to_string()))?;
        peer.subscriber_pc.set_remote_description(answer).await?;
        Ok(())
    }

    /// Trigger renegotiation on the subscriber PC by creating a new offer
    /// and sending it to the peer. Used after dynamically adding/removing tracks.
    pub async fn renegotiate(peer: &Arc<Peer>) -> Result<(), VoiceError> {
        let offer = peer.subscriber_pc.create_offer(None).await?;
        peer.subscriber_pc
            .set_local_description(offer.clone())
            .await?;
        peer.signal_tx
            .send(OutboundMsg::Event(ServerEvent::VoiceSubscriberOffer {
                channel_id: peer.channel_id,
                sdp: offer.sdp,
            }))
            .await
            .map_err(|e| {
                tracing::error!(
                    user_id = %peer.user_id,
                    channel_id = %peer.channel_id,
                    error = %e,
                    "failed to send subscriber offer — client will not receive tracks"
                );
                VoiceError::Signaling("failed to send subscriber offer".into())
            })?;
        Ok(())
    }

    /// Handle an ICE candidate from a peer, routed by `pc_type`.
    pub async fn handle_ice_candidate(
        peer: &Arc<Peer>,
        candidate_str: &str,
        pc_type: &PcType,
    ) -> Result<(), VoiceError> {
        let candidate: webrtc::ice_transport::ice_candidate::RTCIceCandidateInit =
            serde_json::from_str(candidate_str)
                .map_err(|e| VoiceError::Signaling(format!("Invalid ICE candidate: {e}")))?;

        let pc = match pc_type {
            PcType::Subscriber => &peer.subscriber_pc,
            PcType::Publisher => &peer.publisher_pc,
        };
        pc.add_ice_candidate(candidate).await?;

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
                return Err(VoiceError::RateLimited("voice_join"));
            }
        }
        Ok(())
    }

    /// Check if a user can report voice stats (rate limit check).
    pub async fn check_stats_rate_limit(&self, user_id: Uuid) -> Result<(), VoiceError> {
        self.stats_limiter.check_stats(user_id).await
    }

    /// Access the per-peer voice signaling rate limiter.
    #[must_use]
    pub const fn voice_rate_limiter(&self) -> &Arc<VoiceRateLimiter> {
        &self.voice_rate_limiter
    }

    /// Get active room count.
    pub async fn room_count(&self) -> usize {
        self.rooms.read().await.len()
    }
}
