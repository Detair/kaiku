//! WebRTC Client
//!
//! Handles WebRTC peer connection for voice chat.

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info};
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
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocal;
use webrtc::track::track_remote::TrackRemote;

/// Build 3-layer simulcast encoding parameters (high / medium / low).
///
/// **Limitation:** webrtc-rs 0.11 `add_transceiver_from_track` accepts
/// `send_encodings` but only uses the `rid` field to annotate the SDP.
/// It does **not** actually produce multiple encoded streams at different
/// resolutions/bitrates — the Tauri client sends a single RTP stream from
/// its capture + VP9-encode pipeline. True simulcast requires feeding
/// three separately encoded streams, which is tracked as future work.
///
/// The RID values ("h", "m", "l") are set so the SDP matches the browser
/// simulcast offer, allowing the SFU to parse the layers correctly.
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
    Failed,
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

/// WebRTC client for voice chat and screen sharing
pub struct WebRtcClient {
    api: Arc<API>,
    peer_connection: Arc<RwLock<Option<Arc<RTCPeerConnection>>>>,
    audio_sender: Arc<RwLock<Option<Arc<RTCRtpSender>>>>,
    local_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,
    state: Arc<RwLock<ConnectionState>>,
    channel_id: Arc<RwLock<Option<String>>>,

    // Video track for screen sharing (always added at connect time to avoid SDP renegotiation)
    video_sender: Arc<RwLock<Option<Arc<RTCRtpSender>>>>,
    video_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,

    // Video track for webcam (separate from screen share, both can be active simultaneously)
    webcam_sender: Arc<RwLock<Option<Arc<RTCRtpSender>>>>,
    webcam_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,

    // Subscriber PeerConnection — receives remote tracks
    subscriber_pc: Arc<RwLock<Option<Arc<RTCPeerConnection>>>>,

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
                    payload_type: 96,
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
            video_sender: Arc::new(RwLock::new(None)),
            video_track: Arc::new(RwLock::new(None)),
            webcam_sender: Arc::new(RwLock::new(None)),
            webcam_track: Arc::new(RwLock::new(None)),
            subscriber_pc: Arc::new(RwLock::new(None)),
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

    /// Create `RTCConfiguration` from ICE server config
    fn create_rtc_config(ice_servers: &[IceServerConfig]) -> RTCConfiguration {
        let ice_servers: Vec<RTCIceServer> = ice_servers
            .iter()
            .map(|s| RTCIceServer {
                urls: s.urls.clone(),
                username: s.username.clone().unwrap_or_default(),
                credential: s.credential.clone().unwrap_or_default(),
                ..Default::default()
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

        // Create video track for screen sharing (always added to avoid SDP renegotiation)
        // Uses add_transceiver_from_track with simulcast RID encodings (h/m/l)
        // so the SDP advertises simulcast layers matching the browser path.
        // TODO(simulcast): webrtc-rs 0.11 only uses the RID from send_encodings
        // for SDP generation — it does not produce multiple encoded streams.
        // True 3-layer simulcast requires feeding 3 separately encoded RTP
        // streams from the capture pipeline (future work).
        let video_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/VP8".to_string(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: String::new(),
                rtcp_feedback: vec![],
            },
            "screen-video".to_string(),
            "screen-share-stream".to_string(),
        ));

        let video_transceiver = pc
            .add_transceiver_from_track(
                video_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: simulcast_encodings(),
                }),
            )
            .await
            .map_err(|e| WebRtcError::TrackError(e.to_string()))?;
        let video_sender = video_transceiver.sender().await;

        // Create webcam video track (separate from screen share, both can be active)
        // Same simulcast RID setup as screen share — see TODO above.
        let webcam_track = Arc::new(TrackLocalStaticRTP::new(
            RTCRtpCodecCapability {
                mime_type: "video/VP8".to_string(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: String::new(),
                rtcp_feedback: vec![],
            },
            "webcam-video".to_string(),
            "webcam-stream".to_string(),
        ));

        let webcam_transceiver = pc
            .add_transceiver_from_track(
                webcam_track.clone() as Arc<dyn TrackLocal + Send + Sync>,
                Some(RTCRtpTransceiverInit {
                    direction: RTCRtpTransceiverDirection::Sendonly,
                    send_encodings: simulcast_encodings(),
                }),
            )
            .await
            .map_err(|e| WebRtcError::TrackError(e.to_string()))?;
        let webcam_sender = webcam_transceiver.sender().await;

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
        *self.video_sender.write().await = Some(video_sender);
        *self.video_track.write().await = Some(video_track);
        *self.webcam_sender.write().await = Some(webcam_sender);
        *self.webcam_track.write().await = Some(webcam_track);
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
                let new_state = match s {
                    RTCPeerConnectionState::Connected => ConnectionState::Connected,
                    RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Closed => {
                        ConnectionState::Disconnected
                    }
                    RTCPeerConnectionState::Failed => ConnectionState::Failed,
                    _ => ConnectionState::Connecting,
                };

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

        // Add candidate
        pc.add_ice_candidate(candidate_init)
            .await
            .map_err(|e| WebRtcError::IceError(e.to_string()))?;

        debug!("ICE candidate added");
        Ok(())
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

    /// Get the video track for screen sharing
    pub async fn get_video_track(&self) -> Option<Arc<TrackLocalStaticRTP>> {
        (*self.video_track.read().await).clone()
    }

    /// Get the video track for webcam
    pub async fn get_webcam_track(&self) -> Option<Arc<TrackLocalStaticRTP>> {
        (*self.webcam_track.read().await).clone()
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
        *self.video_sender.write().await = None;
        *self.video_track.write().await = None;
        *self.webcam_sender.write().await = None;
        *self.webcam_track.write().await = None;
        *self.state.write().await = ConnectionState::Disconnected;
        *self.channel_id.write().await = None;

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
    fn test_webrtc_client_creation() {
        let client = WebRtcClient::new();
        assert!(client.is_ok());
    }
}
