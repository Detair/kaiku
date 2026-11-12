//! Voice Commands
//!
//! Tauri commands for voice chat functionality.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tauri::{command, AppHandle, Emitter, State};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use vc_common::protocol::PcType;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_remote::TrackRemote;

use crate::audio::{
    AudioDeviceList, AudioError, AudioErrorEvent, AudioErrorKind, AudioHandle, AUDIO_ERROR_EVENT,
    CHANNELS, FRAME_SIZE, FRAME_SIZE_MS, SAMPLE_RATE,
};
use crate::network::{ClientEvent, WebSocketManager};
use crate::voice::audio_mixer::AudioMixer;
use crate::webrtc::IceServerConfig;
use crate::{voice, AppState, VoiceState};

#[derive(serde::Deserialize)]
struct IceServersResponse {
    ice_servers: Vec<IceServerEntry>,
}

#[derive(serde::Deserialize)]
struct IceServerEntry {
    urls: Vec<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    credential: Option<String>,
}

/// Per-request timeout so an unresponsive server cannot stall voice join.
const ICE_SERVERS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Fetch STUN/TURN configuration from the server (`GET /api/voice/ice-servers`).
///
/// Falls back to the default public STUN server on any error or an empty list,
/// so voice still starts (without TURN relay) when the server is unreachable.
pub async fn fetch_ice_servers(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
) -> Vec<IceServerConfig> {
    let url = format!("{}/api/voice/ice-servers", server_url.trim_end_matches('/'));
    let result: Result<Vec<IceServerConfig>, String> = async {
        let resp = http
            .get(&url)
            .bearer_auth(token)
            .timeout(ICE_SERVERS_TIMEOUT)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let body: IceServersResponse = resp.json().await.map_err(|e| e.to_string())?;
        Ok(body
            .ice_servers
            .into_iter()
            .filter(|s| !s.urls.is_empty())
            .map(|s| IceServerConfig {
                urls: s.urls,
                username: s.username,
                credential: s.credential,
            })
            .collect())
    }
    .await;

    match result {
        Ok(servers) if !servers.is_empty() => servers,
        Ok(_) => {
            warn!("Server returned no ICE servers; TURN unavailable, using default STUN");
            vec![IceServerConfig::default()]
        }
        Err(e) => {
            warn!("ICE server fetch failed ({e}); TURN unavailable, using default STUN");
            vec![IceServerConfig::default()]
        }
    }
}

/// Error text of a join whose voice session the frontend already ended.
/// The frontend matches it (`NATIVE_JOIN_CANCELLED` in `lib/webrtc/tauri.ts`).
pub const JOIN_CANCELLED: &str = "VOICE_JOIN_CANCELLED";

/// Voice session epochs (C32 amendment "leave and `voice_removed` win").
///
/// The native process allocates the epochs ([`begin_voice_session`]), so
/// every window and every reload of a window share one order. Allocating
/// an epoch cancels nothing. A leave names the session it ends: it raises
/// the floor just above that session's epoch before it waits for the voice
/// lock, so that session's join (and any older one) stops, and it tears
/// the native session down only if that is not a newer one
/// ([`leave_applies`]). A join whose epoch is below the floor is
/// cancelled: it checks after each await and right before each message it
/// sends. Joins and leaves without an epoch (older frontends) keep the old
/// behaviour: such a join is never cancelled, such a leave ends everything.
#[derive(Debug, Default)]
pub struct VoiceEpochGate {
    /// Last epoch handed out.
    last: AtomicU64,
    /// Joins below this epoch must stop.
    floor: AtomicU64,
}

impl VoiceEpochGate {
    /// A new voice session: the next epoch. Older sessions are not touched;
    /// only the leave that names a session ends it.
    pub fn begin_session(&self) -> u64 {
        self.last.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// A leave of the session with `epoch`: its join, and every older one,
    /// is cancelled. Sessions allocated later go on.
    pub fn cancel_through(&self, epoch: u64) {
        self.floor
            .fetch_max(epoch.saturating_add(1), Ordering::SeqCst);
    }

    /// A leave without an epoch: every session allocated so far is cancelled.
    pub fn cancel_all(&self) {
        let next = self.last.load(Ordering::SeqCst) + 1;
        self.floor.fetch_max(next, Ordering::SeqCst);
    }

    /// Whether a join running under `epoch` may go on.
    pub fn is_current(&self, epoch: Option<u64>) -> bool {
        epoch.is_none_or(|epoch| epoch >= self.floor.load(Ordering::SeqCst))
    }

    /// `Err(JOIN_CANCELLED)` when a join under `epoch` must stop.
    pub fn check(&self, epoch: Option<u64>) -> Result<(), String> {
        if self.is_current(epoch) {
            Ok(())
        } else {
            info!(epoch = ?epoch, "Voice join cancelled: its session ended");
            Err(JOIN_CANCELLED.to_string())
        }
    }
}

/// Whether a leave for the session `leave_epoch` may tear down the native
/// session established under `held_epoch`. A leave never ends a session
/// that is newer than the one it names; without an epoch on either side
/// (older frontends) it ends whatever is there.
pub const fn leave_applies(leave_epoch: Option<u64>, held_epoch: Option<u64>) -> bool {
    match (leave_epoch, held_epoch) {
        (Some(leave), Some(held)) => held <= leave,
        _ => true,
    }
}

/// Payload of `voice:session_state`: the publisher state of the native
/// session established under `epoch` (wire format, hence `snake_case`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionStateEvent {
    /// Epoch of the session the state belongs to (`None`: joined without one).
    pub epoch: Option<u64>,
    /// `ConnectionState` name (`Connected`, `Reconnecting`, `Failed`, ...).
    pub state: String,
}

/// Start a voice session: returns its epoch for `join_voice` and for the
/// `leave_voice` that ends it. Allocated by the native process so reloads
/// and several windows share one order.
#[command]
pub fn begin_voice_session(state: State<'_, AppState>) -> u64 {
    state.voice_gate.begin_session()
}

