//! Selective Forwarding Unit Implementation
//!
//! Manages voice rooms and WebRTC peer connections for real-time audio and video.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;
use vc_common::protocol::PcType;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice::network_type::NetworkType;
use webrtc::ice::udp_network::{EphemeralUDP, UDPNetwork};
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
use super::screen_share::{ScreenShareInfo, ScreenShareLimiter};
use super::track::{spawn_rtp_forwarder, spawn_subscriber_remb_reader, TrackRouter};
use super::track_types::{Layer, TrackSource};
use super::webcam::WebcamInfo;
use crate::config::{Config, IceIpFamily};
use crate::ratelimit::{RateLimitCategory, RateLimiter};
use crate::ws::handlers::{send_outbound, send_outbound_within, OutboundSendError};
use crate::ws::{OutboundMsg, ServerEvent};

/// How long a send to *another* peer's socket may wait for room in its queue.
/// Voice fan-out runs inside the sending user's socket loop, so a stalled peer
/// must not hold it up; the stalled peer's own socket loop tears it down.
const PEER_SIGNAL_SEND_TIMEOUT: Duration = Duration::from_millis(250);

/// Send `event` to every peer concurrently, waiting at most
/// [`PEER_SIGNAL_SEND_TIMEOUT`] per peer. Peers whose queue is closed or stays
/// full are skipped (logged), so one stalled peer never delays the others.
async fn fan_out(senders: Vec<(Uuid, mpsc::Sender<OutboundMsg>)>, event: &ServerEvent) {
    let sends = senders.iter().map(|(user_id, tx)| async move {
        if let Err(e) = send_outbound_within(
            tx,
            OutboundMsg::Event(event.clone()),
            PEER_SIGNAL_SEND_TIMEOUT,
        )
        .await
        {
            warn!(user_id = %user_id, error = %e, "Skipping voice event for peer");
        }
    });
    futures::future::join_all(sends).await;
}

/// How long a peer connection may stay `Failed` before the peer is removed
/// from its room. Gives ICE restarts and brief network switches a chance.
pub const PEER_FAILED_GRACE: Duration = Duration::from_secs(15);

