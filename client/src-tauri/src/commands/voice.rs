//! Voice Commands
//!
//! Tauri commands for voice chat functionality.

use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;

use tauri::{command, AppHandle, Emitter, State};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use crate::audio::{AudioDeviceList, FRAME_SIZE_MS, SAMPLE_RATE};
use crate::network::ClientEvent;
use crate::webrtc::IceServerConfig;
use crate::AppState;

/// Join a voice channel.
///
/// Initializes audio pipeline and WebRTC, sends `VoiceJoin` to server.
/// Server will respond with `VoiceOffer` which should be handled by `handle_voice_offer`.
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

    // Set up ICE candidate callback to send candidates to server
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
                        })
                        .await
                    {
                        error!("Failed to send ICE candidate: {}", e);
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

    // Set up remote track callback for audio playback
    let app_clone = app.clone();
    voice_state
        .webrtc
        .set_on_remote_track(move |track| {
            info!("Remote audio track received: {}", track.kind());
            let _ = app_clone.emit("voice:remote_track", track.kind().to_string());
        })
        .await;

    voice_state.channel_id = Some(channel_id.clone());

    // Send VoiceJoin to server via WebSocket
    let ws = state.websocket.read().await;
    if let Some(ws_manager) = ws.as_ref() {
        ws_manager
            .send(ClientEvent::VoiceJoin { channel_id })
            .await
            .map_err(|e| format!("Failed to send VoiceJoin: {e}"))?;
    } else {
        return Err("WebSocket not connected".into());
    }

    info!("VoiceJoin sent, waiting for server offer");
    Ok(())
}

/// Handle SDP offer from server and return answer.
///
/// Called when frontend receives <ws:voice_offer> event.
#[command]
pub async fn handle_voice_offer(
    channel_id: String,
    sdp: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Handling voice offer for channel: {}", channel_id);

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    // Verify we're joining this channel
    if voice_state.channel_id.as_ref() != Some(&channel_id) {
        return Err(format!(
            "Received offer for wrong channel: {} (expected {:?})",
            channel_id, voice_state.channel_id
        ));
    }

    // Handle the offer and get answer
    let answer = voice_state
        .webrtc
        .handle_offer(&sdp)
        .await
        .map_err(|e| e.to_string())?;

    // Send answer to server
    let ws = state.websocket.read().await;
    if let Some(ws_manager) = ws.as_ref() {
        ws_manager
            .send(ClientEvent::VoiceAnswer {
                channel_id: channel_id.clone(),
                sdp: answer,
            })
            .await
            .map_err(|e| format!("Failed to send VoiceAnswer: {e}"))?;
    } else {
        return Err("WebSocket not connected".into());
    }

    // Start audio capture
    let (audio_tx, audio_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    voice_state
        .audio
        .start_capture(audio_tx.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Start audio playback
    let (_playback_tx, playback_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    voice_state
        .audio
        .start_playback(playback_rx)
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

    info!("Voice answer sent, audio started");
    Ok(())
}

/// Handle ICE candidate from server.
///
/// Called when frontend receives <ws:voice_ice_candidate> event.
#[command]
pub async fn handle_voice_ice_candidate(
    channel_id: String,
    candidate: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!("Handling ICE candidate for channel: {}", channel_id);

    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    // Verify we're in this channel
    if voice_state.channel_id.as_ref() != Some(&channel_id) {
        return Err(format!(
            "Received ICE candidate for wrong channel: {channel_id}"
        ));
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

    // Stop screen share if active
    if let Some(pipeline) = voice_state.screen_share.take() {
        info!("Auto-stopping screen share on voice leave");
        pipeline.shutdown().await;
    }

    // Stop audio
    voice_state.audio.stop_all().await;
    voice_state.audio_tx = None;

    // Disconnect WebRTC
    voice_state
        .webrtc
        .disconnect()
        .await
        .map_err(|e| e.to_string())?;

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
    // RTP state - using atomics for simplicity
    static SEQUENCE_NUMBER: AtomicU16 = AtomicU16::new(0);
    static TIMESTAMP: AtomicU32 = AtomicU32::new(0);

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
        // Get current sequence number and timestamp
        let seq = SEQUENCE_NUMBER.fetch_add(1, Ordering::Relaxed);
        let ts = TIMESTAMP.fetch_add(SAMPLES_PER_FRAME, Ordering::Relaxed);

        // Create RTP packet
        let rtp_packet = RtpPacket {
            header: webrtc::rtp::header::Header {
                version: 2,
                padding: false,
                extension: false,
                marker: false,
                payload_type: OPUS_PAYLOAD_TYPE,
                sequence_number: seq,
                timestamp: ts,
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
    }

    info!("RTP audio sender task ended");
}
