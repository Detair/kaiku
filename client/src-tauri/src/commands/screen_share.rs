//! Screen Share Commands
//!
//! A native screen share runs in two steps (contract C30):
//!
//! 1. `prepare_screen_share` checks the source (Windows/macOS) or lets the system dialog choose it
//!    (Linux: xdg-desktop-portal `ScreenCast`). Nothing is sent; the frontend announces the share
//!    to the server.
//! 2. After the server accepted it (`screen_share_started`), `start_screen_share` adds a fresh
//!    video sender, starts capture → encode → RTP and renegotiates the publisher connection.
//!
//! `stop_screen_share` ends either step. One share per person for 1.0. A
//! capture that ends by itself emits `voice:local_capture_ended` so the
//! frontend runs its normal stop.

use serde::Serialize;
use tauri::{command, AppHandle, Emitter, State};
use tracing::{info, warn};
use uuid::Uuid;

use super::video_negotiation::{renegotiate, NegotiationCtx, Unconfirmed};
use crate::capture::capturer::FrameCapturer;
use crate::capture::source::enumerate_sources;
use crate::capture::{CaptureSource, CaptureSourceType};
use crate::video::publication::{StartCapture, VideoPublication};
use crate::video::QualityParams;
use crate::webrtc::VideoKind;
use crate::AppState;

/// Event to the frontend when a capture ended by itself.
pub const LOCAL_CAPTURE_ENDED_EVENT: &str = "voice:local_capture_ended";

/// Payload of [`LOCAL_CAPTURE_ENDED_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct LocalCaptureEnded {
    pub kind: &'static str,
    pub stream_id: Option<String>,
    /// Why it ended (`video::publication::end_reason`).
    pub reason: &'static str,
}

/// Where a prepared share's frames come from.
pub enum ShareSource {
    /// A `scap` target (Windows, macOS).
    Native { source_id: String },
    /// A `ScreenCast` portal session (Linux); its stream closes when dropped.
    #[cfg(target_os = "linux")]
    Portal(crate::capture::portal::PortalStream),
}

/// A share the user chose but the server has not accepted yet.
pub struct PreparedShare {
    pub source: ShareSource,
    pub source_name: String,
    pub source_type: CaptureSourceType,
    pub quality: String,
}

/// A running share.
pub struct LiveShare {
    pub source_name: String,
    pub source_type: CaptureSourceType,
    pub quality: String,
    pub publication: VideoPublication,
}

/// What [`ShareSlot::remove`] found.
pub enum Removed<P, L> {
    Nothing,
    Prepared(P),
    Live(L),
}

/// One share at a time: being chosen (the system dialog is open),
/// prepared (waiting for the server) or live.
pub struct ShareSlot<P, L> {
    choosing: Option<Uuid>,
    prepared: Option<(Uuid, P)>,
    live: Option<(Uuid, L)>,
}

impl<P, L> Default for ShareSlot<P, L> {
    fn default() -> Self {
        Self {
            choosing: None,
            prepared: None,
            live: None,
        }
    }
}

impl<P, L> ShareSlot<P, L> {
    /// Whether a share is being chosen, prepared or live.
    pub const fn is_busy(&self) -> bool {
        self.choosing.is_some() || self.prepared.is_some() || self.live.is_some()
    }

