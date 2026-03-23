//! Screen Share Commands
//!
//! Tauri commands for native screen capture and sharing via WebRTC.

use std::sync::mpsc as std_mpsc;

use serde::{Deserialize, Serialize};
use tauri::{command, State};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::capture::capturer::FrameCapturer;
use crate::capture::source::enumerate_sources;
use crate::capture::{CaptureSource, CaptureSourceType};
use crate::video::encoder::{VideoEncoder, Vp8Encoder};
use crate::video::rtp::VideoRtpSender;
use crate::video::{EncodedPacket, QualityParams};
use crate::AppState;

/// Maximum number of concurrent screen share pipelines per user.
const MAX_SCREEN_SHARES: usize = 3;

/// Status of an active screen share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareStatus {
    pub stream_id: String,
    pub source_name: String,
    pub source_type: CaptureSourceType,
    pub quality: String,
    pub with_audio: bool,
}

/// Active screen share pipeline (stored in `VoiceState`).
pub struct ScreenSharePipeline {
    pub source_name: String,
    pub quality: String,
    pub with_audio: bool,
    pub source_type: CaptureSourceType,
    pub shutdown_tx: watch::Sender<bool>,
    pub capturer_handle: tokio::task::JoinHandle<()>,
    pub encoder_handle: tokio::task::JoinHandle<()>,
    pub rtp_handle: tokio::task::JoinHandle<()>,
}

impl ScreenSharePipeline {
    /// Shut down the pipeline gracefully.
    pub async fn shutdown(self) {
        info!("Shutting down screen share pipeline");
        let _ = self.shutdown_tx.send(true);
        // Wait for tasks to finish (with timeout)
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let _ = self.capturer_handle.await;
            let _ = self.encoder_handle.await;
            let _ = self.rtp_handle.await;
        })
        .await;
        info!("Screen share pipeline shut down");
    }
}

/// Enumerate available capture sources (monitors and windows).
#[command]
#[tracing::instrument(skip_all)]
pub async fn enumerate_capture_sources() -> Result<Vec<CaptureSource>, String> {
    info!("Enumerating capture sources");

    let sources = enumerate_sources().map_err(|e| e.to_string())?;
    info!(count = sources.len(), "Found capture sources");
    Ok(sources)
}

/// Start native screen sharing.
///
/// Creates the capture -> encode -> RTP pipeline and begins sending
/// video frames via the WebRTC video track. Supports up to
/// `MAX_SCREEN_SHARES` concurrent streams.
#[command]
#[tracing::instrument(skip(state))]
pub async fn start_screen_share(
    source_id: String,
    quality: String,
    with_audio: bool,
    stream_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|e| format!("Invalid stream_id: {e}"))?;

    info!(source_id = %source_id, quality = %quality, stream_id = %stream_id, "Starting screen share");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    // Check concurrent stream limit
    if voice_state.screen_shares.len() >= MAX_SCREEN_SHARES {
        return Err(format!(
            "Maximum of {MAX_SCREEN_SHARES} concurrent screen shares reached"
        ));
    }

    // Check for duplicate stream ID
    if voice_state.screen_shares.contains_key(&stream_uuid) {
        return Err(format!("Stream already exists: {stream_id}"));
    }

    // Must be in a voice channel
    if voice_state.channel_id.is_none() {
        return Err("Not in a voice channel".into());
    }

    // Resolve quality params
    let params = QualityParams::from_tier(&quality)?;

    let source = enumerate_sources()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| format!("Source not found: {source_id}"))?;
    let source_name = source.name;
    let source_type = source.source_type;

    // Get video track from WebRTC
    let video_track = voice_state
        .webrtc
        .get_video_track()
        .await
        .ok_or("No video track available (not connected?)")?;

    // Create shutdown signal
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_rx2 = shutdown_tx.subscribe();

    // Create frame channel (bounded to avoid memory buildup)
    let (frame_tx, frame_rx) = std_mpsc::sync_channel(2);

    // Start capturer on blocking thread
    let capturer = FrameCapturer::new(source_id.clone(), params.fps, params.width, params.height);

    let capturer_handle = capturer
        .start(frame_tx, shutdown_rx)
        .map_err(|e| e.to_string())?;

    // Channel for encoded packets (encoder thread -> RTP sender task)
    let (pkt_tx, mut pkt_rx) = mpsc::channel::<Vec<EncodedPacket>>(4);

    // Encoder runs on a blocking thread (vpx_encode::Encoder is not Send)
    let encoder_handle = {
        let quality_str = quality.clone();
        let shutdown_rx = shutdown_rx2;

        tokio::task::spawn_blocking(move || {
            let mut encoder = match Vp8Encoder::new(&params) {
                Ok(e) => e,
                Err(e) => {
                    error!("Failed to create VP8 encoder: {e}");
                    return;
                }
            };

            info!(quality = %quality_str, "Encoder started");

            loop {
                if *shutdown_rx.borrow() {
                    info!("Encoder shutdown requested");
                    break;
                }

                match frame_rx.recv_timeout(std::time::Duration::from_millis(16)) {
                    Ok(i420) => match encoder.encode(&i420) {
                        Ok(packets) => {
                            if !packets.is_empty() && pkt_tx.blocking_send(packets).is_err() {
                                info!("Packet channel closed, stopping encoder");
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Encode error: {e}");
                        }
                    },
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {
                        // No frame this interval — loop back to check shutdown
                    }
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                        info!("Frame channel closed, stopping encoder");
                        break;
                    }
                }
            }

            info!("Encoder stopped");
        })
    };

    // RTP sender runs as async task
    let rtp_handle = tokio::spawn(async move {
        let rtp_sender = VideoRtpSender::new(video_track);

        info!("RTP sender started");

        while let Some(packets) = pkt_rx.recv().await {
            for pkt in &packets {
                if let Err(e) = rtp_sender.send_packet(pkt).await {
                    warn!("RTP send error: {e}");
                }
            }
        }

        info!("RTP sender stopped");
    });

    // Store pipeline in voice state
    voice_state.screen_shares.insert(
        stream_uuid,
        ScreenSharePipeline {
            source_name,
            quality,
            with_audio,
            source_type,
            shutdown_tx,
            capturer_handle,
            encoder_handle,
            rtp_handle,
        },
    );

    info!(stream_id = %stream_id, "Screen share started");
    Ok(())
}

/// Stop a specific screen share stream.
#[command]
#[tracing::instrument(skip(state))]
pub async fn stop_screen_share(
    stream_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|e| format!("Invalid stream_id: {e}"))?;

    info!(stream_id = %stream_id, "Stopping screen share");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    let pipeline = voice_state
        .screen_shares
        .remove(&stream_uuid)
        .ok_or_else(|| format!("No screen share with stream_id: {stream_id}"))?;

    pipeline.shutdown().await;

    info!(stream_id = %stream_id, "Screen share stopped");
    Ok(())
}

/// Get current screen share status for all active streams.
#[command]
#[tracing::instrument(skip(state))]
pub async fn get_screen_share_status(
    state: State<'_, AppState>,
) -> Result<Vec<ScreenShareStatus>, String> {
    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    Ok(voice_state
        .screen_shares
        .iter()
        .map(|(id, pipeline)| ScreenShareStatus {
            stream_id: id.to_string(),
            source_name: pipeline.source_name.clone(),
            source_type: pipeline.source_type.clone(),
            quality: pipeline.quality.clone(),
            with_audio: pipeline.with_audio,
        })
        .collect())
}