/// Where a join's messages go (a seam for tests; the app uses the manager).
trait VoiceSignal {
    fn send_event(
        &self,
        event: ClientEvent,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

impl VoiceSignal for WebSocketManager {
    async fn send_event(&self, event: ClientEvent) -> Result<(), String> {
        self.send(event).await
    }
}

/// Why sending the join did not complete.
#[derive(Debug, PartialEq, Eq)]
enum JoinSendError {
    /// The session ended before `VoiceJoin` went out: nothing was sent.
    CancelledBeforeJoin,
    /// The session ended after `VoiceJoin` went out; a `VoiceLeave` was
    /// sent in its place (best effort) instead of the offer.
    CancelledAfterJoin,
    /// No socket, or a send failed.
    Failed(String),
}

/// Send `VoiceJoin` and the publisher offer, checking the session epoch
/// after the socket lock is acquired and right before each send: a leave or
/// removal that arrives while the lock is pending sends nothing.
async fn send_join_and_offer<W: VoiceSignal + Sync>(
    ws: &RwLock<Option<W>>,
    gate: &VoiceEpochGate,
    epoch: Option<u64>,
    channel_id: &str,
    offer_sdp: String,
) -> Result<(), JoinSendError> {
    let guard = ws.read().await;
    let Some(sender) = guard.as_ref() else {
        return Err(JoinSendError::Failed("WebSocket not connected".into()));
    };
    if gate.check(epoch).is_err() {
        return Err(JoinSendError::CancelledBeforeJoin);
    }
    sender
        .send_event(ClientEvent::VoiceJoin {
            channel_id: channel_id.to_string(),
        })
        .await
        .map_err(|e| JoinSendError::Failed(format!("Failed to send VoiceJoin: {e}")))?;
    if gate.check(epoch).is_err() {
        // Already in the room on the server: take it back.
        let _ = sender
            .send_event(ClientEvent::VoiceLeave {
                channel_id: channel_id.to_string(),
            })
            .await;
        return Err(JoinSendError::CancelledAfterJoin);
    }
    sender
        .send_event(ClientEvent::VoicePublisherOffer {
            channel_id: channel_id.to_string(),
            sdp: offer_sdp,
        })
        .await
        .map_err(|e| JoinSendError::Failed(format!("Failed to send VoicePublisherOffer: {e}")))
}

/// Join a voice channel.
///
/// Initializes audio pipeline and WebRTC, sends `VoiceJoin` to server.
/// After joining, the client creates a publisher offer and sends it via
/// `VoicePublisherOffer`. The server responds with `VoicePublisherAnswer`.
/// `epoch` is the frontend's voice session epoch ([`VoiceEpochGate`]).
#[command]
pub async fn join_voice(
    channel_id: String,
    epoch: Option<u64>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    info!("Joining voice channel: {}", channel_id);
    let gate = state.voice_gate.clone();

    // Initialize voice state if needed
    state.ensure_voice().await?;
    gate.check(epoch)?;

    // Server-provided STUN/TURN config (falls back to a public STUN server).
    // Fetched before taking the voice lock so a slow server cannot block it.
    let ice_servers = if let Some(info) = state.session.auth_info() {
        fetch_ice_servers(&state.http, &info.server_url, &info.access_token).await
    } else {
        warn!("No active session; using default STUN only, TURN unavailable");
        vec![IceServerConfig::default()]
    };
    gate.check(epoch)?;

    let mut voice = state.voice.write().await;
    // A leave that raced us took the lock first: nothing was created yet.
    gate.check(epoch)?;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    // Set app handle for speaking indicator events
    voice_state.audio.set_app_handle(app.clone());

    // Check if already in a channel
    if voice_state.channel_id.is_some() {
        return Err("Already in a voice channel. Leave first.".into());
    }
    // From here on the native session belongs to this join: the leave that
    // names this epoch (or a later one) tears it down, also after an error.
    voice_state.session_epoch = epoch;

    // Connect WebRTC (creates peer connection)
    voice_state
        .webrtc
        .connect(&channel_id, &ice_servers)
        .await
        .map_err(|e| e.to_string())?;

    // Set up ICE candidate callback to send candidates to server (publisher PC)
    let ws = state.websocket.clone();
    let channel_id_clone = channel_id.clone();
    voice_state
        .webrtc
        .set_on_ice_candidate(move |candidate| {
            let ws = ws.clone();
            let channel_id = channel_id_clone.clone();
            tokio::spawn(async move {
                if let Some(ws_manager) = ws.read().await.as_ref() {
                    if let Err(e) = ws_manager
                        .send(ClientEvent::VoiceIceCandidate {
                            channel_id,
                            candidate,
                            pc_type: PcType::Publisher,
                        })
                        .await
                    {
                        error!("Failed to send ICE candidate: {}", e);
                    }
                }
            });
        })
        .await;

    // Set up ICE candidate callback to send candidates to server (subscriber PC)
    let ws = state.websocket.clone();
    let channel_id_clone = channel_id.clone();
    voice_state
        .webrtc
        .set_on_subscriber_ice_candidate(move |candidate| {
            let ws = ws.clone();
            let channel_id = channel_id_clone.clone();
            tokio::spawn(async move {
                if let Some(ws_manager) = ws.read().await.as_ref() {
                    if let Err(e) = ws_manager
                        .send(ClientEvent::VoiceIceCandidate {
                            channel_id,
                            candidate,
                            pc_type: PcType::Subscriber,
                        })
                        .await
                    {
                        error!("Failed to send subscriber ICE candidate: {}", e);
                    }
                }
            });
        })
        .await;

    // Set up state change callback
    let app_clone = app.clone();
    voice_state
        .webrtc
        .set_on_state_change(move |new_state| {
            debug!("Voice connection state changed: {:?}", new_state);
            // With the session's epoch, so the frontend can tell a late
            // state of an ended session from the one it holds now.
            let _ = app_clone.emit(
                "voice:session_state",
                SessionStateEvent {
                    epoch,
                    state: format!("{new_state:?}"),
                },
            );
        })
        .await;

    // Set up subscriber track callback — fires when the SFU delivers a remote track
    let app_clone = app.clone();
    let voice_arc = state.voice.clone();
    let active_video_count = voice_state.active_video_count.clone();
    let video_tasks = voice_state.video_tasks.clone();
    let video_frame_sinks = voice_state.video_frame_sinks.clone();
    voice_state
        .webrtc
        .set_on_subscriber_track(move |track, _receiver| {
            let stream = track.stream_id();
            let (user_id, source_type) = voice::subscriber::parse_stream_id(&stream);

            if voice::subscriber::is_audio_track(&source_type) {
                let app = app_clone.clone();
                let voice_arc = voice_arc.clone();
                let track_id = format!("{user_id}:{source_type}");

                tokio::spawn(async move {
                    // Get the mixer and stats registry from voice state
                    let state_parts = {
                        let voice_guard = voice_arc.read().await;
                        voice_guard
                            .as_ref()
                            .map(|v| (v.audio_mixer.clone(), v.connection_stats.clone()))
                    };

                    let Some((Some(mixer), stats_registry)) = state_parts else {
                        warn!(
                            track_id = %track_id,
                            "Audio mixer not ready, dropping remote audio track"
                        );
                        return;
                    };

                    // Add track to mixer, get PCM sender
                    let pcm_tx = mixer.add_track(track_id.clone()).await;

                    info!(
                        track_id = %track_id,
                        user_id = %user_id,
                        "Remote audio track added to mixer"
                    );

                    // Emit track added event to frontend
                    let _ = app.emit(
                        "voice:remote_track",
                        serde_json::json!({
                            "user_id": user_id,
                            "source_type": source_type,
                        }),
                    );

                    // Decode RTP → Opus → PCM loop
                    decode_remote_audio_track(track, pcm_tx, &track_id, &stats_registry).await;

                    // Track ended — remove from mixer and stats registry
                    stats_registry.remove_track(&track_id);
                    mixer.remove_track(track_id.clone()).await;
                    info!(track_id = %track_id, "Remote audio track ended");

                    let _ = app.emit(
                        "voice:remote_track_removed",
                        serde_json::json!({
                            "user_id": user_id,
                            "source_type": source_type,
                        }),
                    );
                });
            } else if voice::subscriber::is_video_track(&source_type) {
                let count = active_video_count.clone();

                // Extract stream_id from source_type for screen shares
                // Format: "screen_video:uuid" → stream_id = "uuid"
                let extracted_stream_id =
                    if let Some(rest) = source_type.strip_prefix("screen_video:") {
                        rest.to_string()
                    } else {
                        "default".to_string()
                    };

                // Atomic check-and-increment to avoid TOCTOU race between
                // load() and fetch_add() when multiple tracks arrive concurrently.
                let was_under_cap = count.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |c| {
                    if c < voice::video_decoder::MAX_VIDEO_STREAMS {
                        Some(c + 1)
                    } else {
                        None
                    }
                });

                if was_under_cap.is_err() {
                    // Cap reached — emit metadata only so the UI knows the track exists.
                    info!(
                        user_id = %user_id,
                        source_type = %source_type,
                        "Video stream cap reached, metadata only"
                    );
                    let event_name = if source_type.starts_with("screen_video") {
                        "voice:screen_share_track"
                    } else {
                        "voice:webcam_track"
                    };
                    let _ = app_clone.emit(
                        event_name,
                        voice::video_decoder::VideoTrackEvent {
                            user_id,
                            stream_id: extracted_stream_id,
                            source_type,
                        },
                    );

                    // Drain track to prevent webrtc-rs backpressure
                    let drain_track = track;
                    tokio::spawn(async move {
                        let mut buf = vec![0u8; 4096];
                        while drain_track.read(&mut buf).await.is_ok() {}
                    });

                    return;
                }

                // Count already incremented by fetch_update — proceed with decode.
                let handle = voice::video_decoder::spawn_video_decode_task(
                    track,
                    user_id,
                    source_type,
                    extracted_stream_id,
                    app_clone.clone(),
                    count,
                    video_frame_sinks.clone(),
                );

                // Store handle for cleanup on leave_voice
                let tasks = video_tasks.clone();
                tokio::spawn(async move {
                    tasks.lock().await.push(handle);
                });
            } else {
                warn!(
                    stream_id = %stream,
                    "Unknown track source type"
                );
            }
        })
        .await;

    voice_state.channel_id = Some(channel_id.clone());

    // Create publisher offer
    let offer_sdp = voice_state
        .webrtc
        .create_offer()
        .await
        .map_err(|e| e.to_string())?;

    // Send VoiceJoin + offer; the epoch is checked again after the socket
    // lock and before each send. The voice lock is still held, so a leave
    // cannot have torn this connection down: a cancelled join undoes it.
    match send_join_and_offer(&state.websocket, &gate, epoch, &channel_id, offer_sdp).await {
        Ok(()) => {}
        Err(JoinSendError::Failed(e)) => return Err(e),
        Err(JoinSendError::CancelledBeforeJoin | JoinSendError::CancelledAfterJoin) => {
            voice_state.channel_id = None;
            voice_state.session_epoch = None;
            if let Err(e) = voice_state.webrtc.disconnect().await {
                warn!("Disconnect of a cancelled voice join failed: {e}");
            }
            return Err(JOIN_CANCELLED.to_string());
        }
    }

    info!("VoiceJoin + VoicePublisherOffer sent, waiting for server answer");
    Ok(())
}

/// Handle SDP answer from server for the publisher `PeerConnection`.
///
/// Called when frontend receives `ws:voice_publisher_answer` event.
/// Sets the remote description (answer) on the publisher PC and starts audio.
#[command]
pub async fn handle_voice_publisher_answer(
    channel_id: String,
    sdp: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    info!("Handling publisher answer for channel: {}", channel_id);

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    // Verify we're in this channel
    if voice_state.channel_id.as_ref() != Some(&channel_id) {
        return Err(format!(
            "Received publisher answer for wrong channel: {} (expected {:?})",
            channel_id, voice_state.channel_id
        ));
    }

    // Set the remote description (answer) on the publisher PC
    voice_state
        .webrtc
        .set_remote_answer(&sdp)
        .await
        .map_err(|e| e.to_string())?;

    // Fetched first: the uplink sender is wired before anything can fail.
    let local_track = voice_state.webrtc.get_local_track().await;
    let gains = voice_state.user_gains.clone();
    let started = start_call_audio(
        &mut voice_state.audio,
        gains,
        |event| {
            let _ = app.emit(AUDIO_ERROR_EVENT, event);
        },
        |audio_rx| {
            if let Some(track) = local_track {
                tokio::spawn(async move {
                    send_audio_to_track(track, audio_rx).await;
                });
            } else {
                error!("No local track available for audio sending");
            }
        },
    )
    .await;
    voice_state.audio_tx = Some(started.audio_tx);
    voice_state.audio_mixer = Some(started.mixer);
    started.result?;
    info!("Publisher answer set, audio started");
    Ok(())
}

/// The audio engine as the call start sees it (a seam for tests).
trait CallAudio {
    fn start_capture(
        &mut self,
        output_tx: mpsc::Sender<Vec<u8>>,
    ) -> impl std::future::Future<Output = Result<(), AudioError>> + Send;
    fn start_pcm_playback(
        &mut self,
        input_rx: mpsc::Receiver<Vec<f32>>,
    ) -> impl std::future::Future<Output = Result<(), AudioError>> + Send;
    /// Device name for an audio error of `kind`.
    fn device_label(&self, kind: AudioErrorKind) -> String;
}

impl CallAudio for AudioHandle {
    async fn start_capture(&mut self, output_tx: mpsc::Sender<Vec<u8>>) -> Result<(), AudioError> {
        Self::start_capture(self, output_tx).await
    }

