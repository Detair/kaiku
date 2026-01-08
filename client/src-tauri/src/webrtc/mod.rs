//! WebRTC Client
//!
//! Handles WebRTC peer connection for voice chat.

use std::sync::Arc;

use tauri::AppHandle;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors, media_engine::MediaEngine, APIBuilder,
        API,
    },
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    interceptor::registry::Registry,
    media::track::track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocal},
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription, RTCPeerConnection,
    },
    rtp_transceiver::{
        rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType},
        rtp_sender::RTCRtpSender,
        rtp_transceiver_direction::RTCRtpTransceiverDirection,
        RTCRtpTransceiverInit,
    },
    track::track_remote::TrackRemote,
};

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

/// WebRTC client for voice chat
pub struct WebRtcClient {
    api: Arc<API>,
    peer_connection: Arc<RwLock<Option<Arc<RTCPeerConnection>>>>,
    audio_sender: Arc<RwLock<Option<Arc<RTCRtpSender>>>>,
    local_track: Arc<RwLock<Option<Arc<TrackLocalStaticRTP>>>>,
    state: Arc<RwLock<ConnectionState>>,
    channel_id: Arc<RwLock<Option<String>>>,

    // Callbacks
    on_ice_candidate: Arc<RwLock<Option<Box<dyn Fn(String) + Send + Sync>>>>,
    on_state_change: Arc<RwLock<Option<Box<dyn Fn(ConnectionState) + Send + Sync>>>>,
    on_remote_track: Arc<RwLock<Option<Box<dyn Fn(Arc<TrackRemote>) + Send + Sync>>>>,

    // Audio data channels
    audio_tx: Arc<RwLock<Option<mpsc::Sender<Vec<u8>>>>>,
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
            on_ice_candidate: Arc::new(RwLock::new(None)),
            on_state_change: Arc::new(RwLock::new(None)),
            on_remote_track: Arc::new(RwLock::new(None)),
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

    /// Get current connection state
    pub async fn get_state(&self) -> ConnectionState {
        *self.state.read().await
    }

    /// Get current channel ID
    pub async fn get_channel_id(&self) -> Option<String> {
        self.channel_id.read().await.clone()
    }

    /// Create RTCConfiguration from ICE server config
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

        // Store references
        *self.peer_connection.write().await = Some(pc);
        *self.audio_sender.write().await = Some(sender);
        *self.local_track.write().await = Some(local_track);

        info!("WebRTC peer connection created for channel {}", channel_id);
        Ok(())
    }

    /// Set up peer connection event handlers
    async fn setup_event_handlers(&self, pc: Arc<RTCPeerConnection>) -> Result<(), WebRtcError> {
        // ICE candidate handler
        let on_ice_candidate = self.on_ice_candidate.clone();
        pc.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
            let on_ice_candidate = on_ice_candidate.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(json) = candidate.to_json() {
                        if let Ok(callback) = on_ice_candidate.try_read() {
                            if let Some(ref cb) = *callback {
                                cb(serde_json::to_string(&json).unwrap_or_default());
                            }
                        }
                    }
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

                if let Ok(callback) = on_state_change.try_read() {
                    if let Some(ref cb) = *callback {
                        cb(new_state);
                    }
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

                    if let Ok(callback) = on_remote_track.try_read() {
                        if let Some(ref cb) = *callback {
                            cb(track);
                        }
                    }
                })
            },
        ));

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

    /// Get the local track for sending audio
    pub async fn get_local_track(&self) -> Option<Arc<TrackLocalStaticRTP>> {
        self.local_track.read().await.clone()
    }

    /// Disconnect and clean up
    pub async fn disconnect(&self) -> Result<(), WebRtcError> {
        // Close peer connection
        if let Some(pc) = self.peer_connection.write().await.take() {
            pc.close()
                .await
                .map_err(|e| WebRtcError::PeerConnectionError(e.to_string()))?;
        }

        // Clear state
        *self.audio_sender.write().await = None;
        *self.local_track.write().await = None;
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