    /// Mark share `id` as being chosen (before the source dialog opens), so
    /// a second prepare (a double click) fails at once instead of opening
    /// a second dialog.
    pub const fn begin_choosing(&mut self, id: Uuid) -> Result<(), &'static str> {
        if self.is_busy() {
            return Err("SHARE_ACTIVE: already sharing");
        }
        self.choosing = Some(id);
        Ok(())
    }

    /// The dialog for `id` closed without a share (cancel, error).
    pub fn end_choosing(&mut self, id: Uuid) {
        if self.choosing == Some(id) {
            self.choosing = None;
        }
    }

    /// Hold a prepared share (fails while another one is chosen, prepared
    /// or live; the share being chosen under `id` becomes it).
    pub fn prepare(&mut self, id: Uuid, prepared: P) -> Result<(), &'static str> {
        self.end_choosing(id);
        if self.is_busy() {
            return Err("SHARE_ACTIVE: already sharing");
        }
        self.prepared = Some((id, prepared));
        Ok(())
    }

    /// Take the prepared share `id` to publish it.
    pub fn take_prepared(&mut self, id: Uuid) -> Option<P> {
        if self.prepared.as_ref().is_some_and(|(p, _)| *p == id) {
            self.prepared.take().map(|(_, p)| p)
        } else {
            None
        }
    }

    pub fn set_live(&mut self, id: Uuid, live: L) {
        self.live = Some((id, live));
    }

    pub fn remove(&mut self, id: Uuid) -> Removed<P, L> {
        if let Some(p) = self.take_prepared(id) {
            return Removed::Prepared(p);
        }
        if self.live.as_ref().is_some_and(|(l, _)| *l == id) {
            if let Some((_, l)) = self.live.take() {
                return Removed::Live(l);
            }
        }
        Removed::Nothing
    }

    /// Everything, for a session teardown.
    pub fn drain(&mut self) -> (Option<P>, Option<L>) {
        self.choosing = None;
        (
            self.prepared.take().map(|(_, p)| p),
            self.live.take().map(|(_, l)| l),
        )
    }

    pub fn live(&self) -> Option<(&Uuid, &L)> {
        self.live.as_ref().map(|(id, l)| (id, l))
    }
}

/// Status of an active screen share.
#[derive(Debug, Clone, Serialize)]
pub struct ScreenShareStatus {
    pub stream_id: String,
    pub source_name: String,
    pub source_type: CaptureSourceType,
    pub quality: String,
    pub with_audio: bool,
}

/// Answer of `prepare_screen_share`.
#[derive(Debug, Clone, Serialize)]
pub struct PreparedShareInfo {
    pub source_name: String,
    /// Native capture sends no screen audio yet.
    pub has_audio: bool,
}

/// Enumerate available capture sources (monitors and windows). Linux has no
/// list: the system dialog chooses there.
#[command]
#[tracing::instrument(skip_all)]
pub async fn enumerate_capture_sources() -> Result<Vec<CaptureSource>, String> {
    let sources = tokio::task::spawn_blocking(enumerate_sources)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    info!(count = sources.len(), "Found capture sources");
    Ok(sources)
}

/// Step 1: choose and check the source; nothing is sent.
#[command]
#[tracing::instrument(skip(state))]
pub async fn prepare_screen_share(
    stream_id: String,
    source_id: Option<String>,
    quality: String,
    with_audio: bool,
    state: State<'_, AppState>,
) -> Result<PreparedShareInfo, String> {
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|e| format!("Invalid stream_id: {e}"))?;
    QualityParams::from_tier(&quality)?;
    let _ = with_audio; // screen audio is not captured natively yet

    // Check and mark first, without holding the voice lock across the dialog.
    let epoch = {
        let mut voice = state.voice.write().await;
        let v = voice.as_mut().ok_or("Voice not initialized")?;
        if v.channel_id.is_none() {
            return Err("Not in a voice channel".into());
        }
        v.shares.begin_choosing(stream_uuid)?;
        v.session_epoch
    };

    let prepared = match choose_source(source_id, quality).await {
        Ok(prepared) => prepared,
        Err(e) => {
            if let Some(v) = state.voice.write().await.as_mut() {
                v.shares.end_choosing(stream_uuid);
            }
            return Err(e);
        }
    };
    let info = PreparedShareInfo {
        source_name: prepared.source_name.clone(),
        has_audio: false,
    };

    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    // The call may have ended (or another begun) while the dialog was open.
    if v.channel_id.is_none() || v.session_epoch != epoch {
        v.shares.end_choosing(stream_uuid);
        return Err("Not in a voice channel".into());
    }
    v.shares.prepare(stream_uuid, prepared)?;
    Ok(info)
}