    async fn start_pcm_playback(
        &mut self,
        input_rx: mpsc::Receiver<Vec<f32>>,
    ) -> Result<(), AudioError> {
        Self::start_pcm_playback(self, input_rx).await
    }

    fn device_label(&self, kind: AudioErrorKind) -> String {
        self.active_device_label(matches!(kind, AudioErrorKind::CaptureFailed))
    }
}

/// Result of [`start_call_audio`].
struct StartedCallAudio {
    /// Where captured Opus frames go (kept in the voice state).
    audio_tx: mpsc::Sender<Vec<u8>>,
    mixer: AudioMixer,
    /// Combined user-facing error when the microphone or speaker failed.
    result: Result<(), String>,
}

/// Start the audio of a call whose publisher answer was just applied.
///
/// A microphone or speaker that cannot start must not leave a silent
/// "Connected" call: each failure is emitted (`emit`) right away, the rest
/// of the call still starts, and the uplink sender (`spawn_sender`) is wired
/// before playback is attempted, so picking another device mid-call fixes
/// the failed side. The failures are returned together at the end.
async fn start_call_audio<A: CallAudio + Send>(
    audio: &mut A,
    gains: crate::voice::audio_mixer::UserGains,
    emit: impl Fn(&AudioErrorEvent) + Send + Sync,
    spawn_sender: impl FnOnce(mpsc::Receiver<Vec<u8>>) + Send,
) -> StartedCallAudio {
    let mut problems = Vec::new();
    let mut fail = |kind: AudioErrorKind, what: &str, e: &AudioError, audio: &A| {
        error!("{what} could not be started: {e}");
        emit(&AudioErrorEvent {
            kind,
            device: audio.device_label(kind),
        });
        problems.push(format!("{what} could not be started: {e}"));
    };

    let (audio_tx, audio_rx) = mpsc::channel::<Vec<u8>>(100);
    if let Err(e) = audio.start_capture(audio_tx.clone()).await {
        fail(AudioErrorKind::CaptureFailed, "Microphone", &e, audio);
    }
    spawn_sender(audio_rx);

    // The mixer combines all decoded remote audio tracks into one stream.
    let (pcm_output_tx, pcm_output_rx) = mpsc::channel::<Vec<f32>>(100);
    let mixer = AudioMixer::with_gains(pcm_output_tx, gains);
    if let Err(e) = audio.start_pcm_playback(pcm_output_rx).await {
        fail(AudioErrorKind::PlaybackFailed, "Speaker", &e, audio);
    }

    StartedCallAudio {
        audio_tx,
        mixer,
        result: if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        },
    }
}

/// Handle SDP offer from server for the subscriber `PeerConnection`.
///
/// Called when frontend receives `ws:voice_subscriber_offer` event.
/// Creates an answer and sends it back to the server.
#[command]
pub async fn handle_voice_subscriber_offer(
    channel_id: String,
    sdp: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    info!(channel_id = %channel_id, "Handling subscriber offer");

    let voice_guard = state.voice.read().await;
    let voice = voice_guard.as_ref().ok_or("Not in voice")?;

    if voice.channel_id.as_deref() != Some(&channel_id) {
        return Err("Wrong channel".to_string());
    }

    let answer_sdp = voice
        .webrtc
        .handle_subscriber_offer(&sdp)
        .await
        .map_err(|e| format!("Subscriber offer failed: {e}"))?;

    info!(channel_id = %channel_id, "Subscriber offer handled, answer sent");
    Ok(answer_sdp)
}

