//! WebRTC Client
//!
//! Handles WebRTC peer connection for voice chat.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info, warn};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{APIBuilder, API};
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::rtp_transceiver::{RTCPFeedback, RTCRtpEncodingParameters, RTCRtpTransceiverInit};
use webrtc::stats::StatsReportType;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

use crate::video::rtp::VP8_PAYLOAD_TYPE;

/// Build 3-layer simulcast encoding parameters (high / medium / low).
///
/// **Not applied:** webrtc-rs (0.17) `add_transceiver_from_track` reads
/// only the direction from its init and ignores `send_encodings`, so no
/// RID appears in the SDP and the desktop app sends one plain (non-
/// simulcast) VP8 stream; the SFU handles that like a browser without
/// simulcast. Kept so the intent is in one place if real simulcast (three
/// separately encoded streams) is added later.
fn simulcast_encodings() -> Vec<RTCRtpEncodingParameters> {
    vec![
        RTCRtpEncodingParameters {
            rid: smol_str::SmolStr::new("h"),
            ..Default::default()
        },
        RTCRtpEncodingParameters {
            rid: smol_str::SmolStr::new("m"),
            ..Default::default()
        },
        RTCRtpEncodingParameters {
            rid: smol_str::SmolStr::new("l"),
            ..Default::default()
        },
    ]
}

/// What a published video track carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoKind {
    Screen,
    Webcam,
}

impl VideoKind {
    const fn track_prefix(self) -> &'static str {
        match self {
            Self::Screen => "screen-video",
            Self::Webcam => "webcam-video",
        }
    }

    const fn stream_prefix(self) -> &'static str {
        match self {
            Self::Screen => "screen-share-stream",
            Self::Webcam => "webcam-stream",
        }
    }

    /// Name used in events to the frontend.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Webcam => "webcam",
        }
    }
}

/// Publisher negotiation: never two offers out at once, and never stuck.
///
/// Every offer (the join's first one included) gets an id. A change while
/// an offer waits for its answer is sent as one new offer right after it.
/// An offer that gets no usable answer (deadline, a rejected or failing
/// answer, a failed send) is offered again once; if that fails too the
/// caller gives up visibly ([`OfferFailure::GiveUp`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PublisherNegotiation {
    outstanding: Option<Outstanding>,
    pending: bool,
}

/// Offer ids are unique for the whole process, so a deadline of an older
/// session can never match an offer of a newer one.
static NEXT_OFFER_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Outstanding {
    id: u64,
    retried: bool,
}

/// What to do after an offer failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferFailure {
    /// Not the outstanding offer (already answered or replaced): nothing.
    Stale,
    /// Offer again under this id (the only retry).
    Retry(u64),
    /// The retry failed too: the changes waiting for it must fail.
    GiveUp,
}

impl PublisherNegotiation {
    fn take_id() -> u64 {
        NEXT_OFFER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
    }

    /// A track was added or removed (or the join's first offer).
    /// `Some(id)`: create and send an offer with this id now.
    pub fn request(&mut self) -> Option<u64> {
        if self.outstanding.is_some() {
            self.pending = true;
            None
        } else {
            let id = Self::take_id();
            self.outstanding = Some(Outstanding { id, retried: false });
            Some(id)
        }
    }

    /// The answer to the outstanding offer was applied. `Some(id)`: send
    /// the queued change now under this id.
    pub fn answered(&mut self) -> Option<u64> {
        if self.pending {
            self.pending = false;
            let id = Self::take_id();
            self.outstanding = Some(Outstanding { id, retried: false });
            Some(id)
        } else {
            self.outstanding = None;
            None
        }
    }

    /// Offer `id` got no usable answer (or could not be sent).
    pub fn offer_failed(&mut self, id: u64) -> OfferFailure {
        match self.outstanding {
            Some(o) if o.id == id && !o.retried => {
                let id = Self::take_id();
                // A queued change is part of the new offer.
                self.pending = false;
                self.outstanding = Some(Outstanding { id, retried: true });
                OfferFailure::Retry(id)
            }
            Some(o) if o.id == id => {
                self.outstanding = None;
                self.pending = false;
                OfferFailure::GiveUp
            }
            _ => OfferFailure::Stale,
        }
    }

    /// The id of the offer that waits for its answer.
    pub fn outstanding_id(&self) -> Option<u64> {
        self.outstanding.map(|o| o.id)
    }

    /// Whether the outstanding offer is already the retry.
    pub fn is_retry(&self) -> bool {
        self.outstanding.is_some_and(|o| o.retried)
    }

    /// Whether an offer of ours is out.
    pub const fn is_awaiting_answer(&self) -> bool {
        self.outstanding.is_some()
    }
}

/// WebRTC errors
#[derive(Error, Debug)]
pub enum WebRtcError {
    #[error("WebRTC API error: {0}")]
    ApiError(String),
    #[error("Peer connection error: {0}")]
    PeerConnectionError(String),
    #[error("SDP error: {0}")]
    SdpError(String),
    #[error("ICE error: {0}")]
    IceError(String),
    #[error("Not connected")]
    NotConnected,
    #[error("Already connected")]
    AlreadyConnected,
    #[error("Track error: {0}")]
    TrackError(String),
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    /// Short ICE outage (`disconnected`): media keeps running and may come
    /// back by itself; the UI shows "reconnecting" and keeps everything.
    Reconnecting,
    Failed,
}