#[cfg(not(target_os = "linux"))]
async fn choose_source(
    source_id: Option<String>,
    quality: String,
) -> Result<PreparedShare, String> {
    let source_id = source_id.ok_or("No source selected")?;
    let wanted = source_id.clone();
    let source = tokio::task::spawn_blocking(move || {
        enumerate_sources().map(|all| all.into_iter().find(|s| s.id == wanted))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| match e {
        crate::capture::CaptureError::PermissionDenied => format!("PERMISSION_DENIED: {e}"),
        other => other.to_string(),
    })?
    .ok_or_else(|| format!("NO_DEVICE: source not found: {source_id}"))?;
    Ok(PreparedShare {
        source: ShareSource::Native { source_id },
        source_name: source.name,
        source_type: source.source_type,
        quality,
    })
}

#[cfg(target_os = "linux")]
async fn choose_source(
    _source_id: Option<String>,
    quality: String,
) -> Result<PreparedShare, String> {
    use crate::capture::portal::{open_screencast, DbusPortal, DIALOG_TIMEOUT};
    let stream = open_screencast(&DbusPortal, DIALOG_TIMEOUT)
        .await
        .map_err(|e| e.code_text())?;
    Ok(PreparedShare {
        source_name: stream.label(),
        source_type: if stream.is_window() {
            CaptureSourceType::Window
        } else {
            CaptureSourceType::Monitor
        },
        source: ShareSource::Portal(stream),
        quality,
    })
}

fn capture_starter(source: ShareSource, params: QualityParams) -> StartCapture {
    match source {
        // scap cannot tell why a capture ended: the reason stays "ended".
        ShareSource::Native { source_id } => Box::new(move |frames, shutdown, _reason| {
            FrameCapturer::new(source_id, params.fps, params.width, params.height)
                .start(frames, shutdown)
                .map_err(|e| e.to_string())
        }),
        #[cfg(target_os = "linux")]
        ShareSource::Portal(stream) => Box::new(move |frames, shutdown, reason| {
            Ok(crate::capture::pipewire_capture::start(
                stream, params, frames, shutdown, reason,
            ))
        }),
    }
}

/// Step 2 (after `screen_share_started`): add the track and send.
#[command]
#[tracing::instrument(skip(state, app))]
pub async fn start_screen_share(
    stream_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|e| format!("Invalid stream_id: {e}"))?;
    let mut voice = state.voice.write().await;
    let v = voice.as_mut().ok_or("Voice not initialized")?;
    // Gone when the call ended in between (teardown drops prepared shares).
    let prepared = v
        .shares
        .take_prepared(stream_uuid)
        .ok_or("Not in a voice channel: share not prepared")?;
    let params = QualityParams::from_tier(&prepared.quality)?;

    let (track, sender) = v
        .webrtc
        .add_video_sender(VideoKind::Screen, &stream_id)
        .await
        .map_err(|e| e.to_string())?;
    let on_end = {
        let app = app.clone();
        let stream_id = stream_id.clone();
        Box::new(move |reason: &'static str| {
            let _ = app.emit(
                LOCAL_CAPTURE_ENDED_EVENT,
                LocalCaptureEnded {
                    kind: VideoKind::Screen.as_str(),
                    stream_id: Some(stream_id),
                    reason,
                },
            );
        })
    };
    let publication = match VideoPublication::start(
        params,
        track,
        sender.clone(),
        capture_starter(prepared.source, params),
        on_end,
    ) {
        Ok(p) => p,
        Err(e) => {
            let _ = v.webrtc.remove_video_sender(&sender).await;
            return Err(e);
        }
    };
    v.shares.set_live(
        stream_uuid,
        LiveShare {
            source_name: prepared.source_name,
            source_type: prepared.source_type,
            quality: prepared.quality,
            publication,
        },
    );
    let ctx = NegotiationCtx::from_state(&state, &app);
    renegotiate(v, &ctx, Some(Unconfirmed::Screen(stream_uuid))).await;
    info!(stream_id = %stream_id, "Screen share started");
    Ok(())
}

/// Stop a screen share, prepared or live. The publication is shut down
/// outside the voice lock (it may take up to 3 s), so answers, candidates
/// and mute keep flowing meanwhile.
#[command]
#[tracing::instrument(skip(state, app))]
pub async fn stop_screen_share(
    stream_id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    let stream_uuid = Uuid::parse_str(&stream_id).map_err(|e| format!("Invalid stream_id: {e}"))?;
    let (live, epoch) = {
        let mut voice = state.voice.write().await;
        let v = voice.as_mut().ok_or("Voice not initialized")?;
        v.unconfirmed
            .retain(|w| w.item != Unconfirmed::Screen(stream_uuid));
        match v.shares.remove(stream_uuid) {
            Removed::Live(live) => (live, v.session_epoch),
            // Nothing, or prepared only (dropping closes a portal session).
            Removed::Nothing | Removed::Prepared(_) => return Ok(()),
        }
    };
    let sender = live.publication.sender.clone();
    live.publication.shutdown().await;
    let mut voice = state.voice.write().await;
    let Some(v) = voice.as_mut() else {
        return Ok(());
    };
    if v.session_epoch != epoch {
        return Ok(()); // a newer session; this one's connection is gone
    }
    if let Err(e) = v.webrtc.remove_video_sender(&sender).await {
        warn!("Removing the share's sender failed: {e}");
    }
    let ctx = NegotiationCtx::from_state(&state, &app);
    renegotiate(v, &ctx, None).await;
    Ok(())
}

