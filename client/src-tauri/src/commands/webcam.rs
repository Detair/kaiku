//! Webcam Commands
//!
//! Like a screen share, the camera is prepared first (`prepare_webcam`:
//! device checked, nothing sent) and published only after the server's
//! `webcam_started` (`start_webcam`: fresh video sender, capture → encode →
//! RTP, renegotiation). A camera that ends by itself emits
//! `voice:local_capture_ended` with kind `webcam`.

use tauri::{command, AppHandle, Emitter, State};
use tracing::{info, warn};

use super::screen_share::{renegotiate, LocalCaptureEnded, LOCAL_CAPTURE_ENDED_EVENT};
use crate::capture::webcam::{enumerate_webcam_devices, WebcamCapturer, WebcamDevice};
use crate::video::publication::VideoPublication;
use crate::video::QualityParams;
use crate::webrtc::VideoKind;
use crate::AppState;

/// A camera chosen but not yet accepted by the server.
pub struct PreparedWebcam {
    pub quality: String,
    pub device_index: u32,
}

/// The running camera.
pub struct WebcamPipeline {
    pub quality: String,
    pub device_name: String,
    pub publication: VideoPublication,
}

impl WebcamPipeline {
    /// Shut down the pipeline gracefully.
    pub async fn shutdown(self) {
        self.publication.shutdown().await;
    }
}

/// Check the camera and remember the choice; nothing is sent.
#[command]
#[tracing::instrument(skip(state))]
pub async fn prepare_webcam(
    quality: String,
    device_id: Option<u32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    QualityParams::from_webcam_tier(&quality)?;
    let device_index = device_id.unwrap_or(0);
    let devices = tokio::task::spawn_blocking(enumerate_webcam_devices)
        .await
        .map_err(|e| e.to_string())??;
    if !devices.iter().any(|d| d.index == device_index) {
        return Err(format!("NO_DEVICE: camera {device_index} not found"));
    }
    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    if v.channel_id.is_none() {
        return Err("Not in a voice channel".into());
    }
    if v.webcam.is_some() || v.prepared_webcam.is_some() {
        return Err("Webcam already active".into());
    }
    v.prepared_webcam = Some(PreparedWebcam {
        quality,
        device_index,
    });
    Ok(())
}

/// Start the prepared camera and send it (after `webcam_started`).
#[command]
#[tracing::instrument(skip(state, app))]
pub async fn start_webcam(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    let prepared = v
        .prepared_webcam
        .take()
        .ok_or("Not in a voice channel: camera not prepared")?;
    let params = QualityParams::from_webcam_tier(&prepared.quality)?;
    let device_index = prepared.device_index;
    let device_name = enumerate_webcam_devices()
        .ok()
        .and_then(|devices| {
            devices
                .into_iter()
                .find(|d| d.index == device_index)
                .map(|d| d.name)
        })
        .unwrap_or_else(|| format!("Camera {device_index}"));

    let publication_id = uuid::Uuid::new_v4().to_string();
    let (track, sender) = v
        .webrtc
        .add_video_sender(VideoKind::Webcam, &publication_id)
        .await
        .map_err(|e| e.to_string())?;
    let on_end = {
        let app = app.clone();
        Box::new(move || {
            let _ = app.emit(
                LOCAL_CAPTURE_ENDED_EVENT,
                LocalCaptureEnded {
                    kind: VideoKind::Webcam.as_str(),
                    stream_id: None,
                },
            );
        })
    };
    let start_capture = Box::new(move |frames, shutdown| {
        WebcamCapturer::new(device_index, params.fps, params.width, params.height)
            .start(frames, shutdown)
    });
    let publication =
        match VideoPublication::start(params, track, sender.clone(), start_capture, on_end) {
            Ok(p) => p,
            Err(e) => {
                let _ = v.webrtc.remove_video_sender(&sender).await;
                return Err(e);
            }
        };
    v.webcam = Some(WebcamPipeline {
        quality: prepared.quality,
        device_name,
        publication,
    });
    renegotiate(v, &state.websocket).await?;
    info!("Webcam started");
    Ok(())
}

/// Stop the camera, prepared or live.
#[command]
#[tracing::instrument(skip(state))]
pub async fn stop_webcam(state: State<'_, AppState>) -> Result<(), String> {
    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    v.prepared_webcam = None;
    let Some(pipeline) = v.webcam.take() else {
        return Ok(());
    };
    let sender = pipeline.publication.sender.clone();
    pipeline.shutdown().await;
    if let Err(e) = v.webrtc.remove_video_sender(&sender).await {
        warn!("Removing the camera's sender failed: {e}");
    }
    renegotiate(v, &state.websocket).await
}

#[command]
#[tracing::instrument]
pub async fn enumerate_webcam_devices_cmd() -> Result<Vec<WebcamDevice>, String> {
    let devices = tokio::task::spawn_blocking(enumerate_webcam_devices)
        .await
        .map_err(|e| e.to_string())??;
    info!(count = devices.len(), "Found webcam devices");
    Ok(devices)
}
