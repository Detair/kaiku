//! Webcam Commands
//!
//! Tauri commands for native webcam capture and sharing via WebRTC.

use tauri::{command, State};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::capture::webcam::{enumerate_webcam_devices, WebcamCapturer, WebcamDevice};
use crate::video::encoder::{VideoEncoder, Vp8Encoder};
use crate::video::rtp::VideoRtpSender;
use crate::video::{EncodedPacket, QualityParams};
use crate::AppState;

/// Active webcam pipeline (stored in `VoiceState`).
pub struct WebcamPipeline {
    pub quality: String,
    pub device_name: String,
    pub shutdown_tx: watch::Sender<bool>,
    pub capturer_handle: tokio::task::JoinHandle<()>,
    pub encoder_handle: tokio::task::JoinHandle<()>,
    pub rtp_handle: tokio::task::JoinHandle<()>,
}

impl WebcamPipeline {
    /// Shut down the pipeline gracefully.
    pub async fn shutdown(self) {
        info!("Shutting down webcam pipeline");
        let _ = self.shutdown_tx.send(true);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let _ = self.capturer_handle.await;
            let _ = self.encoder_handle.await;
            let _ = self.rtp_handle.await;
        })
        .await;
        info!("Webcam pipeline shut down");
    }
}

/// Start native webcam capture.
///
/// Creates the capture -> encode -> RTP pipeline and begins sending
/// video frames via the WebRTC webcam track.
#[command]
#[tracing::instrument(skip(state))]
pub async fn start_webcam(
    quality: String,
    device_id: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!(quality = %quality, device_id = ?device_id, "Starting webcam");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    if voice_state.webcam.is_some() {
        return Err("Webcam already active".into());
    }

    if voice_state.channel_id.is_none() {
        return Err("Not in a voice channel".into());
    }

    let params = QualityParams::from_webcam_tier(&quality)?;

    let device_index = device_id.unwrap_or(0);

    // Determine device name for logging
    let device_name = enumerate_webcam_devices()
        .ok()
        .and_then(|devices| {
            devices
                .into_iter()
                .find(|d| d.index == device_index)
                .map(|d| d.name)
        })
        .unwrap_or_else(|| format!("Camera {device_index}"));

    // Get webcam track from WebRTC
    let webcam_track = voice_state
        .webrtc
        .get_webcam_track()
        .await
        .ok_or("No webcam track available (not connected?)")?;

    // Create shutdown signal
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let shutdown_rx2 = shutdown_tx.subscribe();

    // Create frame channel (bounded to avoid memory buildup)
    let (frame_tx, mut frame_rx) = mpsc::channel(2);

    // Start capturer on blocking thread
    let capturer = WebcamCapturer::new(device_index, params.fps, params.width, params.height);

    let capturer_handle = capturer.start(frame_tx, shutdown_rx)?;

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
                    error!("Failed to create VP8 encoder for webcam: {e}");
                    return;
                }
            };

            info!(quality = %quality_str, "Webcam encoder started");

            loop {
                if *shutdown_rx.borrow() {
                    info!("Webcam encoder shutdown requested");
                    break;
                }

                match frame_rx.try_recv() {
                    Ok(i420) => match encoder.encode(&i420) {
                        Ok(packets) => {
                            if !packets.is_empty() && pkt_tx.blocking_send(packets).is_err() {
                                info!("Webcam packet channel closed, stopping encoder");
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Webcam encode error: {e}");
                        }
                    },
                    Err(mpsc::error::TryRecvError::Empty) => {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        info!("Webcam frame channel closed, stopping encoder");
                        break;
                    }
                }
            }

            info!("Webcam encoder stopped");
        })
    };

    // RTP sender runs as async task
    let rtp_handle = tokio::spawn(async move {
        let rtp_sender = VideoRtpSender::new(webcam_track);

        info!("Webcam RTP sender started");

        while let Some(packets) = pkt_rx.recv().await {
            for pkt in &packets {
                if let Err(e) = rtp_sender.send_packet(pkt).await {
                    warn!("Webcam RTP send error: {e}");
                }
            }
        }

        info!("Webcam RTP sender stopped");
    });

    voice_state.webcam = Some(WebcamPipeline {
        quality,
        device_name,
        shutdown_tx,
        capturer_handle,
        encoder_handle,
        rtp_handle,
    });

    info!("Webcam started");
    Ok(())
}

/// Stop webcam capture.
#[command]
#[tracing::instrument(skip(state))]
pub async fn stop_webcam(state: State<'_, AppState>) -> Result<(), String> {
    info!("Stopping webcam");

    let mut voice = state.voice.write().await;
    let voice_state = voice.as_mut().ok_or("Voice not initialized")?;

    let pipeline = voice_state.webcam.take().ok_or("Webcam not active")?;

    pipeline.shutdown().await;

    info!("Webcam stopped");
    Ok(())
}

/// Enumerate available webcam devices.
#[command]
#[tracing::instrument]
pub async fn enumerate_webcam_devices_cmd() -> Result<Vec<WebcamDevice>, String> {
    info!("Enumerating webcam devices");

    let devices = enumerate_webcam_devices()?;
    info!(count = devices.len(), "Found webcam devices");
    Ok(devices)
}