/// A peer whose connection failed and did not recover within
/// [`PEER_FAILED_GRACE`]. Handled by the reaper from
/// [`SfuServer::spawn_stale_peer_reaper`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StalePeer {
    pub user_id: Uuid,
    pub channel_id: Uuid,
    /// Only this session is removed; a newer session of the same user stays.
    pub session_id: Uuid,
}

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
        self.remove_peer_matching(user_id, None).await
    }

    /// Remove a peer only if it is still the given session. Check and removal
    /// happen under one lock, so a newer session of the same user is never hit.
    pub async fn remove_peer_if_session(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Option<Arc<Peer>> {
        self.remove_peer_matching(user_id, Some(session_id)).await
    }

    async fn remove_peer_matching(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
    ) -> Option<Arc<Peer>> {
        let mut peers = self.peers.write().await;
        if let Some(expected) = session_id {
            if peers.get(&user_id).map(|p| p.session_id) != Some(expected) {
                return None;
            }
        }
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

        // Send without holding the lock; never block on another peer's queue
        fan_out(senders, &event).await;
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

        // Send without holding the lock; never block on another peer's queue
        fan_out(senders, &event).await;
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
    /// Peers whose connection failed for longer than [`PEER_FAILED_GRACE`].
    stale_tx: mpsc::UnboundedSender<StalePeer>,
    /// Receiving end of `stale_tx`, taken once by the reaper.
    stale_rx: Mutex<Option<mpsc::UnboundedReceiver<StalePeer>>>,
}

/// Whether a `PUBLIC_IP` string is compatible with the allowed ICE family.
/// Unparseable values are kept (matches pre-existing behaviour).
fn public_ip_matches_family(ip: &str, family: IceIpFamily) -> bool {
    !matches!(
        (ip.parse::<std::net::IpAddr>(), family),
        (Ok(std::net::IpAddr::V4(_)), IceIpFamily::V6)
            | (Ok(std::net::IpAddr::V6(_)), IceIpFamily::V4)
    )
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
        // PUBLIC_IP NAT 1:1 only applies when its family is allowed by
        // VOICE_ICE_IP_FAMILY; otherwise the family filter wins (logged).
        let public_ip_usable = config.public_ip.as_deref().filter(|ip| {
            let ok = public_ip_matches_family(ip, config.voice_ice_ip_family);
            if !ok {
                warn!(public_ip = %ip, family = ?config.voice_ice_ip_family,
                    "PUBLIC_IP ignored: address family excluded by VOICE_ICE_IP_FAMILY");
            }
            ok
        });
        if let Some(public_ip) = public_ip_usable {
            setting_engine.set_nat_1to1_ips(
                vec![public_ip.to_owned()],
                webrtc::ice_transport::ice_candidate_type::RTCIceCandidateType::Host,
            );
            setting_engine.set_network_types(vec![NetworkType::Udp4, NetworkType::Udp6]);
            info!(public_ip = %public_ip, "SFU NAT 1:1 IP configured");
        }

        // Restrict ICE to the configured IP family / interfaces.
        match config.voice_ice_ip_family {
            IceIpFamily::V4 => setting_engine.set_network_types(vec![NetworkType::Udp4]),
            IceIpFamily::V6 => setting_engine.set_network_types(vec![NetworkType::Udp6]),
            IceIpFamily::Both => {}
        }
        if let Some(allowed) = config.voice_ice_interfaces.clone() {
            info!(interfaces = ?allowed, "SFU ICE interface filter configured");
            setting_engine
                .set_interface_filter(Box::new(move |name| allowed.iter().any(|a| a == name)));
        }

        // Pin ICE/media to the configured UDP range (firewall-friendly).
        if let (Some(min), Some(max)) = (config.voice_udp_port_min, config.voice_udp_port_max) {
            let ephemeral = EphemeralUDP::new(min, max)
                .map_err(|e| VoiceError::WebRtc(format!("invalid UDP port range: {e}")))?;
            setting_engine.set_udp_network(UDPNetwork::Ephemeral(ephemeral));
            info!(min, max, "SFU UDP port range configured");
        }

        // Build WebRTC API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();

        info!("SFU server initialized");

        let (stale_tx, stale_rx) = mpsc::unbounded_channel();

        Ok(Self {
            rooms: Arc::new(RwLock::new(HashMap::new())),
            api: Arc::new(api),
            config,
            rate_limiter: rate_limiter.map(Arc::new),
            stats_limiter: Arc::new(VoiceStatsLimiter::default()),
            voice_rate_limiter: Arc::new(VoiceRateLimiter::new()),
            stale_tx,
            stale_rx: Mutex::new(Some(stale_rx)),
        })
    }

    /// Sender for reporting stale peers to the reaper.
    #[must_use]
    pub fn stale_peer_sender(&self) -> mpsc::UnboundedSender<StalePeer> {
        self.stale_tx.clone()
    }

    /// Start the task that removes peers reported as stale (see
    /// [`PEER_FAILED_GRACE`]). Call once after startup; later calls only log.
    /// The task holds a weak reference, so it ends when the SFU is dropped.
    pub fn spawn_stale_peer_reaper(
        self: &Arc<Self>,
        pool: PgPool,
        limiter: Option<ScreenShareLimiter>,
    ) -> JoinHandle<()> {
        let rx = self
            .stale_rx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let sfu = Arc::downgrade(self);
        tokio::spawn(async move {
            let Some(mut rx) = rx else {
                warn!("Stale voice peer reaper already running");
                return;
            };
            while let Some(stale) = rx.recv().await {
                let Some(sfu) = sfu.upgrade() else { break };
                match super::leave::leave_voice_internal(
                    &sfu,
                    &pool,
                    stale.user_id,
                    stale.channel_id,
                    limiter.as_ref(),
                    Some(stale.session_id),
                )
                .await
                {
                    Ok(true) => info!(
                        user_id = %stale.user_id,
                        channel_id = %stale.channel_id,
                        "Removed voice peer after failed connection"
                    ),
                    Ok(false) => debug!(
                        user_id = %stale.user_id,
                        session_id = %stale.session_id,
                        "Stale voice peer already gone or replaced"
                    ),
                    Err(e) => warn!(
                        user_id = %stale.user_id,
                        channel_id = %stale.channel_id,
                        error = %e,
                        "Failed to remove stale voice peer"
                    ),
                }
            }
        })
    }

    /// Find the voice peer of `user_id` whose signaling queue is `tx`, i.e. the
    /// peer that belongs to that WebSocket. Returns `(channel_id, session_id)`.
    pub async fn find_peer_owned_by(
        &self,
        user_id: Uuid,
        tx: &mpsc::Sender<OutboundMsg>,
    ) -> Option<(Uuid, Uuid)> {
        let rooms: Vec<Arc<Room>> = self.rooms.read().await.values().cloned().collect();
        for room in rooms {
            if let Some(peer) = room.get_peer(user_id).await {
                if peer.signal_tx.same_channel(tx) {
                    return Some((room.channel_id, peer.session_id));
                }
            }
        }
        None
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

        // Add TURN server if credentials are actually derivable — webrtc-rs
        // rejects a turn: URL with empty username/credential ("turn server
        // credentials required"), which broke every voice join on servers
        // configured with TURN_SHARED_SECRET only (the beta setup).
        // webrtc-rs 0.17 removed `credential_type` from RTCIceServer; password is
        // the only credential type now per the W3C RTCIceServer dictionary.
        if let Some(turn) = &self.config.turn_server {
            if let Some(secret) = &self.config.turn_shared_secret {
                let (username, credential) =
                    super::turn_rest_credentials(secret, self.config.turn_credential_ttl, "sfu");
                ice_servers.push(RTCIceServer {
                    urls: vec![turn.clone()],
                    username,
                    credential,
                });
            } else if let (Some(username), Some(credential)) = (
                self.config.turn_username.clone(),
                self.config.turn_credential.clone(),
            ) {
                ice_servers.push(RTCIceServer {
                    urls: vec![turn.clone()],
                    username,
                    credential,
                });
            }
            // No usable credentials: skip TURN rather than hand webrtc-rs an
            // invalid server. The SFU runs on a public address and does not
            // depend on a relay itself.
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
        Self::register_pc_state_handler(&publisher_pc, &peer, self.stale_tx.clone(), "publisher");
        Self::register_pc_state_handler(&subscriber_pc, &peer, self.stale_tx.clone(), "subscriber");

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
    ///
    /// `Failed` starts a [`PEER_FAILED_GRACE`] timer; if the connection is
    /// still `Failed` afterwards, the peer is reported to the stale-peer
    /// reaper. `Disconnected` is only logged (it often recovers by itself).
    /// The callback holds only weak references to the peer and the PC, so it
    /// does not keep either alive.
    fn register_pc_state_handler(
        pc: &Arc<RTCPeerConnection>,
        peer: &Arc<Peer>,
        stale_tx: mpsc::UnboundedSender<StalePeer>,
        pc_label: &'static str,
    ) {
        let peer_weak = Arc::downgrade(peer);
        let pc_weak = Arc::downgrade(pc);
        let user_id = peer.user_id;
        let channel_id = peer.channel_id;
        let session_id = peer.session_id;
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let pw = peer_weak.clone();
            let pcw = pc_weak.clone();
            let stale_tx = stale_tx.clone();
            Box::pin(async move {
                debug!(
                    user_id = %user_id,
                    channel_id = %channel_id,
                    pc = pc_label,
                    state = ?state,
                    "Peer connection state changed"
                );

                if pw.upgrade().is_none() {
                    return;
                }
                match state {
                    RTCPeerConnectionState::Failed => {
                        warn!(user_id = %user_id, pc = pc_label, "Peer connection failed");
                        let stale = StalePeer {
                            user_id,
                            channel_id,
                            session_id,
                        };
                        tokio::spawn(report_if_still_failed(pw, pcw, stale_tx, stale, pc_label));
                    }
                    RTCPeerConnectionState::Disconnected => {
                        warn!(user_id = %user_id, pc = pc_label, "Peer connection disconnected");
                    }
                    _ => {}
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

    /// Trigger renegotiation on the subscriber PC of *another* peer by creating
    /// a new offer and sending it to that peer. Used after dynamically
    /// adding/removing tracks. Runs from the sender's socket loop, so the wait
    /// is bounded by the short per-peer [`PEER_SIGNAL_SEND_TIMEOUT`].
    pub async fn renegotiate(peer: &Arc<Peer>) -> Result<(), VoiceError> {
        let event = Self::create_subscriber_offer(peer).await?;
        let sent = send_outbound_within(&peer.signal_tx, event, PEER_SIGNAL_SEND_TIMEOUT).await;
        Self::check_offer_sent(peer, sent)
    }

    /// [`Self::renegotiate`] for the peer of the socket that is handling the
    /// current event (e.g. the joiner's first subscriber offer). Uses the
    /// socket's own full slow-client bound, so a briefly busy queue does not
    /// drop the offer.
    pub async fn renegotiate_own(peer: &Arc<Peer>) -> Result<(), VoiceError> {
        let event = Self::create_subscriber_offer(peer).await?;
        let sent = send_outbound(&peer.signal_tx, event).await;
        Self::check_offer_sent(peer, sent)
    }

    async fn create_subscriber_offer(peer: &Arc<Peer>) -> Result<OutboundMsg, VoiceError> {
        let offer = peer.subscriber_pc.create_offer(None).await?;
        peer.subscriber_pc
            .set_local_description(offer.clone())
            .await?;
        Ok(OutboundMsg::Event(ServerEvent::VoiceSubscriberOffer {
            channel_id: peer.channel_id,
            sdp: offer.sdp,
        }))
    }

    fn check_offer_sent(
        peer: &Arc<Peer>,
        sent: Result<(), OutboundSendError>,
    ) -> Result<(), VoiceError> {
        sent.map_err(|e| {
            tracing::error!(
                user_id = %peer.user_id,
                channel_id = %peer.channel_id,
                error = %e,
                "failed to send subscriber offer — client will not receive tracks"
            );
            VoiceError::Outbound(e)
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

/// After [`PEER_FAILED_GRACE`], report the peer as stale if it still exists
/// and this PC is still `Failed`.
async fn report_if_still_failed(
    peer: Weak<Peer>,
    pc: Weak<RTCPeerConnection>,
    stale_tx: mpsc::UnboundedSender<StalePeer>,
    stale: StalePeer,
    pc_label: &'static str,
) {
    tokio::time::sleep(PEER_FAILED_GRACE).await;
    if peer.upgrade().is_none() {
        return;
    }
    let still_failed = pc
        .upgrade()
        .is_some_and(|pc| pc.connection_state() == RTCPeerConnectionState::Failed);
    if !still_failed {
        return;
    }
    warn!(
        user_id = %stale.user_id,
        channel_id = %stale.channel_id,
        pc = pc_label,
        "Peer connection did not recover, removing peer"
    );
    if stale_tx.send(stale).is_err() {
        warn!(user_id = %stale.user_id, "Stale voice peer reaper is not running");
    }
}

#[cfg(test)]
mod fan_out_tests {
    use super::*;

    fn joined_event(user_id: Uuid) -> ServerEvent {
        ServerEvent::VoiceUserLeft {
            channel_id: Uuid::nil(),
            user_id,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fan_out_skips_stalled_peer_without_blocking_others() {
        let stalled_id = Uuid::from_u128(1);
        let (stalled_tx, mut stalled_rx) = mpsc::channel(1);
        stalled_tx
            .try_send(OutboundMsg::Ping)
            .expect("fill stalled queue"); // never drained during the call
        let (healthy_a_tx, mut healthy_a_rx) = mpsc::channel(8);
        let (healthy_b_tx, mut healthy_b_rx) = mpsc::channel(8);

        let started = tokio::time::Instant::now();
        tokio::time::timeout(
            PEER_SIGNAL_SEND_TIMEOUT * 2,
            fan_out(
                vec![
                    (Uuid::from_u128(2), healthy_a_tx),
                    (stalled_id, stalled_tx),
                    (Uuid::from_u128(3), healthy_b_tx),
                ],
                &joined_event(stalled_id),
            ),
        )
        .await
        .expect("fan_out must not block on a stalled peer");
        assert!(started.elapsed() <= PEER_SIGNAL_SEND_TIMEOUT);

        for rx in [&mut healthy_a_rx, &mut healthy_b_rx] {
            assert!(matches!(
                rx.try_recv(),
                Ok(OutboundMsg::Event(ServerEvent::VoiceUserLeft { .. }))
            ));
        }
        // The stalled peer only holds what it had before; its event was skipped.
        assert!(matches!(stalled_rx.try_recv(), Ok(OutboundMsg::Ping)));
        assert!(stalled_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn fan_out_skips_closed_peer() {
        let (closed_tx, closed_rx) = mpsc::channel(1);
        drop(closed_rx);
        let (healthy_tx, mut healthy_rx) = mpsc::channel(8);
        fan_out(
            vec![
                (Uuid::from_u128(1), closed_tx),
                (Uuid::from_u128(2), healthy_tx),
            ],
            &joined_event(Uuid::from_u128(9)),
        )
        .await;
        assert!(healthy_rx.try_recv().is_ok());
    }
}

#[cfg(test)]
mod renegotiate_tests {
    use super::*;

    /// The joiner's own subscriber offer must survive a briefly busy queue:
    /// it gets the full slow-client bound, not the short fan-out bound.
    #[tokio::test]
    async fn own_offer_waits_past_fan_out_timeout() {
        let sfu = SfuServer::new(Arc::new(Config::default_for_test()), None).expect("sfu");
        let (tx, mut rx) = mpsc::channel(1);
        let peer = sfu
            .create_peer(
                Uuid::from_u128(1),
                "joiner".into(),
                "Joiner".into(),
                Uuid::from_u128(2),
                tx.clone(),
            )
            .await
            .expect("peer");
        tx.try_send(OutboundMsg::Ping).expect("fill queue");

        // Drain the queue only after the fan-out bound has passed.
        let drain = tokio::spawn(async move {
            tokio::time::sleep(PEER_SIGNAL_SEND_TIMEOUT * 2).await;
            assert!(matches!(rx.recv().await, Some(OutboundMsg::Ping)));
            rx.recv().await
        });

        SfuServer::renegotiate_own(&peer)
            .await
            .expect("own offer must not be dropped by the fan-out bound");
        assert!(matches!(
            drain.await.expect("drain task"),
            Some(OutboundMsg::Event(ServerEvent::VoiceSubscriberOffer { .. }))
        ));
        peer.close().await;
    }
}

#[cfg(test)]
mod udp_range_tests {
    use super::*;

    #[tokio::test]
    async fn sfu_candidates_use_configured_port_range() {
        let mut config = Config::default_for_test();
        config.voice_udp_port_min = Some(50000);
        config.voice_udp_port_max = Some(50010);
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");

        let pc = sfu
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("pc");
        pc.create_data_channel("probe", None).await.expect("dc");
        let offer = pc.create_offer(None).await.expect("offer");
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.expect("local desc");
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;

        let sdp = pc.local_description().await.expect("local description").sdp;
        let ports: Vec<u16> = sdp
            .lines()
            .filter_map(|l| l.strip_prefix("a=candidate:"))
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                (f.get(2).is_some_and(|p| p.eq_ignore_ascii_case("udp")))
                    .then(|| f.get(5)?.parse().ok())
                    .flatten()
            })
            .collect();
        let _ = pc.close().await;
        assert!(!ports.is_empty(), "no UDP host candidates gathered:\n{sdp}");
        for p in ports {
            assert!(
                (50000..=50010).contains(&p),
                "candidate port {p} outside range"
            );
        }
    }
}

#[cfg(test)]
mod ice_filter_tests {
    use super::*;

    /// Gather host candidate IPs for an SFU built from `config`.
    async fn gather_ips(config: Config) -> Vec<std::net::IpAddr> {
        let sfu = SfuServer::new(Arc::new(config), None).expect("sfu");
        let pc = sfu
            .api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .expect("pc");
        pc.create_data_channel("probe", None).await.expect("dc");
        let offer = pc.create_offer(None).await.expect("offer");
        let mut gathered = pc.gathering_complete_promise().await;
        pc.set_local_description(offer).await.expect("local desc");
        let _ = tokio::time::timeout(Duration::from_secs(10), gathered.recv()).await;
        let sdp = pc.local_description().await.expect("desc").sdp;
        let _ = pc.close().await;
        sdp.lines()
            .filter_map(|l| l.strip_prefix("a=candidate:"))
            .filter_map(|l| l.split_whitespace().nth(4)?.parse().ok())
            .collect()
    }

    #[tokio::test]
    async fn sfu_candidates_respect_ip_family() {
        let mut config = Config::default_for_test();
        config.voice_ice_ip_family = IceIpFamily::V4;
        let ips = gather_ips(config).await;
        assert!(ips.iter().all(std::net::IpAddr::is_ipv4), "{ips:?}");

        // Opposite family: nothing of the excluded family may appear (on a
        // host without global IPv6 this also yields no candidates at all).
        let mut config = Config::default_for_test();
        config.voice_ice_ip_family = IceIpFamily::V6;
        let ips = gather_ips(config).await;
        assert!(ips.iter().all(std::net::IpAddr::is_ipv6), "{ips:?}");
    }

    /// Approach: loopback is suppressed by webrtc-ice, so pick a real
    /// interface from the host (the first one that has a non-loopback IPv4
    /// address) and assert all candidate IPs belong to it; a nonexistent
    /// name must yield no host candidates.
    #[tokio::test]
    async fn sfu_candidates_respect_interface_filter() {
        let ifaces = webrtc::util::vnet::net::Net::new(None)
            .get_interfaces()
            .await;
        let Some(chosen) = ifaces.iter().find(|i| {
            i.addrs()
                .iter()
                .any(|a| a.addr().is_ipv4() && !a.addr().is_loopback())
        }) else {
            eprintln!("SKIP: host has no non-loopback IPv4 interface");
            return;
        };
        let own: Vec<std::net::IpAddr> = chosen.addrs().iter().map(|a| a.addr()).collect();

        let mut config = Config::default_for_test();
        config.voice_ice_interfaces = Some(vec![chosen.name().to_owned()]);
        let ips = gather_ips(config).await;
        assert!(!ips.is_empty(), "expected candidates on {}", chosen.name());
        assert!(
            ips.iter().all(|ip| own.contains(ip)),
            "{ips:?} not in {own:?}"
        );

        let mut config = Config::default_for_test();
        config.voice_ice_interfaces = Some(vec!["no-such-if0".to_owned()]);
        let ips = gather_ips(config).await;
        assert!(ips.is_empty(), "{ips:?}");
    }

    #[test]
    fn public_ip_family_compatibility() {
        assert!(public_ip_matches_family("203.0.113.5", IceIpFamily::V4));
        assert!(!public_ip_matches_family("203.0.113.5", IceIpFamily::V6));
        assert!(!public_ip_matches_family("2001:db8::1", IceIpFamily::V4));
        assert!(public_ip_matches_family("2001:db8::1", IceIpFamily::Both));
    }
}
