//! Webcam Commands
//!
//! Like a screen share, the camera is prepared first (`prepare_webcam`:
//! device checked, nothing sent) and published only after the server's
//! `webcam_started` (`start_webcam`: fresh video sender, capture → encode →
//! RTP, renegotiation). A camera that ends by itself emits
//! `voice:local_capture_ended` with kind `webcam`.

use tauri::{command, AppHandle, Emitter, State};
use tracing::{info, warn};

use super::screen_share::{LocalCaptureEnded, LOCAL_CAPTURE_ENDED_EVENT};
use super::video_negotiation::{renegotiate, NegotiationCtx, Unconfirmed};
use crate::capture::webcam::{enumerate_webcam_devices, WebcamCapturer, WebcamDevice};
use crate::video::publication::VideoPublication;
use crate::video::QualityParams;
use crate::webrtc::VideoKind;
use crate::AppState;

/// A camera chosen but not yet accepted by the server.
pub struct PreparedWebcam {
    /// Identity of this camera publication (events and commands name it).
    pub publication_id: String,
    pub quality: String,
    pub device_index: u32,
    pub device_name: String,
}

/// The running camera.
pub struct WebcamPipeline {
    pub publication_id: String,
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

/// Check the camera and remember the choice; nothing is sent. Returns the
/// publication id that `start_webcam` and the end event use.
#[command]
#[tracing::instrument(skip(state))]
pub async fn prepare_webcam(
    quality: String,
    device_id: Option<u32>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    QualityParams::from_webcam_tier(&quality)?;
    let device_index = device_id.unwrap_or(0);
    let devices = tokio::task::spawn_blocking(enumerate_webcam_devices)
        .await
        .map_err(|e| e.to_string())??;
    let device_name = devices
        .into_iter()
        .find(|d| d.index == device_index)
        .map(|d| d.name)
        .ok_or_else(|| format!("NO_DEVICE: camera {device_index} not found"))?;
    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    if v.channel_id.is_none() {
        return Err("Not in a voice channel".into());
    }
    if v.webcam.is_some() || v.prepared_webcam.is_some() {
        return Err("Webcam already active".into());
    }
    let publication_id = uuid::Uuid::new_v4().to_string();
    v.prepared_webcam = Some(PreparedWebcam {
        publication_id: publication_id.clone(),
        quality,
        device_index,
        device_name,
    });
    Ok(publication_id)
}

/// Start the prepared camera `publication_id` and send it (after
/// `webcam_started`).
#[command]
#[tracing::instrument(skip(state, app))]
pub async fn start_webcam(
    publication_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    // Only the camera this window prepared (not an older session's).
    if v.prepared_webcam
        .as_ref()
        .is_none_or(|p| p.publication_id != publication_id)
    {
        return Err("Not in a voice channel: camera not prepared".into());
    }
    let Some(prepared) = v.prepared_webcam.take() else {
        return Err("Not in a voice channel: camera not prepared".into());
    };
    let params = QualityParams::from_webcam_tier(&prepared.quality)?;
    let device_index = prepared.device_index;

    let (track, sender) = v
        .webrtc
        .add_video_sender(VideoKind::Webcam, &publication_id)
        .await
        .map_err(|e| e.to_string())?;
    let on_end = {
        let app = app.clone();
        let id = publication_id.clone();
        Box::new(move |reason: &'static str| {
            let _ = app.emit(
                LOCAL_CAPTURE_ENDED_EVENT,
                LocalCaptureEnded {
                    kind: VideoKind::Webcam.as_str(),
                    stream_id: Some(id),
                    reason,
                },
            );
        })
    };
    let start_capture = Box::new(move |frames, shutdown, _reason| {
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
        publication_id: publication_id.clone(),
        quality: prepared.quality,
        device_name: prepared.device_name,
        publication,
    });
    let ctx = NegotiationCtx::from_state(&state, &app);
    renegotiate(v, &ctx, Some(Unconfirmed::Webcam(publication_id))).await;
    info!("Webcam started");
    Ok(())
}

/// Stop the camera, prepared or live (shut down outside the voice lock).
#[command]
#[tracing::instrument(skip(state, app))]
pub async fn stop_webcam(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let (pipeline, epoch) = {
        let mut voice = state.voice.write().await;
        let v = voice.as_mut().ok_or("Voice not initialized")?;
        v.prepared_webcam = None;
        let Some(pipeline) = v.webcam.take() else {
            return Ok(());
        };
        let id = pipeline.publication_id.clone();
        v.unconfirmed
            .retain(|w| w.item != Unconfirmed::Webcam(id.clone()));
        (pipeline, v.session_epoch)
    };
    let sender = pipeline.publication.sender.clone();
    pipeline.shutdown().await;
    let mut voice = state.voice.write().await;
    let Some(v) = voice.as_mut() else {
        return Ok(());
    };
    if v.session_epoch != epoch {
        return Ok(());
    }
    if let Err(e) = v.webrtc.remove_video_sender(&sender).await {
        warn!("Removing the camera's sender failed: {e}");
    }
    let ctx = NegotiationCtx::from_state(&state, &app);
    renegotiate(v, &ctx, None).await;
    Ok(())
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