/// Current screen share status.
#[command]
#[tracing::instrument(skip(state))]
pub async fn get_screen_share_status(
    state: State<'_, AppState>,
) -> Result<Vec<ScreenShareStatus>, String> {
    let voice = state.voice.read().await;
    let v = voice.as_ref().ok_or("Voice not initialized")?;
    Ok(v.shares
        .live()
        .map(|(id, live)| ScreenShareStatus {
            stream_id: id.to_string(),
            source_name: live.source_name.clone(),
            source_type: live.source_type.clone(),
            quality: live.quality.clone(),
            with_audio: false,
        })
        .into_iter()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_stop_share_again() {
        let mut slot: ShareSlot<&str, &str> = ShareSlot::default();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        slot.prepare(a, "prep-a").unwrap();
        assert_eq!(slot.take_prepared(a), Some("prep-a"));
        slot.set_live(a, "live-a");
        assert!(matches!(slot.remove(a), Removed::Live("live-a")));
        slot.prepare(b, "prep-b").unwrap();
        assert_eq!(slot.take_prepared(b), Some("prep-b"));
        slot.set_live(b, "live-b");
        assert!(slot.live().is_some_and(|(id, _)| *id == b));
    }

    #[test]
    fn one_share_at_a_time() {
        let mut slot: ShareSlot<(), ()> = ShareSlot::default();
        slot.prepare(Uuid::new_v4(), ()).unwrap();
        assert!(
            slot.prepare(Uuid::new_v4(), ()).is_err(),
            "second prepare while prepared"
        );
        let mut live: ShareSlot<(), ()> = ShareSlot::default();
        live.set_live(Uuid::new_v4(), ());
        assert!(
            live.prepare(Uuid::new_v4(), ()).is_err(),
            "second share while live"
        );
    }

    #[test]
    fn a_second_prepare_while_the_dialog_is_open_fails_at_once() {
        let mut slot: ShareSlot<&str, &str> = ShareSlot::default();
        let a = Uuid::new_v4();
        slot.begin_choosing(a).unwrap();
        assert!(
            slot.begin_choosing(Uuid::new_v4()).is_err(),
            "double click: no second dialog"
        );
        slot.prepare(a, "prep-a").unwrap();
        assert_eq!(slot.take_prepared(a), Some("prep-a"));
        // A cancelled dialog frees the slot; a stale end changes nothing.
        let b = Uuid::new_v4();
        slot.begin_choosing(b).unwrap();
        slot.end_choosing(Uuid::new_v4());
        assert!(slot.is_busy());
        slot.end_choosing(b);
        assert!(!slot.is_busy());
        // A teardown while the dialog is open frees it too.
        slot.begin_choosing(b).unwrap();
        slot.drain();
        assert!(!slot.is_busy());
    }

    #[test]
    fn teardown_mid_start_drops_the_prepared_share_and_a_late_start_finds_nothing() {
        let mut slot: ShareSlot<&str, &str> = ShareSlot::default();
        let a = Uuid::new_v4();
        slot.prepare(a, "prep").unwrap();
        // The call ends while the server's answer is outstanding.
        assert_eq!(slot.drain(), (Some("prep"), None));
        // The late `start_screen_share` for it has nothing to publish.
        assert_eq!(slot.take_prepared(a), None);
        assert!(!slot.is_busy());
    }

    #[test]
    fn a_start_for_another_stream_does_not_take_the_prepared_one() {
        let mut slot: ShareSlot<&str, &str> = ShareSlot::default();
        let a = Uuid::new_v4();
        slot.prepare(a, "prep").unwrap();
        assert_eq!(slot.take_prepared(Uuid::new_v4()), None);
        assert!(matches!(slot.remove(Uuid::new_v4()), Removed::Nothing));
        assert_eq!(slot.take_prepared(a), Some("prep"));
    }
}