/// Handle ICE candidate from server.
///
/// Called when frontend receives `ws:voice_ice_candidate` event.
#[command]
pub async fn handle_voice_ice_candidate(
    channel_id: String,
    candidate: String,
    pc_type: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!(
        "Handling ICE candidate for channel: {} (pc_type: {})",
        channel_id, pc_type
    );

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    // Verify we're in this channel
    if voice_state.channel_id.as_ref() != Some(&channel_id) {
        return Err(format!(
            "Received ICE candidate for wrong channel: {channel_id}"
        ));
    }

    if pc_type == "subscriber" {
        voice_state
            .webrtc
            .add_subscriber_ice_candidate(&candidate)
            .await
            .map_err(|e| format!("Subscriber ICE failed: {e}"))?;
        return Ok(());
    }

    voice_state
        .webrtc
        .add_ice_candidate(&candidate)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Whether leaving sends `VoiceLeave`. A silent leave (C31 `voice_removed`,
/// C32 rejoin after a new socket) only drops the local call: the server
/// already removed this peer, and a `VoiceLeave` from this socket could
/// remove the user's peer on another device (peers are keyed by user).
fn sends_voice_leave(silent: Option<bool>) -> bool {
    !silent.unwrap_or(false)
}

/// Longest one step of a voice teardown may take. A step that does not
/// finish in time is given up (logged) and the teardown goes on with the
/// next one, so a teardown always runs to its end and never keeps the voice
/// lock for good.
const TEARDOWN_STEP_LIMIT: std::time::Duration = std::time::Duration::from_secs(3);

/// Run one teardown step, waiting at most [`TEARDOWN_STEP_LIMIT`] for it.
async fn bounded_step<T>(
    what: &'static str,
    step: impl std::future::Future<Output = T>,
) -> Option<T> {
    if let Ok(done) = tokio::time::timeout(TEARDOWN_STEP_LIMIT, step).await {
        Some(done)
    } else {
        warn!(
            step = what,
            "Voice teardown step did not finish in time; going on"
        );
        None
    }
}

/// The native voice session as a teardown sees it (a seam for tests; the
/// app uses [`VoiceState`]).
trait NativeSession: Send {
    /// Epoch of the session that holds the connection, if any.
    fn session_epoch(&self) -> Option<u64>;
    /// Release everything of the held session (microphone and speaker
    /// streams, shares, webcam, peer connections), each step bounded.
    /// Returns the channel the session was in.
    fn release(&mut self) -> impl std::future::Future<Output = Option<String>> + Send;
}

impl NativeSession for VoiceState {
    fn session_epoch(&self) -> Option<u64> {
        self.session_epoch
    }

    async fn release(&mut self) -> Option<String> {
        self.session_epoch = None;
        let channel_id = self.channel_id.take();

        // Stop all screen shares if active
        if !self.screen_shares.is_empty() {
            info!(
                count = self.screen_shares.len(),
                "Auto-stopping screen shares on voice leave"
            );
            let pipelines: Vec<_> = self.screen_shares.drain().collect();
            for (stream_id, pipeline) in pipelines {
                info!(stream_id = %stream_id, "Shutting down screen share pipeline");
                bounded_step("screen share", pipeline.shutdown()).await;
            }
        }

        // Stop webcam if active
        if let Some(pipeline) = self.webcam.take() {
            info!("Auto-stopping webcam on voice leave");
            bounded_step("webcam", pipeline.shutdown()).await;
        }

        // Stop all video decode tasks
        {
            let mut tasks = self.video_tasks.lock().await;
            if !tasks.is_empty() {
                info!(
                    count = tasks.len(),
                    "Aborting video decode tasks on voice leave"
                );
                for handle in tasks.drain(..) {
                    handle.abort();
                }
            }
            self.active_video_count.store(0, Ordering::Relaxed);
        }

        // Drop any pending frame sinks (subscriptions whose track never arrived).
        // Dropping the FrameBuffer closes its watch channel, which causes the
        // emitter task in spawn_frame_emitter to exit cleanly.
        self.video_frame_sinks.lock().await.clear();

        // Stop audio and drop mixer
        bounded_step("audio", self.audio.stop_all()).await;
        self.audio_tx = None;
        self.audio_mixer = None;
        // Disconnect WebRTC. A failed or slow disconnect must not keep the
        // rest of the session alive.
        if let Some(Err(e)) = bounded_step("peer connections", self.webrtc.disconnect()).await {
            warn!("WebRTC disconnect on leave failed: {e}");
        }

        // Clear receive stats AFTER disconnect: decode tasks publish until their
        // tracks close, so clearing first would let a final publish resurrect
        // stale entries into the next session.
        self.connection_stats.clear();

        channel_id
    }
}

/// Tear down the native session `epoch` names. Everything happens under the
/// voice lock, which joins take too: a join of a newer session waits until
/// this teardown is through and then finds a clean state, and a teardown
/// that names an older session than the held one changes nothing
/// ([`leave_applies`]). Because every step is bounded, the lock is always
/// released again.
async fn teardown_session<S: NativeSession + Sync, W: VoiceSignal + Send + Sync>(
    voice: &RwLock<Option<S>>,
    ws: &RwLock<Option<W>>,
    silent: Option<bool>,
    epoch: Option<u64>,
) -> Result<(), String> {
    let mut voice = voice.write().await;
    let session = voice.as_mut().ok_or("Voice not initialized")?;

    if !leave_applies(epoch, session.session_epoch()) {
        info!(
            epoch = ?epoch,
            held = ?session.session_epoch(),
            "Leave names an older voice session; the newer one stays"
        );
        return Ok(());
    }

    let channel_id = session.release().await;

    // Tell the server while the lock is still held, so the leave goes out
    // before the `VoiceJoin` of a join that waits for the lock.
    if let Some(channel_id) = channel_id.filter(|_| sends_voice_leave(silent)) {
        let ws = ws.read().await;
        if let Some(sender) = ws.as_ref() {
            let _ = sender
                .send_event(ClientEvent::VoiceLeave { channel_id })
                .await;
        }
    }

    info!("Left voice channel");
    Ok(())
}

/// Leave a voice session. `silent: true` skips `VoiceLeave`. `epoch` names
/// the session to end (from [`begin_voice_session`]): its join is cancelled
/// at once, and the native session is torn down unless a newer one holds
/// the connection by now ([`leave_applies`]). Without `epoch` whatever is
/// there ends.
///
/// The teardown runs as a task of its own: it runs to its end whether or
/// not the window that asked for it still waits (or still exists), and it
/// is ordered with later joins by the voice lock ([`teardown_session`]).
#[command]
pub async fn leave_voice(
    silent: Option<bool>,
    epoch: Option<u64>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(silent = ?silent, epoch = ?epoch, "Leaving voice channel");
    // Before waiting for the lock: a join of the ended session stops at its
    // next check instead of connecting after this leave.
    match epoch {
        Some(epoch) => state.voice_gate.cancel_through(epoch),
        None => state.voice_gate.cancel_all(),
    }

    let voice = state.voice.clone();
    let websocket = state.websocket.clone();
    tauri::async_runtime::spawn(
        async move { teardown_session(&voice, &websocket, silent, epoch).await },
    )
    .await
    .map_err(|e| format!("Voice teardown did not complete: {e}"))?
}

/// Whether a publisher connection in this state carries media.
const fn publisher_alive(
    state: webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState,
) -> bool {
    matches!(
        state,
        webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState::Connected
    )
}

/// Whether the publisher connection of the voice session `epoch` is alive
/// right now. Read from the peer connection itself, not from the last
/// state event: after the computer slept, events may not have arrived yet.
/// False when another session (or none) holds the connection.
#[command]
pub async fn voice_probe(epoch: Option<u64>, state: State<'_, AppState>) -> Result<bool, String> {
    let publisher = {
        let voice = state.voice.read().await;
        let Some(voice_state) = voice.as_ref() else {
            return Ok(false);
        };
        if voice_state.channel_id.is_none() || voice_state.session_epoch != epoch {
            return Ok(false);
        }
        voice_state.webrtc.publisher_pc().await
    };
    Ok(publisher.is_some_and(|pc| publisher_alive(pc.connection_state())))
}

/// Set mute state.
#[command]
pub async fn set_mute(muted: bool, state: State<'_, AppState>) -> Result<(), String> {
    debug!("Setting mute: {}", muted);

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state.audio.set_muted(muted);

    // Notify server
    if let Some(channel_id) = &voice_state.channel_id {
        let ws = state.websocket.read().await;
        if let Some(ws_manager) = ws.as_ref() {
            let event = if muted {
                ClientEvent::VoiceMute {
                    channel_id: channel_id.clone(),
                }
            } else {
                ClientEvent::VoiceUnmute {
                    channel_id: channel_id.clone(),
                }
            };
            let _ = ws_manager.send(event).await;
        }
    }

    Ok(())
}

/// Set deafen state.
#[command]
pub async fn set_deafen(deafened: bool, state: State<'_, AppState>) -> Result<(), String> {
    debug!("Setting deafen: {}", deafened);

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state.audio.set_deafened(deafened);

    Ok(())
}

/// Set VAD configuration (enabled + threshold).
#[command]
pub async fn set_vad_config(
    enabled: bool,
    threshold: f32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!(
        "Setting VAD config: enabled={}, threshold={:.2}",
        enabled, threshold
    );

    state.ensure_voice().await?;
    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state.audio.set_vad_config(enabled, threshold);

    Ok(())
}

/// Set noise suppression (uses `RNNoise` denoised output when enabled).
#[command]
pub async fn set_noise_suppression(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!("Setting noise suppression: {}", enabled);

    state.ensure_voice().await?;
    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state.audio.set_noise_suppression(enabled);

    Ok(())
}

/// Start microphone test (local only, no server connection).
#[command]
pub async fn start_mic_test(
    device_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Starting mic test");

    state.ensure_voice().await?;

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    voice_state
        .audio
        .start_mic_test(device_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Stop microphone test.
#[command]
pub async fn stop_mic_test(state: State<'_, AppState>) -> Result<(), String> {
    info!("Stopping mic test");

    let mut voice = state.voice.write().await;
    if let Some(voice_state) = voice.as_mut() {
        voice_state.audio.stop_mic_test().await;
    }

    Ok(())
}

/// Get current microphone test level (0-100).
#[command]
pub async fn get_mic_level(state: State<'_, AppState>) -> Result<u8, String> {
    let voice = state.voice.read().await;
    if let Some(voice_state) = voice.as_ref() {
        Ok(voice_state.audio.get_mic_test_level())
    } else {
        Ok(0)
    }
}

/// Get list of available audio devices.
#[command]
pub async fn get_audio_devices(state: State<'_, AppState>) -> Result<AudioDeviceList, String> {
    state.ensure_voice().await?;

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    voice_state
        .audio
        .enumerate_devices()
        .map_err(|e| e.to_string())
}

/// Set input (microphone) device.
///
/// During a call the microphone switches immediately; if the new device
/// cannot be opened the previous one keeps running and an error is returned.
#[command]
pub async fn set_input_device(
    device_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Setting input device: {:?}", device_id);

    state.ensure_voice().await?;

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    voice_state
        .audio
        .set_input_device(device_id)
        .await
        .map_err(|e| e.to_string())
}

/// Set how loud one person plays for this user (`volume`: 0.0 = muted,
/// 1.0 = unchanged, up to 2.0). Applies to all of their audio immediately
/// and to later calls in this session.
#[command]
pub async fn set_user_volume(
    user_id: String,
    volume: f32,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let user_id = uuid::Uuid::parse_str(&user_id).map_err(|e| format!("Invalid user id: {e}"))?;
    debug!(%user_id, volume, "Setting user volume");

    state.ensure_voice().await?;
    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;
    // Shared with the running mixer (if any), so this takes effect mid-call.
    voice_state.user_gains.set(user_id, volume);
    Ok(())
}

/// Set output (speaker) device.
#[command]
pub async fn set_output_device(
    device_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Setting output device: {:?}", device_id);

    state.ensure_voice().await?;

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    voice_state
        .audio
        .set_output_device(device_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Check if currently in a voice channel.
#[command]
pub async fn is_in_voice(state: State<'_, AppState>) -> Result<bool, String> {
    let voice = state.voice.read().await;
    if let Some(voice_state) = voice.as_ref() {
        Ok(voice_state.channel_id.is_some())
    } else {
        Ok(false)
    }
}

/// Native connection quality stats, serialized to the frontend adapter.
///
/// Wire format (`snake_case` per project convention). The TS layer converts
/// this into the shared `ConnectionMetrics` shape, computing interval loss
/// from the cumulative counters (delta between polls, like the browser
/// adapter does with WebRTC's inbound-rtp counters) and the quality tier
/// with the same thresholds. `null` fields mean "not measured yet" — the
/// frontend maps a fully unmeasured payload to the "unknown" state instead
/// of fabricating a confident 0 ms / good reading.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ConnectionStatsPayload {
    /// Round-trip time in milliseconds (RTCP receiver reports), if measured.
    pub rtt_ms: Option<f64>,
    /// Worst RFC 3550 receive jitter across inbound audio tracks, in ms.
    pub jitter_ms: Option<f64>,
    /// Cumulative receive-side counters summed across inbound audio tracks.
    pub packets_lost: u64,
    pub packets_received: u64,
}

/// Fetch native WebRTC connection metrics (TD-16).
///
/// Returns `Ok(None)` when not connected to voice, so the frontend can keep
/// showing the "unknown" quality state instead of treating it as an error.
#[command]
pub async fn voice_connection_stats(
    state: State<'_, AppState>,
) -> Result<Option<ConnectionStatsPayload>, String> {
    // Clone the handles out and drop the voice guard BEFORE awaiting
    // get_stats(): it walks every transceiver and can be slow on a degraded
    // connection, and holding the read guard would queue join/leave/mute
    // (write-lock takers) behind every 3 s stats poll.
    let (publisher_pc, registry) = {
        let voice = state.voice.read().await;
        let Some(voice_state) = voice.as_ref() else {
            return Ok(None);
        };
        if voice_state.channel_id.is_none() {
            return Ok(None);
        }
        (
            voice_state.webrtc.publisher_pc().await,
            voice_state.connection_stats.clone(),
        )
    };

    let rtt_ms = match publisher_pc {
        Some(pc) => crate::webrtc::WebRtcClient::extract_publisher_rtt_ms(&pc.get_stats().await),
        None => return Ok(None),
    };
    let receive = registry.aggregate();

    Ok(Some(ConnectionStatsPayload {
        rtt_ms,
        jitter_ms: receive.map(|r| r.max_jitter_ms),
        packets_lost: receive.map_or(0, |r| r.packets_lost),
        packets_received: receive.map_or(0, |r| r.packets_received),
    }))
}

/// Get current voice channel ID.
#[command]
pub async fn get_voice_channel(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let voice = state.voice.read().await;
    if let Some(voice_state) = voice.as_ref() {
        Ok(voice_state.channel_id.clone())
    } else {
        Ok(None)
    }
}

/// Subscribe to decoded video frames for a given stream.
///
/// Called by the frontend (Solid.js) when mounting a screen-share / webcam
/// tile. Registers a `FrameBuffer` under `stream_id` in `VoiceState` and
/// spawns an emitter task that forwards decoded frames to the provided
/// Tauri `Channel<DecodedFrame>`.
///
/// The video decode task (see `spawn_video_decode_task`) looks up the
/// matching `FrameBuffer` by `stream_id` when the remote track arrives and
/// hands it to a `Vp8VideoDecoder` instance. If the track has already
/// arrived without a sink, the decode task drains packets until either a
/// sink is registered or the lookup deadline passes.
#[command]
pub async fn subscribe_video_frames(
    stream_id: String,
    on_frame: tauri::ipc::Channel<crate::voice::video_decoder::DecodedFrame>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use crate::voice::frame_buffer::{spawn_frame_emitter, FrameBuffer};

    let (frame_buffer, frame_rx) = FrameBuffer::new();
    spawn_frame_emitter(on_frame, frame_rx);

    let voice_guard = state.voice.read().await;
    let voice = voice_guard
        .as_ref()
        .ok_or_else(|| "Voice state not initialized".to_string())?;
    voice
        .video_frame_sinks
        .lock()
        .await
        .insert(stream_id.clone(), frame_buffer);
    info!(stream_id = %stream_id, "Registered video frame subscription");
    Ok(())
}

/// Send captured audio to WebRTC track with RTP packetization.
///
/// This function runs in a background task and:
/// 1. Receives Opus-encoded audio frames from the capture task
/// 2. Creates RTP packets with proper headers (sequence number, timestamp, SSRC)
/// 3. Writes the RTP packets to the WebRTC track for transmission
async fn send_audio_to_track(
    track: Arc<TrackLocalStaticRTP>,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    // Calculate samples per frame (20ms at 48kHz)
    const SAMPLES_PER_FRAME: u32 = SAMPLE_RATE / 1000 * FRAME_SIZE_MS as u32;

    // RTP payload type for Opus (as configured in webrtc/mod.rs)
    const OPUS_PAYLOAD_TYPE: u8 = 111;

    // Per RFC 3550 §5.1: start seq and timestamp at random values for each session.
    // SSRC is already per-session (derived from SystemTime at spawn), so combining
    // with a random seq start ensures receivers see a fresh stream after reconnect.
    let mut seq: u16 = rand::random();
    let mut timestamp: u32 = rand::random();

    // Generate SSRC (Synchronization Source identifier) from timestamp
    // This provides a unique value per stream session
    let ssrc: u32 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32;

    info!("Starting RTP audio sender task (SSRC: {})", ssrc);

    while let Some(opus_data) = audio_rx.recv().await {
        // Create RTP packet
        let rtp_packet = RtpPacket {
            header: webrtc::rtp::header::Header {
                version: 2,
                padding: false,
                extension: false,
                marker: false,
                payload_type: OPUS_PAYLOAD_TYPE,
                sequence_number: seq,
                timestamp,
                ssrc,
                ..Default::default()
            },
            payload: opus_data.into(),
        };

        // Write packet to track
        if let Err(e) = track.write_rtp(&rtp_packet).await {
            warn!("Failed to write RTP packet: {}", e);
            // Don't break - connection might recover
        }

        // Advance state for next packet. The first packet uses the random
        // initial values per RFC 3550 §5.1.
        seq = seq.wrapping_add(1);
        timestamp = timestamp.wrapping_add(SAMPLES_PER_FRAME);
    }

    info!("RTP audio sender task ended");
}

/// How many RTP packets between stats publishes — at the 20 ms Opus frame
/// cadence this is roughly once per second, while the frontend polls every
/// 3 s. Estimators update on every packet; only the shared-registry write
/// (mutex + insert) is throttled.
const STATS_PUBLISH_INTERVAL_PACKETS: u64 = 50;

/// Decode a remote audio track from RTP/Opus to PCM and send to the mixer.
///
/// Runs in a spawned task per remote audio track. Each task owns its own
/// Opus decoder instance (`opus::Decoder` is `Send` but not `Sync`).
///
/// Also measures per-track receive jitter (RFC 3550, from RTP timestamps)
/// and packet loss (expected-vs-received, from RTP sequence numbers):
/// webrtc-rs does not expose either in `get_stats()`, and this loop is
/// where every inbound audio RTP packet already passes through.
///
/// Known limitation: arrival times are sampled after `read_rtp()` returns,
/// so if the mixer backpressures `pcm_tx.send()`, buffered packets read in
/// a burst inflate the jitter estimate — local stall shows up as network
/// jitter. Acceptable: both indicate degraded audio.
async fn decode_remote_audio_track(
    track: Arc<TrackRemote>,
    pcm_tx: mpsc::Sender<Vec<f32>>,
    track_id: &str,
    stats_registry: &voice::connection_stats::ConnectionStatsRegistry,
) {
    let mut decoder = match opus::Decoder::new(SAMPLE_RATE, opus::Channels::Stereo) {
        Ok(dec) => dec,
        Err(e) => {
            error!("Failed to create Opus decoder for remote track: {}", e);
            return;
        }
    };

    // Output buffer: one stereo frame at 48 kHz / 20 ms = 960 samples * 2 channels
    let frame_samples = FRAME_SIZE * CHANNELS as usize;
    let mut pcm_f32 = vec![0.0f32; frame_samples];

    let mut jitter = voice::connection_stats::JitterEstimator::new();
    let mut loss = voice::connection_stats::LossTracker::new();
    let mut packets_since_publish: u64 = 0;

    loop {
        match track.read_rtp().await {
            Ok((rtp_packet, _attributes)) => {
                jitter.on_packet(rtp_packet.header.timestamp, std::time::Instant::now());
                loss.on_packet(rtp_packet.header.sequence_number);
                packets_since_publish += 1;
                if packets_since_publish >= STATS_PUBLISH_INTERVAL_PACKETS {
                    packets_since_publish = 0;
                    stats_registry.publish(
                        track_id,
                        voice::connection_stats::TrackSnapshot {
                            jitter_ms: jitter.jitter_ms(),
                            packets_lost: loss.packets_lost(),
                            packets_received: loss.packets_received(),
                        },
                    );
                }
                let payload = &rtp_packet.payload[..];
                if payload.is_empty() {
                    continue;
                }

                match decoder.decode_float(payload, &mut pcm_f32, false) {
                    Ok(samples_per_channel) => {
                        let total_samples = samples_per_channel * CHANNELS as usize;
                        let frame = pcm_f32[..total_samples].to_vec();
                        if pcm_tx.send(frame).await.is_err() {
                            debug!("Mixer channel closed, stopping remote audio decode");
                            break;
                        }
                    }
                    Err(e) => {
                        warn!("Opus decode error on remote track: {}", e);
                    }
                }
            }
            Err(e) => {
                info!("Remote audio track read ended: {}", e);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::audio::AudioError;
    use crate::voice::audio_mixer::UserGains;

    /// Scripted audio engine for the call-start order.
    struct FakeCallAudio {
        capture_ok: bool,
        playback_ok: bool,
        log: Vec<&'static str>,
    }

    impl CallAudio for FakeCallAudio {
        async fn start_capture(&mut self, _tx: mpsc::Sender<Vec<u8>>) -> Result<(), AudioError> {
            self.log.push("capture");
            if self.capture_ok {
                Ok(())
            } else {
                Err(AudioError::StreamError("mic gone".into()))
            }
        }

        async fn start_pcm_playback(
            &mut self,
            _rx: mpsc::Receiver<Vec<f32>>,
        ) -> Result<(), AudioError> {
            self.log.push("playback");
            if self.playback_ok {
                Ok(())
            } else {
                Err(AudioError::DeviceNotFound("Headset".into()))
            }
        }

        fn device_label(&self, kind: AudioErrorKind) -> String {
            match kind {
                AudioErrorKind::CaptureFailed => "USB Mic".into(),
                AudioErrorKind::PlaybackFailed => "Headset".into(),
            }
        }
    }

    async fn start(
        capture_ok: bool,
        playback_ok: bool,
    ) -> (
        FakeCallAudio,
        StartedCallAudio,
        Vec<AudioErrorEvent>,
        Option<mpsc::Receiver<Vec<u8>>>,
    ) {
        let mut audio = FakeCallAudio {
            capture_ok,
            playback_ok,
            log: Vec::new(),
        };
        let events = std::sync::Mutex::new(Vec::new());
        let mut sender_rx = None;
        let started = start_call_audio(
            &mut audio,
            UserGains::default(),
            |event| events.lock().unwrap().push(event.clone()),
            |rx| {
                sender_rx = Some(rx);
            },
        )
        .await;
        (audio, started, events.into_inner().unwrap(), sender_rx)
    }

    #[tokio::test]
    async fn sender_is_wired_even_when_playback_fails() {
        // Saved headset unplugged between calls: the speaker cannot start.
        let (audio, started, events, sender_rx) = start(true, false).await;

        assert_eq!(audio.log, ["capture", "playback"]);
        let mut sender_rx = sender_rx.expect("uplink sender wired");
        // The microphone's frames still reach the WebRTC sender.
        started.audio_tx.try_send(vec![7]).unwrap();
        assert_eq!(sender_rx.try_recv().unwrap(), vec![7]);

        assert_eq!(
            events,
            vec![AudioErrorEvent {
                kind: AudioErrorKind::PlaybackFailed,
                device: "Headset".into(),
            }]
        );
        let err = started.result.unwrap_err();
        assert!(err.contains("Speaker"), "{err}");
    }

    #[tokio::test]
    async fn capture_and_playback_failures_are_both_reported() {
        let (audio, started, events, sender_rx) = start(false, false).await;
        assert_eq!(audio.log, ["capture", "playback"]);
        assert!(sender_rx.is_some(), "a mic chosen later still has a sender");
        let kinds: Vec<_> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                AudioErrorKind::CaptureFailed,
                AudioErrorKind::PlaybackFailed
            ]
        );
        let err = started.result.unwrap_err();
        assert!(
            err.contains("Microphone") && err.contains("Speaker"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn healthy_call_audio_reports_nothing() {
        let (_, started, events, sender_rx) = start(true, true).await;
        assert!(sender_rx.is_some());
        assert!(events.is_empty());
        assert!(started.result.is_ok());
    }

    #[test]
    fn join_is_cancelled_by_a_leave() {
        let gate = VoiceEpochGate::default();
        let epoch = gate.begin_session();
        assert!(gate.check(Some(epoch)).is_ok());
        gate.cancel_all();
        assert_eq!(gate.check(Some(epoch)), Err(JOIN_CANCELLED.to_string()));
        // The next session's join goes on.
        let next = gate.begin_session();
        assert!(gate.check(Some(next)).is_ok());
    }

    #[test]
    fn a_new_session_alone_cancels_nothing() {
        let gate = VoiceEpochGate::default();
        let a = gate.begin_session();
        let b = gate.begin_session();
        assert!(b > a);
        // Allocation is not a leave: a join that got its epoch earlier goes
        // on, whatever order the allocations were answered in.
        assert!(gate.is_current(Some(a)));
        assert!(gate.is_current(Some(b)));
    }

    #[test]
    fn a_leave_ends_its_session_and_older_ones_only() {
        let gate = VoiceEpochGate::default();
        let old = gate.begin_session();
        let left = gate.begin_session();
        let newer = gate.begin_session();
        gate.cancel_through(left);
        assert!(!gate.is_current(Some(old)));
        assert!(!gate.is_current(Some(left)));
        assert!(gate.is_current(Some(newer)));
        // A late leave of an older session changes nothing any more.
        gate.cancel_through(old);
        assert!(gate.is_current(Some(newer)));
    }

    #[test]
    fn a_leave_never_tears_down_a_newer_native_session() {
        // The leave of session 3 arrives after session 5 was established.
        assert!(!leave_applies(Some(3), Some(5)));
        // Its own session, or an older one that was left behind, ends.
        assert!(leave_applies(Some(3), Some(3)));
        assert!(leave_applies(Some(3), Some(2)));
        // Callers or sessions without an epoch keep the old behaviour.
        assert!(leave_applies(None, Some(5)));
        assert!(leave_applies(Some(3), None));
        assert!(leave_applies(None, None));
    }

    #[test]
    fn reloaded_window_continues_the_native_order() {
        // Window before reload: many sessions, the last one left.
        let gate = VoiceEpochGate::default();
        let mut last = 0;
        for _ in 0..50 {
            last = gate.begin_session();
        }
        gate.cancel_through(last);
        // The reloaded window knows nothing of the old epochs; it asks the
        // same process and gets one above the floor.
        let after_reload = gate.begin_session();
        assert!(gate.is_current(Some(after_reload)));
    }

    #[test]
    fn two_windows_share_one_order() {
        let gate = VoiceEpochGate::default();
        let window_a = gate.begin_session();
        let window_b = gate.begin_session();
        // A's leave ends A's session; B, started later, goes on.
        gate.cancel_through(window_a);
        assert!(!gate.is_current(Some(window_a)));
        assert!(gate.is_current(Some(window_b)));
        // A leave without an epoch (older caller) ends both.
        gate.cancel_all();
        assert!(!gate.is_current(Some(window_b)));
    }

    #[test]
    fn joins_without_epoch_are_never_cancelled() {
        let gate = VoiceEpochGate::default();
        gate.begin_session();
        gate.cancel_all();
        assert!(gate.check(None).is_ok());
    }

    /// Records what a join sends.
    #[derive(Default)]
    struct FakeSignal {
        sent: std::sync::Mutex<Vec<String>>,
    }

    impl VoiceSignal for FakeSignal {
        async fn send_event(&self, event: ClientEvent) -> Result<(), String> {
            let name = match event {
                ClientEvent::VoiceJoin { .. } => "join",
                ClientEvent::VoicePublisherOffer { .. } => "offer",
                ClientEvent::VoiceLeave { .. } => "leave",
                _ => "other",
            };
            self.sent.lock().unwrap().push(name.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn removal_while_join_waits_for_the_socket_lock_sends_nothing() {
        let ws = Arc::new(RwLock::new(Some(FakeSignal::default())));
        let gate = Arc::new(VoiceEpochGate::default());
        let epoch = gate.begin_session();

        // Park the join on the socket lock.
        let held = ws.write().await;
        let join = tokio::spawn({
            let ws = ws.clone();
            let gate = gate.clone();
            async move { send_join_and_offer(&ws, &gate, Some(epoch), "ch", "sdp".into()).await }
        });
        tokio::task::yield_now().await;
        // voice_removed / leave arrives meanwhile.
        gate.cancel_all();
        drop(held);

        assert_eq!(join.await.unwrap(), Err(JoinSendError::CancelledBeforeJoin));
        let sent = ws
            .read()
            .await
            .as_ref()
            .unwrap()
            .sent
            .lock()
            .unwrap()
            .clone();
        assert!(sent.is_empty(), "sent {sent:?}");
    }

    #[tokio::test]
    async fn current_join_sends_join_then_offer() {
        let ws = RwLock::new(Some(FakeSignal::default()));
        let gate = VoiceEpochGate::default();
        let epoch = gate.begin_session();
        assert_eq!(
            send_join_and_offer(&ws, &gate, Some(epoch), "ch", "sdp".into()).await,
            Ok(())
        );
        let sent = ws
            .read()
            .await
            .as_ref()
            .unwrap()
            .sent
            .lock()
            .unwrap()
            .clone();
        assert_eq!(sent, vec!["join", "offer"]);
    }

    /// Cancels the session while `VoiceJoin` is being sent.
    struct CancelOnJoin {
        gate: Arc<VoiceEpochGate>,
        inner: FakeSignal,
    }

    impl VoiceSignal for CancelOnJoin {
        async fn send_event(&self, event: ClientEvent) -> Result<(), String> {
            if matches!(event, ClientEvent::VoiceJoin { .. }) {
                self.gate.cancel_all();
            }
            self.inner.send_event(event).await
        }
    }

    #[tokio::test]
    async fn cancel_during_voice_join_sends_leave_instead_of_offer() {
        let gate = Arc::new(VoiceEpochGate::default());
        let epoch = gate.begin_session();
        let ws = RwLock::new(Some(CancelOnJoin {
            gate: gate.clone(),
            inner: FakeSignal::default(),
        }));
        assert_eq!(
            send_join_and_offer(&ws, &gate, Some(epoch), "ch", "sdp".into()).await,
            Err(JoinSendError::CancelledAfterJoin)
        );
        let sent = ws
            .read()
            .await
            .as_ref()
            .unwrap()
            .inner
            .sent
            .lock()
            .unwrap()
            .clone();
        assert_eq!(sent, vec!["join", "leave"]);
    }

    /// A native session for the teardown tests.
    struct FakeSession {
        epoch: Option<u64>,
        channel: Option<String>,
        /// How long releasing takes.
        release_takes: std::time::Duration,
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl NativeSession for FakeSession {
        fn session_epoch(&self) -> Option<u64> {
            self.epoch
        }

        async fn release(&mut self) -> Option<String> {
            let epoch = self.epoch.take();
            self.log
                .lock()
                .unwrap()
                .push(format!("release {epoch:?} starts"));
            bounded_step("fake", tokio::time::sleep(self.release_takes)).await;
            self.log
                .lock()
                .unwrap()
                .push(format!("release {epoch:?} done"));
            self.channel.take()
        }
    }

    fn fake_session(
        epoch: u64,
        release_takes_secs: u64,
        log: &Arc<std::sync::Mutex<Vec<String>>>,
    ) -> FakeSession {
        FakeSession {
            epoch: Some(epoch),
            channel: Some(format!("ch-{epoch}")),
            release_takes: std::time::Duration::from_secs(release_takes_secs),
            log: log.clone(),
        }
    }

    fn sent_by(ws: &RwLock<Option<FakeSignal>>) -> Vec<String> {
        ws.try_read()
            .unwrap()
            .as_ref()
            .unwrap()
            .sent
            .lock()
            .unwrap()
            .clone()
    }

    #[tokio::test(start_paused = true)]
    async fn a_teardown_step_that_hangs_is_given_up_after_the_limit() {
        let started = tokio::time::Instant::now();
        assert_eq!(
            bounded_step("hangs", std::future::pending::<u8>()).await,
            None
        );
        assert_eq!(started.elapsed(), TEARDOWN_STEP_LIMIT);
        assert_eq!(bounded_step("quick", async { 7 }).await, Some(7));
    }

    #[tokio::test(start_paused = true)]
    async fn a_teardown_with_a_hanging_step_still_completes_and_frees_the_lock() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        // Releasing would take an hour: far beyond the limit of one step.
        let voice = Arc::new(RwLock::new(Some(fake_session(3, 3600, &log))));
        let ws = RwLock::new(Some(FakeSignal::default()));

        assert_eq!(
            teardown_session(&voice, &ws, Some(false), Some(3)).await,
            Ok(())
        );
        // The session is gone, the server was told, the lock is free again.
        assert!(voice.try_write().is_ok());
        assert_eq!(voice.read().await.as_ref().unwrap().epoch, None);
        assert_eq!(sent_by(&ws), vec!["leave"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_newer_join_waits_for_the_background_teardown_and_is_not_touched() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let voice = Arc::new(RwLock::new(Some(fake_session(3, 2, &log))));
        let ws = Arc::new(RwLock::new(Some(FakeSignal::default())));

        // The teardown of session 3 runs on its own; nobody waits for it.
        let teardown = tokio::spawn({
            let voice = voice.clone();
            let ws = ws.clone();
            async move { teardown_session(&voice, &ws, Some(false), Some(3)).await }
        });
        tokio::task::yield_now().await;

        // Meanwhile session 5 joins: it takes the same lock.
        let join = tokio::spawn({
            let voice = voice.clone();
            let log = log.clone();
            async move {
                let mut held = voice.write().await;
                log.lock().unwrap().push("join 5".into());
                *held = Some(fake_session(5, 0, &log));
            }
        });
        join.await.unwrap();
        assert_eq!(teardown.await.unwrap(), Ok(()));

        // Session 3 was released completely before session 5 was created.
        assert_eq!(
            *log.lock().unwrap(),
            vec!["release Some(3) starts", "release Some(3) done", "join 5"]
        );
        // A late repeat of the old leave does not touch session 5.
        assert_eq!(
            teardown_session(&voice, &ws, Some(false), Some(3)).await,
            Ok(())
        );
        let held = voice.read().await;
        assert_eq!(held.as_ref().unwrap().epoch, Some(5));
        assert_eq!(held.as_ref().unwrap().channel.as_deref(), Some("ch-5"));
        assert_eq!(log.lock().unwrap().len(), 3);
        assert_eq!(sent_by(&ws), vec!["leave"]);
    }

    #[tokio::test]
    async fn a_silent_teardown_tells_the_server_nothing() {
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let voice = RwLock::new(Some(fake_session(4, 0, &log)));
        let ws = RwLock::new(Some(FakeSignal::default()));
        assert_eq!(
            teardown_session(&voice, &ws, Some(true), Some(4)).await,
            Ok(())
        );
        assert_eq!(voice.read().await.as_ref().unwrap().epoch, None);
        assert!(sent_by(&ws).is_empty());
    }

    #[test]
    fn only_a_connected_publisher_counts_as_alive() {
        use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState as S;
        assert!(publisher_alive(S::Connected));
        for state in [
            S::New,
            S::Connecting,
            S::Disconnected,
            S::Failed,
            S::Closed,
            S::Unspecified,
        ] {
            assert!(!publisher_alive(state), "{state:?}");
        }
    }

    #[test]
    fn leave_sends_voice_leave_unless_silent() {
        assert!(sends_voice_leave(None));
        assert!(sends_voice_leave(Some(false)));
        assert!(!sends_voice_leave(Some(true)));
    }

    #[tokio::test]
    async fn ice_servers_parsed_from_server() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/voice/ice-servers"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ice_servers": [
                    { "urls": ["stun:stun.example.org:3478"] },
                    {
                        "urls": ["turn:turn.example.org:3478"],
                        "username": "u",
                        "credential": "c"
                    }
                ]
            })))
            .mount(&server)
            .await;

        let servers = fetch_ice_servers(&reqwest::Client::new(), &server.uri(), "tok").await;
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].urls, vec!["stun:stun.example.org:3478"]);
        assert!(servers[0].username.is_none());
        assert_eq!(servers[1].username.as_deref(), Some("u"));
        assert_eq!(servers[1].credential.as_deref(), Some("c"));
    }

    #[tokio::test]
    async fn ice_servers_fall_back_to_default_on_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/voice/ice-servers"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let servers = fetch_ice_servers(&reqwest::Client::new(), &server.uri(), "tok").await;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].urls, IceServerConfig::default().urls);
    }

    #[tokio::test]
    async fn ice_servers_fall_back_on_empty_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/voice/ice-servers"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ice_servers": [] })),
            )
            .mount(&server)
            .await;

        let servers = fetch_ice_servers(&reqwest::Client::new(), &server.uri(), "tok").await;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].urls, IceServerConfig::default().urls);
    }

    #[tokio::test]
    async fn ice_servers_drop_entries_with_empty_urls() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/voice/ice-servers"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "ice_servers": [{ "urls": [] }] })),
            )
            .mount(&server)
            .await;

        let servers = fetch_ice_servers(&reqwest::Client::new(), &server.uri(), "tok").await;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].urls, IceServerConfig::default().urls);
    }

    #[tokio::test]
    async fn ice_servers_fall_back_on_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/voice/ice-servers"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(ICE_SERVERS_TIMEOUT + std::time::Duration::from_secs(1))
                    .set_body_json(serde_json::json!({
                        "ice_servers": [{ "urls": ["stun:slow.example:3478"] }]
                    })),
            )
            .mount(&server)
            .await;

        let servers = fetch_ice_servers(&reqwest::Client::new(), &server.uri(), "tok").await;
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].urls, IceServerConfig::default().urls);
    }
}
