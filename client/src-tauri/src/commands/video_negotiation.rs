//! Publisher renegotiation for screen shares and the webcam.
//!
//! One offer at a time ([`PublisherNegotiation`]). Each sent offer gets a
//! deadline: without an answer after [`ANSWER_DEADLINE`] (or when the
//! answer cannot be applied) it is offered once more; if that fails too,
//! every publication still waiting for an applied answer is stopped and the
//! frontend is told (`voice:local_capture_ended`, reason `not_sent`), so it
//! runs its normal stop (server told, text shown). Nothing stays stuck: the
//! next change offers again at once.
//!
//! The server may drop a publisher offer silently (its offer rate limit)
//! or answer it with a `voice_error`; both end at the deadline.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use super::screen_share::{LocalCaptureEnded, Removed, LOCAL_CAPTURE_ENDED_EVENT};
use crate::network::{ClientEvent, WebSocketManager};
use crate::video::publication::end_reason;
use crate::webrtc::{OfferFailure, VideoKind};
use crate::VoiceState;

/// How long an offer waits for its answer. The server answers within a
/// round trip; its offer rate limit refills one offer per second, so a
/// retry after this always gets through.
pub const ANSWER_DEADLINE: Duration = Duration::from_secs(10);

/// A publication whose track is not yet confirmed by an applied answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unconfirmed {
    Screen(Uuid),
    Webcam(String),
}

/// What the deadline task needs to act on its own later.
#[derive(Clone)]
pub struct NegotiationCtx {
    pub voice: Arc<RwLock<Option<VoiceState>>>,
    pub ws: Arc<RwLock<Option<WebSocketManager>>>,
    pub app: Option<AppHandle>,
}

impl NegotiationCtx {
    pub fn from_state(state: &crate::AppState, app: &AppHandle) -> Self {
        Self {
            voice: state.voice.clone(),
            ws: state.websocket.clone(),
            app: Some(app.clone()),
        }
    }
}

/// A track came or went: offer now, or after the outstanding answer.
/// `added` is a publication that waits for that answer.
pub async fn renegotiate(v: &mut VoiceState, ctx: &NegotiationCtx, added: Option<Unconfirmed>) {
    if let Some(added) = added {
        v.unconfirmed.push(added);
    }
    if let Some(id) = v.negotiation.request() {
        send_offer(v, ctx, id).await;
    }
}

/// Create and send offer `id`, and arm its deadline. A failure counts as a
/// failed offer right away.
pub fn send_offer<'a>(
    v: &'a mut VoiceState,
    ctx: &'a NegotiationCtx,
    id: u64,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let sent = async {
            let channel_id = v.channel_id.clone().ok_or("Not in a voice channel")?;
            let sdp = v.webrtc.create_offer().await.map_err(|e| e.to_string())?;
            let guard = ctx.ws.read().await;
            let sender = guard.as_ref().ok_or("WebSocket not connected")?;
            sender
                .send(ClientEvent::VoicePublisherOffer { channel_id, sdp })
                .await
        }
        .await;
        match sent {
            Ok(()) => arm_deadline(ctx, v.session_epoch, id),
            Err(e) => {
                warn!(offer = id, "Publisher offer could not be sent: {e}");
                offer_failed(v, ctx, id).await;
            }
        }
    })
}

fn arm_deadline(ctx: &NegotiationCtx, epoch: Option<u64>, id: u64) {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(ANSWER_DEADLINE).await;
        let mut voice = ctx.voice.write().await;
        let Some(v) = voice.as_mut() else { return };
        // Only the offer of this very session, if it is still outstanding.
        if v.session_epoch != epoch || v.negotiation.outstanding_id() != Some(id) {
            return;
        }
        warn!(offer = id, "No answer to the publisher offer");
        offer_failed(v, &ctx, id).await;
    });
}

/// Offer `id` got no usable answer: retry once, then give up visibly.
pub async fn offer_failed(v: &mut VoiceState, ctx: &NegotiationCtx, id: u64) {
    match v.negotiation.offer_failed(id) {
        OfferFailure::Stale => {}
        OfferFailure::Retry(next) => {
            info!(offer = next, "Offering the publisher connection again");
            send_offer(v, ctx, next).await;
        }
        OfferFailure::GiveUp => give_up(v, ctx).await,
    }
}

/// Stop every publication still waiting for an applied answer and tell the
/// frontend, which runs its normal stop with a text.
pub async fn give_up(v: &mut VoiceState, ctx: &NegotiationCtx) {
    let waiting = std::mem::take(&mut v.unconfirmed);
    for item in waiting {
        let (kind, stream_id, publication) = match &item {
            Unconfirmed::Screen(id) => match v.shares.remove(*id) {
                Removed::Live(live) => (VideoKind::Screen, id.to_string(), Some(live.publication)),
                _ => continue,
            },
            Unconfirmed::Webcam(id) => {
                if v.webcam.as_ref().is_some_and(|w| &w.publication_id == id) {
                    let pipeline = v.webcam.take();
                    (
                        VideoKind::Webcam,
                        id.clone(),
                        pipeline.map(|p| p.publication),
                    )
                } else {
                    continue;
                }
            }
        };
        warn!(?item, "Publication could not be sent; stopping it");
        if let Some(publication) = publication {
            let sender = publication.sender.clone();
            publication.shutdown().await;
            let _ = v.webrtc.remove_video_sender(&sender).await;
        }
        if let Some(app) = &ctx.app {
            let _ = app.emit(
                LOCAL_CAPTURE_ENDED_EVENT,
                LocalCaptureEnded {
                    kind: kind.as_str(),
                    stream_id: Some(stream_id),
                    reason: end_reason::NOT_SENT,
                },
            );
        }
    }
}

/// Apply the answer to a renegotiation offer. A failing answer counts as a
/// failed offer (retry, then give up); a good one confirms the waiting
/// publications and sends a queued change.
pub async fn apply_renegotiation_answer(v: &mut VoiceState, ctx: &NegotiationCtx, sdp: &str) {
    let Some(id) = v.negotiation.outstanding_id() else {
        warn!("Publisher answer without an outstanding offer; ignored");
        return;
    };
    if let Err(e) = v.webrtc.set_remote_answer(sdp).await {
        warn!(offer = id, "Publisher answer could not be applied: {e}");
        offer_failed(v, ctx, id).await;
        return;
    }
    match v.negotiation.answered() {
        // A queued change: the waiting publications ride on the next offer.
        Some(next) => send_offer(v, ctx, next).await,
        None => v.unconfirmed.clear(),
    }
}