/// Map the publisher peer connection's state to the voice state the UI sees.
///
/// `Disconnected` is a short ICE outage, not the end of the call: it becomes
/// `Reconnecting` so the UI keeps its state (A8). Only `Closed` (own leave)
/// ends the call; `Failed` lets the UI run its rejoin flow.
pub const fn publisher_state(state: RTCPeerConnectionState) -> ConnectionState {
    match state {
        RTCPeerConnectionState::Connected => ConnectionState::Connected,
        RTCPeerConnectionState::Disconnected => ConnectionState::Reconnecting,
        RTCPeerConnectionState::Closed => ConnectionState::Disconnected,
        RTCPeerConnectionState::Failed => ConnectionState::Failed,
        _ => ConnectionState::Connecting,
    }
}

/// ICE server configuration
#[derive(Debug, Clone)]
pub struct IceServerConfig {
    pub urls: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<String>,
}

impl Default for IceServerConfig {
    fn default() -> Self {
        Self {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            username: None,
            credential: None,
        }
    }
}

/// Remote ICE candidates that arrived before the remote description.
///
/// The server may send a candidate before (or while) its answer/offer is
/// applied; `RTCPeerConnection::add_ice_candidate` rejects candidates until a
/// remote description exists, which silently lost them. Candidates are held
/// here and applied right after `set_remote_description`. The flag and the
/// queue live under one lock so a candidate can never be queued after the
/// flush already ran.
#[derive(Default)]
struct IceCandidateQueue {
    remote_description_set: bool,
    pending: Vec<RTCIceCandidateInit>,
}

impl IceCandidateQueue {
    /// Queue `candidate` if no remote description is set yet; otherwise hand
    /// it back for immediate use.
    fn queue_or_pass(&mut self, candidate: RTCIceCandidateInit) -> Option<RTCIceCandidateInit> {
        if self.remote_description_set {
            Some(candidate)
        } else {
            self.pending.push(candidate);
            None
        }
    }

    /// Mark the remote description as set and take the queued candidates.
    fn open(&mut self) -> Vec<RTCIceCandidateInit> {
        self.remote_description_set = true;
        std::mem::take(&mut self.pending)
    }
}

/// Apply candidates released by [`IceCandidateQueue::open`]. A bad candidate
/// is logged and skipped; it must not fail the description that was just set.
async fn apply_queued_candidates(
    pc: &RTCPeerConnection,
    candidates: Vec<RTCIceCandidateInit>,
    which: &str,
) {
    if candidates.is_empty() {
        return;
    }
    debug!(
        count = candidates.len(),
        pc = which,
        "Applying queued ICE candidates"
    );
    for candidate in candidates {
        if let Err(e) = pc.add_ice_candidate(candidate).await {
            warn!(pc = which, "Queued ICE candidate rejected: {e}");
        }
    }
}

/// WebRTC client for voice chat and screen sharing
pub struct WebRtcClient {
    api: Arc<API>,
    peer_connection: Arc<RwLock<Option<Arc<RTCPeerConnection>>>>,
    audio_sender: Arc<RwLock<Option<Arc<RTCRtpSender>>>>,
    local_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,
    state: Arc<RwLock<ConnectionState>>,
    channel_id: Arc<RwLock<Option<String>>>,

    // Video senders are added per publication (screen share, webcam) and
    // removed at its end: see `add_video_sender` / `remove_video_sender`.

    // Subscriber PeerConnection — receives remote tracks
    subscriber_pc: Arc<RwLock<Option<Arc<RTCPeerConnection>>>>,

    // Remote ICE candidates received before each PC's remote description.
    publisher_ice: Arc<Mutex<IceCandidateQueue>>,
    subscriber_ice: Arc<Mutex<IceCandidateQueue>>,

    // Callbacks (publisher)
    on_ice_candidate: Arc<RwLock<Option<Box<dyn Fn(String) + Send + Sync>>>>,
    on_state_change: Arc<RwLock<Option<Box<dyn Fn(ConnectionState) + Send + Sync>>>>,
    on_remote_track: Arc<RwLock<Option<Box<dyn Fn(Arc<TrackRemote>) + Send + Sync>>>>,

    // Callbacks (subscriber)
    on_subscriber_ice_candidate: Arc<RwLock<Option<Box<dyn Fn(String) + Send + Sync>>>>,
    on_subscriber_track:
        Arc<RwLock<Option<Box<dyn Fn(Arc<TrackRemote>, Arc<RTCRtpReceiver>) + Send + Sync>>>>,

    // Audio data channels (reserved for future use)
    #[allow(dead_code)]
    audio_tx: Arc<RwLock<Option<mpsc::Sender<Vec<u8>>>>>,
    #[allow(dead_code)]
    audio_rx: Arc<RwLock<Option<mpsc::Receiver<Vec<u8>>>>>,
}

