//! Voice Commands
//!
//! Tauri commands for voice chat functionality.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tauri::{command, AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use vc_common::protocol::PcType;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use webrtc::track::track_remote::TrackRemote;

use crate::audio::{AudioDeviceList, CHANNELS, FRAME_SIZE, FRAME_SIZE_MS, SAMPLE_RATE};
use crate::network::ClientEvent;
use crate::voice::audio_mixer::AudioMixer;
use crate::webrtc::IceServerConfig;
use crate::{voice, AppState};

/// Join a voice channel.
///
/// Initializes audio pipeline and WebRTC, sends `VoiceJoin` to server.
/// After joining, the client creates a publisher offer and sends it via
/// `VoicePublisherOffer`. The server responds with `VoicePublisherAnswer`.
#[command]
pub async fn join_voice(
    channel_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    info!("Joining voice channel: {}", channel_id);

    // Initialize voice state if needed
    state.ensure_voice().await?;

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    // Set app handle for speaking indicator events
    voice_state.audio.set_app_handle(app.clone());

    // Check if already in a channel
    if voice_state.channel_id.is_some() {
        return Err("Already in a voice channel. Leave first.".into());
    }

    // Default ICE servers (can be configured from server later)
    let ice_servers = vec![IceServerConfig::default()];

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
            let _ = app_clone.emit("voice:state_change", format!("{new_state:?}"));
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
                let track = track.clone();
                let app = app_clone.clone();
                let voice_arc = voice_arc.clone();
                let track_id = format!("{}:{}", user_id, source_type);

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
                    let drain_track = track.clone();
                    tokio::spawn(async move {
                        let mut buf = vec![0u8; 4096];
                        loop {
                            match drain_track.read(&mut buf).await {
                                Ok(_) => {} // discard
                                Err(_) => break,
                            }
                        }
                    });

                    return;
                }

                // Count already incremented by fetch_update — proceed with decode.
                let handle = voice::video_decoder::spawn_video_decode_task(
                    track.clone(),
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

    // Send VoiceJoin to server via WebSocket
    let ws = state.websocket.read().await;
    if let Some(ws_manager) = ws.as_ref() {
        ws_manager
            .send(ClientEvent::VoiceJoin {
                channel_id: channel_id.clone(),
            })
            .await
            .map_err(|e| format!("Failed to send VoiceJoin: {e}"))?;

        // Send publisher offer
        ws_manager
            .send(ClientEvent::VoicePublisherOffer {
                channel_id,
                sdp: offer_sdp,
            })
            .await
            .map_err(|e| format!("Failed to send VoicePublisherOffer: {e}"))?;
    } else {
        return Err("WebSocket not connected".into());
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

    // Start audio capture
    let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    voice_state
        .audio
        .start_capture(audio_tx.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Create audio mixer and start PCM playback from its output.
    // The mixer combines all decoded remote audio tracks into one stream.
    let (pcm_output_tx, pcm_output_rx) = tokio::sync::mpsc::channel::<Vec<f32>>(100);
    let mixer = AudioMixer::new(pcm_output_tx);
    voice_state.audio_mixer = Some(mixer);

    voice_state
        .audio
        .start_pcm_playback(pcm_output_rx)
        .await
        .map_err(|e| e.to_string())?;

    voice_state.audio_tx = Some(audio_tx);

    // Spawn task to send captured audio to WebRTC track
    let local_track = voice_state.webrtc.get_local_track().await;
    if let Some(track) = local_track {
        tokio::spawn(async move {
            send_audio_to_track(track, audio_rx).await;
        });
    } else {
        error!("No local track available for audio sending");
    }

    info!("Publisher answer set, audio started");
    Ok(())
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

/// Leave the current voice channel.
#[command]
pub async fn leave_voice(state: State<'_, AppState>) -> Result<(), String> {
    info!("Leaving voice channel");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    let channel_id = voice_state.channel_id.take();

    // Stop all screen shares if active
    if !voice_state.screen_shares.is_empty() {
        info!(
            count = voice_state.screen_shares.len(),
            "Auto-stopping screen shares on voice leave"
        );
        let pipelines: Vec<_> = voice_state.screen_shares.drain().collect();
        for (stream_id, pipeline) in pipelines {
            info!(stream_id = %stream_id, "Shutting down screen share pipeline");
            pipeline.shutdown().await;
        }
    }

    // Stop webcam if active
    if let Some(pipeline) = voice_state.webcam.take() {
        info!("Auto-stopping webcam on voice leave");
        pipeline.shutdown().await;
    }

    // Stop all video decode tasks
    {
        let mut tasks = voice_state.video_tasks.lock().await;
        if !tasks.is_empty() {
            info!(
                count = tasks.len(),
                "Aborting video decode tasks on voice leave"
            );
            for handle in tasks.drain(..) {
                handle.abort();
            }
        }
        voice_state.active_video_count.store(0, Ordering::Relaxed);
    }

    // Drop any pending frame sinks (subscriptions whose track never arrived).
    // Dropping the FrameBuffer closes its watch channel, which causes the
    // emitter task in spawn_frame_emitter to exit cleanly.
    voice_state.video_frame_sinks.lock().await.clear();

    // Stop audio and drop mixer
    voice_state.audio.stop_all().await;
    voice_state.audio_tx = None;
    voice_state.audio_mixer = None;
    // Disconnect WebRTC
    voice_state
        .webrtc
        .disconnect()
        .await
        .map_err(|e| e.to_string())?;

    // Clear receive stats AFTER disconnect: decode tasks publish until their
    // tracks close, so clearing first would let a final publish resurrect
    // stale entries into the next session.
    voice_state.connection_stats.clear();

    // Send VoiceLeave to server
    if let Some(channel_id) = channel_id {
        let ws = state.websocket.read().await;
        if let Some(ws_manager) = ws.as_ref() {
            let _ = ws_manager
                .send(ClientEvent::VoiceLeave { channel_id })
                .await;
        }
    }

    info!("Left voice channel");
    Ok(())
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

/// Set noise suppression (uses RNNoise denoised output when enabled).
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
#[command]
pub async fn set_input_device(
    device_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Setting input device: {:?}", device_id);

    state.ensure_voice().await?;

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    voice_state.audio.set_input_device(device_id);

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

    voice_state.audio.set_output_device(device_id);

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
/// Wire format (snake_case per project convention). The TS layer converts
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
    // Per RFC 3550 §5.1: start seq and timestamp at random values for each session.
    // SSRC is already per-session (derived from SystemTime at spawn), so combining
    // with a random seq start ensures receivers see a fresh stream after reconnect.
    let mut seq: u16 = rand::random();
    let mut timestamp: u32 = rand::random();

    // Calculate samples per frame (20ms at 48kHz)
    const SAMPLES_PER_FRAME: u32 = SAMPLE_RATE / 1000 * FRAME_SIZE_MS as u32;

    // RTP payload type for Opus (as configured in webrtc/mod.rs)
    const OPUS_PAYLOAD_TYPE: u8 = 111;

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
