//! Screen Share Commands
//!
//! Tauri commands for native screen capture and sharing via WebRTC.

use crate::capture::capturer::FrameCapturer;
use crate::capture::source::{enumerate_sources, find_target_by_id};
use crate::capture::{CaptureSource, CaptureSourceType};
use crate::video::encoder::{VideoEncoder, Vp9Encoder};
use crate::video::rtp::VideoRtpSender;
use crate::video::{EncodedPacket, QualityParams};
use crate::AppState;

use serde::{Deserialize, Serialize};
use tauri::command;
use tauri::State;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

/// Status of an active screen share.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenShareStatus {
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
/// Creates the capture → encode → RTP pipeline and begins sending
/// video frames via the WebRTC video track.
#[command]
#[tracing::instrument(skip(state))]
pub async fn start_screen_share(
    source_id: String,
    quality: String,
    with_audio: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(source_id = %source_id, quality = %quality, "Starting screen share");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    // Check not already sharing
    if voice_state.screen_share.is_some() {
        return Err("Already sharing screen".into());
    }

    // Must be in a voice channel
    if voice_state.channel_id.is_none() {
        return Err("Not in a voice channel".into());
    }

    // Resolve quality params
    let params = QualityParams::from_tier(&quality).map_err(|e| e)?;

    // Verify source exists
    let target =
        find_target_by_id(&source_id).ok_or_else(|| format!("Source not found: {source_id}"))?;

    let source_name = match &target {
        scap::Target::Display(d) => {
            if d.title.is_empty() {
                format!("Display {}", d.id)
            } else {
                d.title.clone()
            }
        }
        scap::Target::Window(w) => {
            if w.title.is_empty() {
                format!("Window {}", w.id)
            } else {
                w.title.clone()
            }
        }
    };

    let source_type = match &target {
        scap::Target::Display(_) => CaptureSourceType::Monitor,
        scap::Target::Window(_) => CaptureSourceType::Window,
    };

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
    let (frame_tx, mut frame_rx) = mpsc::channel(2);

    // Start capturer on blocking thread
    let capturer = FrameCapturer::new(
        target,
        source_id.clone(),
        params.fps,
        params.width,
        params.height,
    );

    let capturer_handle = capturer
        .start(frame_tx, shutdown_rx)
        .map_err(|e| e.to_string())?;

    // Channel for encoded packets (encoder thread → RTP sender task)
    let (pkt_tx, mut pkt_rx) = mpsc::channel::<Vec<EncodedPacket>>(4);

    // Encoder runs on a blocking thread (vpx_encode::Encoder is not Send)
    let encoder_handle = {
        let quality_str = quality.clone();
        let shutdown_rx = shutdown_rx2;

        tokio::task::spawn_blocking(move || {
            let mut encoder = match Vp9Encoder::new(&params) {
                Ok(e) => e,
                Err(e) => {
                    error!("Failed to create VP9 encoder: {e}");
                    return;
                }
            };

            info!(quality = %quality_str, "Encoder started");

            loop {
                if *shutdown_rx.borrow() {
                    info!("Encoder shutdown requested");
                    break;
                }

                match frame_rx.try_recv() {
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
                    Err(mpsc::error::TryRecvError::Empty) => {
                        // No frame available — sleep briefly then re-check shutdown
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
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
    voice_state.screen_share = Some(ScreenSharePipeline {
        source_name,
        quality,
        with_audio,
        source_type,
        shutdown_tx,
        capturer_handle,
        encoder_handle,
        rtp_handle,
    });

    info!("Screen share started");
    Ok(())
}

/// Stop screen sharing.
#[command]
#[tracing::instrument(skip(state))]
pub async fn stop_screen_share(state: State<'_, AppState>) -> Result<(), String> {
    info!("Stopping screen share");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    let pipeline = voice_state
        .screen_share
        .take()
        .ok_or("Not sharing screen")?;

    pipeline.shutdown().await;

    info!("Screen share stopped");
    Ok(())
}

/// Get current screen share status.
#[command]
#[tracing::instrument(skip(state))]
pub async fn get_screen_share_status(
    state: State<'_, AppState>,
) -> Result<Option<ScreenShareStatus>, String> {
    let voice = state.voice.read().await;
    let voice_state = voice.as_ref().ok_or("Voice not initialized")?;

    Ok(voice_state
        .screen_share
        .as_ref()
        .map(|pipeline| ScreenShareStatus {
            source_name: pipeline.source_name.clone(),
            source_type: pipeline.source_type.clone(),
            quality: pipeline.quality.clone(),
            with_audio: pipeline.with_audio,
        }))
}