impl WebRtcClient {
    /// Create a new WebRTC client
    pub fn new() -> Result<Self, WebRtcError> {
        // Configure MediaEngine with Opus audio codec (matching server)
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
            .map_err(|e| WebRtcError::ApiError(e.to_string()))?;

        // Register VP8 video codec (only video codec — matches server PT 96)
        let video_rtcp_feedback = vec![
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
        ];

        media_engine
            .register_codec(
                RTCRtpCodecParameters {
                    capability: RTCRtpCodecCapability {
                        mime_type: "video/VP8".to_string(),
                        clock_rate: 90000,
                        channels: 0,
                        sdp_fmtp_line: String::new(),
                        rtcp_feedback: video_rtcp_feedback,
                    },
                    payload_type: VP8_PAYLOAD_TYPE,
                    ..Default::default()
                },
                RTPCodecType::Video,
            )
            .map_err(|e| WebRtcError::ApiError(e.to_string()))?;

        // Create interceptor registry
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)
            .map_err(|e| WebRtcError::ApiError(e.to_string()))?;

        // Build WebRTC API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .build();

        info!("WebRTC client initialized");

        Ok(Self {
            api: Arc::new(api),
            peer_connection: Arc::new(RwLock::new(None)),
            audio_sender: Arc::new(RwLock::new(None)),
            local_track: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            channel_id: Arc::new(RwLock::new(None)),
            subscriber_pc: Arc::new(RwLock::new(None)),
            publisher_ice: Arc::new(Mutex::new(IceCandidateQueue::default())),
            subscriber_ice: Arc::new(Mutex::new(IceCandidateQueue::default())),
            on_ice_candidate: Arc::new(RwLock::new(None)),
            on_state_change: Arc::new(RwLock::new(None)),
            on_remote_track: Arc::new(RwLock::new(None)),
            on_subscriber_ice_candidate: Arc::new(RwLock::new(None)),
            on_subscriber_track: Arc::new(RwLock::new(None)),
            audio_tx: Arc::new(RwLock::new(None)),
            audio_rx: Arc::new(RwLock::new(None)),
        })
    }

    /// Set ICE candidate callback
    pub async fn set_on_ice_candidate<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        *self.on_ice_candidate.write().await = Some(Box::new(callback));
    }

    /// Set state change callback
    pub async fn set_on_state_change<F>(&self, callback: F)
    where
        F: Fn(ConnectionState) + Send + Sync + 'static,
    {
        *self.on_state_change.write().await = Some(Box::new(callback));
    }

    /// Set remote track callback
    pub async fn set_on_remote_track<F>(&self, callback: F)
    where
        F: Fn(Arc<TrackRemote>) + Send + Sync + 'static,
    {
        *self.on_remote_track.write().await = Some(Box::new(callback));
    }

    /// Set subscriber ICE candidate callback
    pub async fn set_on_subscriber_ice_candidate<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        *self.on_subscriber_ice_candidate.write().await = Some(Box::new(callback));
    }

    /// Set subscriber track callback — fires when a remote track arrives on the subscriber PC
    pub async fn set_on_subscriber_track<F>(&self, callback: F)
    where
        F: Fn(Arc<TrackRemote>, Arc<RTCRtpReceiver>) + Send + Sync + 'static,
    {
        *self.on_subscriber_track.write().await = Some(Box::new(callback));
    }

    /// Get current connection state
    pub async fn get_state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Get current channel ID
    pub async fn get_channel_id(&self) -> Option<String> {
        self.channel_id.read().await.clone()
    }

    /// Clone the publisher `RTCPeerConnection` handle, if connected.
    ///
    /// Callers should clone the handle out and drop any application-level
    /// locks before awaiting `get_stats()` on it — stats collection walks
    /// every transceiver and can be slow on a degraded connection.
    pub async fn publisher_pc(&self) -> Option<Arc<RTCPeerConnection>> {
        self.peer_connection.read().await.clone()
    }

    /// Extract the round-trip time in milliseconds from a publisher stats
    /// report.
    ///
    /// Primary source is `RemoteInboundRTP` (audio): the SFU's RTCP receiver
    /// reports about our published stream. webrtc-rs reports this value in
    /// MILLISECONDS already (interceptor's `calculate_rtt_ms`), deviating
    /// from the W3C stat of the same name, which is in seconds — do not
    /// convert. The nominated `CandidatePair`'s `current_round_trip_time`
    /// is kept as a fallback, but note webrtc-ice 0.17 never populates it
    /// (always 0.0), so today this returns `None` until the first receiver
    /// report arrives — the frontend maps that to the "unknown" state.
    ///
    /// Receive-side jitter and packet loss are not available from webrtc-rs
    /// stats (upstream TODOs) and are measured in the audio decode loop —
    /// see `voice::connection_stats`.
    pub fn extract_publisher_rtt_ms(report: &webrtc::stats::StatsReport) -> Option<f64> {
        let mut rtt_ms: Option<f64> = None;
        let mut fallback_rtt_ms: Option<f64> = None;

        for stats in report.reports.values() {
            match stats {
                StatsReportType::RemoteInboundRTP(remote) if remote.kind == "audio" => {
                    if let Some(rtt) = remote.round_trip_time {
                        // Already in ms — see doc comment above.
                        rtt_ms = Some(rtt);
                    }
                }
                StatsReportType::CandidatePair(pair)
                    if pair.nominated && pair.current_round_trip_time > 0.0 =>
                {
                    fallback_rtt_ms = Some(pair.current_round_trip_time * 1000.0);
                }
                _ => {}
            }
        }

        rtt_ms.or(fallback_rtt_ms)
    }

    /// Create `RTCConfiguration` from ICE server config
    fn create_rtc_config(ice_servers: &[IceServerConfig]) -> RTCConfiguration {
        let ice_servers: Vec<RTCIceServer> = ice_servers
            .iter()
            .map(|s| RTCIceServer {
                urls: s.urls.clone(),
                username: s.username.clone().unwrap_or_default(),
                credential: s.credential.clone().unwrap_or_default(),
            })
            .collect();

        RTCConfiguration {
            ice_servers,
            ..Default::default()
        }
    }

    /// Connect to a voice channel (creates peer connection, waits for offer)
    pub async fn connect(
        &self,
        channel_id: &str,
        ice_servers: &[IceServerConfig],
    ) -> Result<(), WebRtcError> {
        // Check if already connected
        if self.peer_connection.read().await.is_some() {
            return Err(WebRtcError::AlreadyConnected);
        }

        // Update state
        *self.state.write().await = ConnectionState::Connecting;
        *self.channel_id.write().await = Some(channel_id.to_string());
        *self.publisher_ice.lock().await = IceCandidateQueue::default();
        *self.subscriber_ice.lock().await = IceCandidateQueue::default();

        // Create peer connection
        let config = Self::create_rtc_config(ice_servers);
        let peer_connection = self
            .api
            .new_peer_connection(config)
            .await
            .map_err(|e| WebRtcError::PeerConnectionError(e.to_string()))?;

        let pc = Arc::new(peer_connection);

        // Set up event handlers
        self.setup_event_handlers(pc.clone()).await?;

        // Create local audio track
        let local_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "audio/opus".to_string(),
                clock_rate: 48000,
                channels: 2,
                sdp_fmtp_line: "minptime=10;useinbandfec=1".to_string(),
                rtcp_feedback: vec![],
            },
            "audio".to_string(),
            "voice-stream".to_string(),
        ));

        // Add transceiver for sendrecv audio
        let sender = pc
            .add_track(local_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|e| WebRtcError::TrackError(e.to_string()))?;

        // Create subscriber PeerConnection (receives remote tracks from SFU)
        let sub_config = Self::create_rtc_config(ice_servers);
        let subscriber_connection = self
            .api
            .new_peer_connection(sub_config)
            .await
            .map_err(|e| WebRtcError::PeerConnectionError(e.to_string()))?;
        let sub_pc = Arc::new(subscriber_connection);

        // Wire subscriber event handlers
        self.setup_subscriber_event_handlers(sub_pc.clone()).await?;

        // Store references
        *self.peer_connection.write().await = Some(pc);
        *self.audio_sender.write().await = Some(sender);
        *self.local_track.write().await = Some(local_track);
        *self.subscriber_pc.write().await = Some(sub_pc);

        info!(
            "WebRTC peer connections created for channel {} (publisher + subscriber)",
            channel_id
        );
        Ok(())
    }

    /// Set up peer connection event handlers
    async fn setup_event_handlers(&self, pc: Arc<RTCPeerConnection>) -> Result<(), WebRtcError> {
        // ICE candidate handler
        let on_ice_candidate = self.on_ice_candidate.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let on_ice_candidate = on_ice_candidate.clone();
            Box::pin(async move {
                let Some(candidate) = candidate else { return };
                let json = match candidate.to_json() {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to convert ICE candidate to JSON");
                        return;
                    }
                };
                let candidate_str = match serde_json::to_string(&json) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to serialize ICE candidate JSON");
                        return;
                    }
                };
                let callback = on_ice_candidate.read().await;
                if let Some(ref cb) = *callback {
                    cb(candidate_str);
                }
            })
        }));

        // Connection state change handler
        let state = self.state.clone();
        let on_state_change = self.on_state_change.clone();
        pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            let state = state.clone();
            let on_state_change = on_state_change.clone();
            Box::pin(async move {
                let new_state = publisher_state(s);

                *state.write().await = new_state;

                let callback = on_state_change.read().await;
                if let Some(ref cb) = *callback {
                    cb(new_state);
                }

                info!("Peer connection state changed: {:?}", s);
            })
        }));

        // Remote track handler
        let on_remote_track = self.on_remote_track.clone();
        pc.on_track(Box::new(
            move |track: Arc<TrackRemote>, _receiver, _transceiver| {
                let on_remote_track = on_remote_track.clone();
                Box::pin(async move {
                    info!(
                        "Remote track received: {} ({})",
                        track.kind(),
                        track.codec().capability.mime_type
                    );

                    let callback = on_remote_track.read().await;
                    if let Some(ref cb) = *callback {
                        cb(track);
                    }
                })
            },
        ));

        Ok(())
    }

    /// Set up subscriber peer connection event handlers
    async fn setup_subscriber_event_handlers(
        &self,
        pc: Arc<RTCPeerConnection>,
    ) -> Result<(), WebRtcError> {
        // ICE candidate handler for subscriber
        let on_ice_candidate = self.on_subscriber_ice_candidate.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let on_ice_candidate = on_ice_candidate.clone();
            Box::pin(async move {
                let Some(candidate) = candidate else { return };
                let json = match candidate.to_json() {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to convert subscriber ICE candidate to JSON");
                        return;
                    }
                };
                let candidate_str = match serde_json::to_string(&json) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to serialize subscriber ICE candidate JSON");
                        return;
                    }
                };
                let callback = on_ice_candidate.read().await;
                if let Some(ref cb) = *callback {
                    cb(candidate_str);
                }
            })
        }));

        // Connection state change handler for subscriber
        pc.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            Box::pin(async move {
                // Only log subscriber state changes — publisher state is the primary indicator.
                // Writing to shared state here would be misleading since the publisher PC
                // may still be healthy.
                if s == RTCPeerConnectionState::Failed {
                    tracing::warn!(
                        "Subscriber PeerConnection failed — remote audio/video may be unavailable"
                    );
                }
                info!("Subscriber peer connection state changed: {:?}", s);
            })
        }));

        // Remote track handler for subscriber
        let on_subscriber_track = self.on_subscriber_track.clone();
        pc.on_track(Box::new(
            move |track: Arc<TrackRemote>, receiver: Arc<RTCRtpReceiver>, _transceiver| {
                let on_subscriber_track = on_subscriber_track.clone();
                Box::pin(async move {
                    info!(
                        "Subscriber remote track received: {} ({})",
                        track.kind(),
                        track.codec().capability.mime_type
                    );

                    let callback = on_subscriber_track.read().await;
                    if let Some(ref cb) = *callback {
                        cb(track, receiver);
                    }
                })
            },
        ));

        Ok(())
    }

    /// Create an SDP offer for the publisher `PeerConnection`.
    ///
    /// The client creates the offer and sends it to the server. The server
    /// will respond with an answer via `VoicePublisherAnswer`.
    pub async fn create_offer(&self) -> Result<String, WebRtcError> {
        let pc = self
            .peer_connection
            .read()
            .await
            .clone()
            .ok_or(WebRtcError::NotConnected)?;

        // Create offer
        let offer = pc
            .create_offer(None)
            .await
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;

        // Set local description
        pc.set_local_description(offer.clone())
            .await
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;

        debug!("Publisher offer created and local description set");
        Ok(offer.sdp)
    }

    /// Set the remote SDP answer on the publisher `PeerConnection`.
    ///
    /// Called after the server responds to our offer with a `VoicePublisherAnswer`.
    pub async fn set_remote_answer(&self, sdp: &str) -> Result<(), WebRtcError> {
        let pc = self
            .peer_connection
            .read()
            .await
            .clone()
            .ok_or(WebRtcError::NotConnected)?;

        // Parse answer SDP
        let answer = RTCSessionDescription::answer(sdp.to_string())
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;

        // Set remote description
        pc.set_remote_description(answer)
            .await
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;
        let queued = self.publisher_ice.lock().await.open();
        apply_queued_candidates(&pc, queued, "publisher").await;

        debug!("Remote answer set on publisher PC");
        Ok(())
    }

    /// Handle SDP offer from server, return SDP answer
    pub async fn handle_offer(&self, sdp: &str) -> Result<String, WebRtcError> {
        let pc = self
            .peer_connection
            .read()
            .await
            .clone()
            .ok_or(WebRtcError::NotConnected)?;

        // Parse offer SDP
        let offer = RTCSessionDescription::offer(sdp.to_string())
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;

        // Set remote description
        pc.set_remote_description(offer)
            .await
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;
        let queued = self.publisher_ice.lock().await.open();
        apply_queued_candidates(&pc, queued, "publisher").await;

        debug!("Remote description set");

        // Create answer
        let answer = pc
            .create_answer(None)
            .await
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;

        // Set local description
        pc.set_local_description(answer.clone())
            .await
            .map_err(|e| WebRtcError::SdpError(e.to_string()))?;

        debug!("Local description set");

        Ok(answer.sdp)
    }

    /// Add ICE candidate from server
    pub async fn add_ice_candidate(&self, candidate_json: &str) -> Result<(), WebRtcError> {
        let pc = self
            .peer_connection
            .read()
            .await
            .clone()
            .ok_or(WebRtcError::NotConnected)?;

        // Parse candidate
        let candidate_init: RTCIceCandidateInit = serde_json::from_str(candidate_json)
            .map_err(|e| WebRtcError::IceError(e.to_string()))?;

        // Before the answer is applied the candidate waits in the queue.
        let Some(candidate_init) = self
            .publisher_ice
            .lock()
            .await
            .queue_or_pass(candidate_init)
        else {
            debug!("ICE candidate queued until the remote description is set");
            return Ok(());
        };

        // Add candidate
        pc.add_ice_candidate(candidate_init)
            .await
            .map_err(|e| WebRtcError::IceError(e.to_string()))?;

        debug!("ICE candidate added");
        Ok(())
    }

    /// Number of publisher candidates waiting for the remote description.
    pub async fn pending_ice_candidates(&self) -> usize {
        self.publisher_ice.lock().await.pending.len()
    }

    /// Number of subscriber candidates waiting for the server's offer.
    pub async fn pending_subscriber_ice_candidates(&self) -> usize {
        self.subscriber_ice.lock().await.pending.len()
    }

    /// Handle a subscriber offer from the server
    pub async fn handle_subscriber_offer(&self, sdp: &str) -> Result<String, WebRtcError> {
        let pc = self.subscriber_pc.read().await;
        let pc = pc.as_ref().ok_or(WebRtcError::NotConnected)?;

        let offer = RTCSessionDescription::offer(sdp.to_string())
            .map_err(|e| WebRtcError::SdpError(format!("subscriber parse offer: {e}")))?;

        pc.set_remote_description(offer)
            .await
            .map_err(|e| WebRtcError::SdpError(format!("subscriber set remote: {e}")))?;
        let queued = self.subscriber_ice.lock().await.open();
        apply_queued_candidates(pc, queued, "subscriber").await;

        let answer = pc
            .create_answer(None)
            .await
            .map_err(|e| WebRtcError::SdpError(format!("subscriber create answer: {e}")))?;

        pc.set_local_description(answer.clone())
            .await
            .map_err(|e| WebRtcError::SdpError(format!("subscriber set local: {e}")))?;

        debug!("Subscriber offer handled, answer created");
        Ok(answer.sdp)
    }

    /// Add ICE candidate to subscriber PC
    pub async fn add_subscriber_ice_candidate(
        &self,
        candidate_json: &str,
    ) -> Result<(), WebRtcError> {
        let pc = self.subscriber_pc.read().await;
        let pc = pc.as_ref().ok_or(WebRtcError::NotConnected)?;

        let candidate_init: RTCIceCandidateInit = serde_json::from_str(candidate_json)
            .map_err(|e| WebRtcError::IceError(format!("parse: {e}")))?;

        let Some(candidate_init) = self
            .subscriber_ice
            .lock()
            .await
            .queue_or_pass(candidate_init)
        else {
            debug!("Subscriber ICE candidate queued until the offer is applied");
            return Ok(());
        };

        pc.add_ice_candidate(candidate_init)
            .await
            .map_err(|e| WebRtcError::IceError(format!("subscriber add: {e}")))?;

        debug!("Subscriber ICE candidate added");
        Ok(())
    }

    /// Get the local track for sending audio
    pub async fn get_local_track(&self) -> Option<Arc<TrackLocalStaticRTP>> {
        (*self.local_track.read().await).clone()
    }

    /// Add a fresh video sender for one publication (one screen share or the
    /// webcam) to the publisher connection. Each publication gets its own
    /// track and m-line, so the server sees a new track per share and a
    /// later webcam is never taken for an old share. Renegotiation follows
    /// (see [`PublisherNegotiation`]).
    pub async fn add_video_sender(
        &self,
        kind: VideoKind,
        publication_id: &str,
    ) -> Result<(Arc<TrackLocalStaticRTP>, Arc<RTCRtpSender>), WebRtcError> {
        let pc = self
            .peer_connection
            .read()
            .await
            .clone()
            .ok_or(WebRtcError::NotConnected)?;
        let track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/VP8".to_string(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: String::new(),
                rtcp_feedback: vec![],
            },
            format!("{}-{publication_id}", kind.track_prefix()),
            format!("{}-{publication_id}", kind.stream_prefix()),
        ));
        // `send_encodings` is ignored by webrtc-rs (see `simulcast_encodings`).
        let transceiver = pc
            .add_transceiver_from_track(
                track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: simulcast_encodings(),
                }),
            )
            .await
            .map_err(|e| WebRtcError::TrackError(e.to_string()))?;
        Ok((track, transceiver.sender().await))
    }

    /// Remove a publication's sender (its m-line goes inactive with the next
    /// offer, and its RTCP reader ends).
    pub async fn remove_video_sender(&self, sender: &Arc<RTCRtpSender>) -> Result<(), WebRtcError> {
        let Some(pc) = self.peer_connection.read().await.clone() else {
            return Ok(()); // the connection is gone and its senders with it
        };
        pc.remove_track(sender)
            .await
            .map_err(|e| WebRtcError::TrackError(e.to_string()))
    }

    /// Ask the subscriber connection's sender of `media_ssrc` for a key frame
    /// (a viewer that subscribed late).
    pub async fn request_subscriber_keyframe(&self, media_ssrc: u32) {
        let Some(pc) = self.subscriber_pc.read().await.clone() else {
            return;
        };
        let pli = webrtc::rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication {
            sender_ssrc: 0,
            media_ssrc,
        };
        if let Err(e) = pc.write_rtcp(&[Box::new(pli)]).await {
            debug!("Key frame request on the subscriber connection failed: {e}");
        }
    }

    /// Disconnect and clean up
    pub async fn disconnect(&self) -> Result<(), WebRtcError> {
        // Close publisher peer connection
        if let Some(pc) = self.peer_connection.write().await.take() {
            pc.close()
                .await
                .map_err(|e| WebRtcError::PeerConnectionError(e.to_string()))?;
        }

        // Close subscriber peer connection
        if let Some(pc) = self.subscriber_pc.write().await.take() {
            pc.close()
                .await
                .map_err(|e| WebRtcError::PeerConnectionError(format!("subscriber close: {e}")))?;
        }

        // Clear state
        *self.audio_sender.write().await = None;
        *self.local_track.write().await = None;
        *self.state.write().await = ConnectionState::Disconnected;
        *self.channel_id.write().await = None;
        *self.publisher_ice.lock().await = IceCandidateQueue::default();
        *self.subscriber_ice.lock().await = IceCandidateQueue::default();

        info!("WebRTC disconnected");
        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        *self.state.read().await == ConnectionState::Connected
    }
}

impl Default for WebRtcClient {
    fn default() -> Self {
        Self::new().expect("Failed to create WebRTC client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ice_outage_is_reconnecting_not_disconnected() {
        assert_eq!(
            publisher_state(RTCPeerConnectionState::Disconnected),
            ConnectionState::Reconnecting
        );
        assert_eq!(
            publisher_state(RTCPeerConnectionState::Connected),
            ConnectionState::Connected
        );
    }

    #[test]
    fn only_closed_ends_and_failed_stays_failed() {
        assert_eq!(
            publisher_state(RTCPeerConnectionState::Closed),
            ConnectionState::Disconnected
        );
        assert_eq!(
            publisher_state(RTCPeerConnectionState::Failed),
            ConnectionState::Failed
        );
        assert_eq!(
            publisher_state(RTCPeerConnectionState::New),
            ConnectionState::Connecting
        );
    }

    #[test]
    fn test_webrtc_client_creation() {
        let client = WebRtcClient::new();
        assert!(client.is_ok());
    }

    /// Answer `offer` from a plain peer connection (stands in for the SFU).
    async fn sfu_answer(offer: String) -> (Arc<RTCPeerConnection>, String) {
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs().unwrap();
        let api = APIBuilder::new().with_media_engine(media_engine).build();
        let pc = Arc::new(
            api.new_peer_connection(RTCConfiguration::default())
                .await
                .unwrap(),
        );
        pc.set_remote_description(RTCSessionDescription::offer(offer).unwrap())
            .await
            .unwrap();
        let answer = pc.create_answer(None).await.unwrap();
        pc.set_local_description(answer.clone()).await.unwrap();
        (pc, answer.sdp)
    }

    const HOST_CANDIDATE: &str = r#"{"candidate":"candidate:1 1 udp 2130706431 127.0.0.1 50000 typ host","sdpMid":"0","sdpMLineIndex":0}"#;

    #[tokio::test]
    async fn candidates_before_remote_description_are_applied_after() {
        let client = WebRtcClient::new().unwrap();
        client.connect("channel", &[]).await.unwrap();
        let offer = client.create_offer().await.unwrap();

        // The server's candidate overtakes its answer: it must be kept, not lost.
        client
            .add_ice_candidate(HOST_CANDIDATE)
            .await
            .expect("early candidate is queued, not rejected");
        assert_eq!(client.pending_ice_candidates().await, 1);

        let (sfu, answer) = sfu_answer(offer).await;
        client.set_remote_answer(&answer).await.unwrap();
        assert_eq!(client.pending_ice_candidates().await, 0, "queue flushed");

        // The flushed candidate reached the ICE agent (it registers remote
        // candidates asynchronously, so poll briefly).
        let pc = client.publisher_pc().await.unwrap();
        let mut has_remote = false;
        for _ in 0..50 {
            let report = pc.get_stats().await;
            has_remote = report
                .reports
                .values()
                .any(|r| matches!(r, StatsReportType::RemoteCandidate(c) if c.port == 50000));
            if has_remote {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(
            has_remote,
            "flushed candidate applied to the peer connection"
        );

        // Later candidates go straight through.
        client.add_ice_candidate(HOST_CANDIDATE).await.unwrap();
        assert_eq!(client.pending_ice_candidates().await, 0);

        client.disconnect().await.unwrap();
        sfu.close().await.unwrap();
    }

    #[tokio::test]
    async fn subscriber_candidates_before_offer_are_applied_after() {
        let client = WebRtcClient::new().unwrap();
        client.connect("channel", &[]).await.unwrap();

        client
            .add_subscriber_ice_candidate(HOST_CANDIDATE)
            .await
            .expect("early subscriber candidate is queued");
        assert_eq!(client.pending_subscriber_ice_candidates().await, 1);

        // The SFU offers to the subscriber PC.
        let mut media_engine = MediaEngine::default();
        media_engine.register_default_codecs().unwrap();
        let api = APIBuilder::new().with_media_engine(media_engine).build();
        let sfu = api
            .new_peer_connection(RTCConfiguration::default())
            .await
            .unwrap();
        sfu.add_transceiver_from_kind(RTPCodecType::Audio, None)
            .await
            .unwrap();
        let offer = sfu.create_offer(None).await.unwrap();
        sfu.set_local_description(offer.clone()).await.unwrap();

        client.handle_subscriber_offer(&offer.sdp).await.unwrap();
        assert_eq!(client.pending_subscriber_ice_candidates().await, 0);

        client.disconnect().await.unwrap();
        sfu.close().await.unwrap();
    }

    #[test]
    fn negotiation_never_has_two_offers_out() {
        let mut n = PublisherNegotiation::default();
        let first = n.request().expect("first change: offer now");
        assert_eq!(n.request(), None, "second change while waiting: queued");
        assert_eq!(n.request(), None, "third change: still one queued offer");
        let next = n.answered().expect("answer arrives: send the queued offer");
        assert_ne!(first, next);
        assert_eq!(n.outstanding_id(), Some(next));
        assert_eq!(n.answered(), None, "its answer: nothing more to send");
        assert!(!n.is_awaiting_answer());
    }

    #[test]
    fn a_lost_answer_is_offered_once_more_then_given_up() {
        let mut n = PublisherNegotiation::default();
        let id = n.request().unwrap();
        assert_eq!(n.request(), None, "a change waits behind the lost offer");
        let OfferFailure::Retry(retry) = n.offer_failed(id) else {
            panic!("first failure retries")
        };
        assert_eq!(
            n.offer_failed(id),
            OfferFailure::Stale,
            "the old id is gone"
        );
        assert_eq!(n.offer_failed(retry), OfferFailure::GiveUp);
        assert!(!n.is_awaiting_answer(), "nothing stays stuck");
        assert!(n.request().is_some(), "the next change offers at once");
    }

    #[test]
    fn offer_ids_never_repeat_across_sessions() {
        let mut first = PublisherNegotiation::default();
        let a = first.request().unwrap();
        // The next session starts a fresh guard: its offer has a new id, so
        // the old session's deadline (id `a`) cannot match it.
        let mut second = PublisherNegotiation::default();
        let b = second.request().unwrap();
        assert_ne!(a, b);
        assert_eq!(second.offer_failed(a), OfferFailure::Stale);
    }

    #[test]
    fn a_retry_that_gets_its_answer_clears_the_guard() {
        let mut n = PublisherNegotiation::default();
        let id = n.request().unwrap();
        let OfferFailure::Retry(retry) = n.offer_failed(id) else {
            panic!("retry")
        };
        assert_eq!(n.outstanding_id(), Some(retry));
        assert_eq!(n.answered(), None);
        assert_eq!(
            n.offer_failed(retry),
            OfferFailure::Stale,
            "answered: deadline is moot"
        );
    }

    /// (stream id, direction) of each media section that names a stream.
    fn sections(sdp: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for section in sdp.split("m=").skip(1) {
            let stream = section
                .lines()
                .find_map(|l| l.strip_prefix("a=msid:"))
                .and_then(|l| l.split_whitespace().next());
            let dir = section.lines().find(|l| {
                matches!(
                    *l,
                    "a=sendonly" | "a=recvonly" | "a=inactive" | "a=sendrecv"
                )
            });
            if let (Some(stream), Some(dir)) = (stream, dir) {
                out.push((stream.to_string(), dir.trim_start_matches("a=").to_string()));
            }
        }
        out
    }

    fn msid_streams(sdp: &str) -> Vec<String> {
        sdp.lines()
            .filter_map(|l| l.strip_prefix("a=msid:"))
            .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
            .collect()
    }

    #[tokio::test]
    async fn share_stop_share_again_gets_a_new_track_each_time() {
        let client = WebRtcClient::new().unwrap();
        client.connect("ch", &[]).await.unwrap();
        let offer = client.create_offer().await.unwrap();
        assert!(
            !offer.contains("m=video"),
            "no video m-line before anything is published"
        );

        let (first, first_sender) = client
            .add_video_sender(VideoKind::Screen, "a")
            .await
            .unwrap();
        let offer = client.create_offer().await.unwrap();
        assert!(msid_streams(&offer).contains(&"screen-share-stream-a".to_string()));

        client.remove_video_sender(&first_sender).await.unwrap();
        let (second, _) = client
            .add_video_sender(VideoKind::Screen, "b")
            .await
            .unwrap();
        assert_ne!(first.id(), second.id());
        let offer = client.create_offer().await.unwrap();
        let sections = sections(&offer);
        assert!(sections.contains(&("screen-share-stream-b".into(), "sendonly".into())));
        assert!(
            sections.contains(&("screen-share-stream-a".into(), "inactive".into())),
            "the stopped share's m-line is inactive: nothing of it is sent"
        );
        client.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn share_then_webcam_are_separate_tracks_of_the_right_kind() {
        let client = WebRtcClient::new().unwrap();
        client.connect("ch", &[]).await.unwrap();
        let (screen, _) = client
            .add_video_sender(VideoKind::Screen, "s1")
            .await
            .unwrap();
        let (cam, _) = client
            .add_video_sender(VideoKind::Webcam, "c1")
            .await
            .unwrap();
        assert_eq!(screen.id(), "screen-video-s1");
        assert_eq!(cam.id(), "webcam-video-c1");
        let offer = client.create_offer().await.unwrap();
        let streams = msid_streams(&offer);
        assert!(streams.contains(&"screen-share-stream-s1".to_string()));
        assert!(streams.contains(&"webcam-stream-c1".to_string()));
        assert_eq!(offer.matches("m=video").count(), 2);
        client.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn no_video_sender_without_a_connection() {
        let client = WebRtcClient::new().unwrap();
        assert!(matches!(
            client.add_video_sender(VideoKind::Screen, "x").await,
            Err(WebRtcError::NotConnected)
        ));
    }
}
