//! Decoded frames from the VP8 decoder to the webview, with flow control.
//!
//! The decoder pushes into a `watch` channel per view (latest frame wins,
//! the decoder never waits). One emitter task per view sends frames over a
//! Tauri channel as raw bytes (`tauri::ipc::Response`, an `ArrayBuffer` in
//! the webview), not as JSON.
//!
//! Flow control (frame budget):
//! - per view at most [`MAX_IN_FLIGHT_PER_VIEW`] frames sent but not yet acknowledged by the page
//!   (`ack_video_frame` with the frame's sequence number; an ack for a frame that is not
//!   outstanding frees nothing); a newer frame replaces an unsent one (latest wins);
//! - all views together at most [`MAX_IN_FLIGHT_TOTAL`] frames in flight;
//! - at most [`MAX_EMIT_FPS`] frames per second per view;
//! - at most [`MAX_VIEWS_PER_STREAM`] views of one stream and [`MAX_VIEWS`] in total;
//! - a view without any acknowledgement for [`ACK_TIMEOUT`] gets its slots back and the latest
//!   frame once more; a second such silence in a row stops sending until the page acknowledges a
//!   frame sent after that reset (a late acknowledgement of a frame the reset forgot shows the page
//!   is alive but frees nothing), and after [`VIEW_SILENCE_END`] without any acknowledgement the
//!   view ends;
//! - a view that ends here leaves the registry before the page is told, so its place is free;
//! - every subscription of an older page instance ends when a new page instance subscribes.
//!
//! So a stalled or slow page has at most four frames per view queued
//! towards it (two, then two after the one reset), ~13 MB per view at
//! 1080p (~3.2 MB per frame with decoder padding), and at most
//! [`MAX_VIEWS`] views.
//!
//! A view that ends here (leave, a newer page instance, a long silence)
//! gets [`FRAME_END_MAGIC`] as its last message, so the tile does not
//! freeze on its last picture. A view the page unsubscribed gets nothing.
//!
//! Wire layout of one frame (little endian), see [`encode_frame`]:
//!
//! | bytes | field |
//! |---|---|
//! | 0..4 | magic `KVF1` |
//! | 4..8 | width |
//! | 8..12 | height |
//! | 12..16 | `y_stride` |
//! | 16..20 | `uv_stride` |
//! | 20..24 | y length |
//! | 24..28 | u length |
//! | 28..32 | v length |
//! | 32..40 | pts (ms) |
//! | 40..44 | sequence number (per view, for `ack_video_frame`) |
//! | 44.. | Y, U, V planes |

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::ipc::{Channel, Response};
use tokio::sync::{watch, Notify};
use tracing::debug;

use crate::voice::video_decoder::DecodedFrame;

/// Frames per second sent to one view at most.
pub const MAX_EMIT_FPS: u32 = 30;
/// Frames sent to one view and not yet acknowledged, at most.
pub const MAX_IN_FLIGHT_PER_VIEW: u32 = 2;
/// Frames in flight to all views together, at most.
pub const MAX_IN_FLIGHT_TOTAL: u32 = 4;
/// Silence after which a view's unacknowledged slots count as free again.
pub const ACK_TIMEOUT: Duration = Duration::from_secs(3);
/// Silence (no acknowledgement at all while frames wait) after which a
/// view ends: its page is gone or has stopped for good.
pub const VIEW_SILENCE_END: Duration = Duration::from_secs(30);
/// Views of one stream at most (a page shows a share in one or two places).
pub const MAX_VIEWS_PER_STREAM: usize = 4;
/// Views in total at most.
pub const MAX_VIEWS: usize = 16;

/// Header size of [`encode_frame`].
pub const FRAME_HEADER_LEN: usize = 44;

/// Magic at the start of every frame.
pub const FRAME_MAGIC: &[u8; 4] = b"KVF1";

/// The whole last message of a view that ended natively.
pub const FRAME_END_MAGIC: &[u8; 4] = b"KVF0";

/// Raw framing of one decoded frame (see the module docs).
pub fn encode_frame(frame: &DecodedFrame, seq: u32) -> Vec<u8> {
    let planes = frame.y_plane.len() + frame.u_plane.len() + frame.v_plane.len();
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + planes);
    out.extend_from_slice(FRAME_MAGIC);
    let len = |v: &Vec<u8>| u32::try_from(v.len()).unwrap_or(u32::MAX);
    for value in [
        frame.width,
        frame.height,
        frame.y_stride,
        frame.uv_stride,
        len(&frame.y_plane),
        len(&frame.u_plane),
        len(&frame.v_plane),
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&frame.pts_ms.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&frame.y_plane);
    out.extend_from_slice(&frame.u_plane);
    out.extend_from_slice(&frame.v_plane);
    out
}

/// Sink-side handle for the decoder (latest frame wins). Frames are shared
/// (`Arc`), so several views and the emitter never copy the planes.
pub struct FrameBuffer {
    tx: watch::Sender<Option<Arc<DecodedFrame>>>,
}

impl FrameBuffer {
    pub fn new() -> (Self, watch::Receiver<Option<Arc<DecodedFrame>>>) {
        let (tx, rx) = watch::channel(None);
        (Self { tx }, rx)
    }

    pub fn push(&self, frame: Arc<DecodedFrame>) {
        let _ = self.tx.send(Some(frame));
    }
}

/// Flow-control state of one view.
#[derive(Debug)]
pub struct ViewFlow {
    /// Sequence numbers of the frames sent and not yet acknowledged.
    sent: Mutex<VecDeque<u32>>,
    next_seq: AtomicU32,
    acked: Notify,
    /// Start of the current acknowledgement window. Tokio time (pausable in tests).
    last_ack: Mutex<tokio::time::Instant>,
    /// The page's last acknowledgement of any kind (or the subscribe).
    last_life: Mutex<tokio::time::Instant>,
    /// The window was reset and no acknowledgement came since.
    reset_unanswered: AtomicBool,
    /// Two silences in a row: nothing is sent until the page answers.
    stalled: AtomicBool,
    dead: AtomicBool,
    /// The page itself ended the view (it needs no end message).
    ended_by_page: AtomicBool,
}

impl ViewFlow {
    fn new() -> Self {
        let now = tokio::time::Instant::now();
        Self {
            sent: Mutex::new(VecDeque::new()),
            next_seq: AtomicU32::new(0),
            acked: Notify::new(),
            last_ack: Mutex::new(now),
            last_life: Mutex::new(now),
            reset_unanswered: AtomicBool::new(false),
            stalled: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            ended_by_page: AtomicBool::new(false),
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
    }

    pub fn in_flight(&self) -> u32 {
        self.sent
            .lock()
            .map_or(0, |q| u32::try_from(q.len()).unwrap_or(u32::MAX))
    }

    fn is_stalled(&self) -> bool {
        self.stalled.load(Ordering::Acquire)
    }

    /// Record a frame as sent; returns its sequence number.
    fn mark_sent(&self) -> u32 {
        let seq = self.next_seq.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        if let Ok(mut q) = self.sent.lock() {
            q.push_back(seq);
        }
        seq
    }

    /// Forget every outstanding frame; returns how many there were.
    fn forget_sent(&self) -> u32 {
        self.sent.lock().map_or(0, |mut q| {
            let n = u32::try_from(q.len()).unwrap_or(u32::MAX);
            q.clear();
            n
        })
    }

    fn silence(&self) -> Duration {
        self.last_life
            .lock()
            .map_or(Duration::ZERO, |at| at.elapsed())
    }
}

/// Why a subscribe was refused.
pub const TOO_MANY_VIEWS: &str = "TOO_MANY_VIEWS: too many video views";

struct View {
    id: u64,
    page: String,
    buffer: Arc<FrameBuffer>,
    flow: Arc<ViewFlow>,
}

/// Where decoded frames go: per stream id, every open view (subscription).
/// The decode task looks the views up for every packet, so a view opened
/// late, re-opened after a layout change, or closed is seen at once.
#[derive(Clone, Default)]
pub struct FrameSinks {
    inner: Arc<Mutex<HashMap<String, Vec<View>>>>,
    next_id: Arc<AtomicU64>,
    /// Frames in flight to all views.
    total_in_flight: Arc<AtomicU32>,
    /// Wakes views waiting for the global budget when a slot frees.
    global_freed: Arc<Notify>,
    /// Runs right after an emitter failed to reserve a global slot (tests
    /// put a concurrent release exactly there).
    #[cfg(test)]
    reserve_failed_hook: Arc<Mutex<Option<Box<dyn FnMut() + Send>>>>,
}

impl FrameSinks {
    /// Add a view of `stream_id` for page instance `page`. Views of any
    /// other page instance end (the page was reloaded). Returns the
    /// subscription id and the view's flow state (for its emitter), or
    /// [`TOO_MANY_VIEWS`] beyond [`MAX_VIEWS_PER_STREAM`] / [`MAX_VIEWS`].
    pub fn subscribe(
        &self,
        stream_id: &str,
        page: &str,
        sink: FrameBuffer,
    ) -> Result<(u64, Arc<ViewFlow>), &'static str> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let flow = Arc::new(ViewFlow::new());
        let mut map = self.inner.lock().map_err(|_| "video views unavailable")?;
        for views in map.values_mut() {
            for view in views.iter().filter(|v| v.page != page) {
                view.flow.dead.store(true, Ordering::Release);
                view.flow.acked.notify_one();
            }
            views.retain(|v| v.page == page);
        }
        map.retain(|_, views| !views.is_empty());
        let total: usize = map.values().map(Vec::len).sum();
        let of_stream = map.get(stream_id).map_or(0, Vec::len);
        if total >= MAX_VIEWS || of_stream >= MAX_VIEWS_PER_STREAM {
            return Err(TOO_MANY_VIEWS);
        }
        map.entry(stream_id.to_string()).or_default().push(View {
            id,
            page: page.to_string(),
            buffer: Arc::new(sink),
            flow: flow.clone(),
        });
        Ok((id, flow))
    }

    /// Remove one view; its emitter ends.
    pub fn unsubscribe(&self, stream_id: &str, subscription_id: u64) -> bool {
        let Ok(mut map) = self.inner.lock() else {
            return false;
        };
        let Some(list) = map.get_mut(stream_id) else {
            return false;
        };
        let before = list.len();
        list.retain(|v| {
            let keep = v.id != subscription_id;
            if !keep {
                v.flow.ended_by_page.store(true, Ordering::Release);
                v.flow.dead.store(true, Ordering::Release);
                v.flow.acked.notify_one();
            }
            keep
        });
        let removed = list.len() != before;
        if list.is_empty() {
            map.remove(stream_id);
        }
        removed
    }

    /// The page took frame `seq` of this view. Only an outstanding frame
    /// frees its slot; any acknowledgement shows the page is alive.
    pub fn ack(&self, stream_id: &str, subscription_id: u64, seq: u32) {
        let Ok(map) = self.inner.lock() else { return };
        let Some(view) = map
            .get(stream_id)
            .and_then(|l| l.iter().find(|v| v.id == subscription_id))
        else {
            return;
        };
        let freed = view.flow.sent.lock().is_ok_and(|mut q| {
            q.iter()
                .position(|s| *s == seq)
                .and_then(|at| q.remove(at))
                .is_some()
        });
        let now = tokio::time::Instant::now();
        if let Ok(mut at) = view.flow.last_life.lock() {
            *at = now;
        }
        if !freed {
            // A late acknowledgement (of a frame the window reset forgot) or
            // an unknown one: the page is alive, but it frees nothing, opens
            // no new window and does not end a pause.
            return;
        }
        update_u32(&self.total_in_flight, |n| n.checked_sub(1));
        self.global_freed.notify_waiters();
        if let Ok(mut at) = view.flow.last_ack.lock() {
            *at = now;
        }
        view.flow.reset_unanswered.store(false, Ordering::Release);
        view.flow.stalled.store(false, Ordering::Release);
        view.flow.acked.notify_one();
    }

    /// The live views of `stream_id` right now.
    pub fn sinks_for(&self, stream_id: &str) -> Vec<Arc<FrameBuffer>> {
        self.inner
            .lock()
            .ok()
            .and_then(|map| {
                map.get(stream_id).map(|l| {
                    l.iter()
                        .filter(|v| !v.flow.is_dead())
                        .map(|v| v.buffer.clone())
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    pub fn has_sinks(&self, stream_id: &str) -> bool {
        !self.sinks_for(stream_id).is_empty()
    }

    /// Frames in flight to all views (tests, logging).
    pub fn total_in_flight(&self) -> u32 {
        self.total_in_flight.load(Ordering::Acquire)
    }

    /// Drop every view (voice leave).
    pub fn clear(&self) {
        if let Ok(mut map) = self.inner.lock() {
            for view in map.values().flatten() {
                view.flow.dead.store(true, Ordering::Release);
                view.flow.acked.notify_one();
            }
            map.clear();
        }
    }

    /// Run the emitter of one view until it ends; `send` delivers bytes.
    pub async fn run_emitter<F>(
        &self,
        mut rx: watch::Receiver<Option<Arc<DecodedFrame>>>,
        flow: Arc<ViewFlow>,
        max_fps: u32,
        mut send: F,
    ) where
        F: FnMut(Vec<u8>) -> bool,
    {
        let interval = Duration::from_secs(1) / max_fps.max(1);
        let mut unsent: Option<Arc<DecodedFrame>> = None;
        let mut last_sent: Option<Arc<DecodedFrame>> = None;
        loop {
            if flow.is_dead() {
                break;
            }
            // Listen for a freed global slot before trying to take one, so
            // a slot freed right after a failed try still wakes this view.
            let freed = self.global_freed.notified();
            tokio::pin!(freed);
            freed.as_mut().enable();
            let stalled = flow.is_stalled();
            let wants_slot =
                unsent.is_some() && !stalled && flow.in_flight() < MAX_IN_FLIGHT_PER_VIEW;
            if wants_slot {
                if self.reserve_global() {
                    let Some(frame) = unsent.take() else { continue };
                    let seq = flow.mark_sent();
                    if !send(encode_frame(&frame, seq)) {
                        debug!("Frame channel closed; emitter ends");
                        break;
                    }
                    last_sent = Some(frame);
                    tokio::time::sleep(interval).await;
                    continue;
                }
                #[cfg(test)]
                self.run_reserve_failed_hook();
            }
            // A silence while frames are out (GC pause, slow render, sleep,
            // a lost acknowledgement): the window resets and the latest
            // frame is offered again, once. A second silence in a row
            // stops sending until the page acknowledges a frame sent after
            // the reset; those frames stay charged meanwhile, so at most
            // two frames are forgotten per acknowledged one.
            let waiting = flow.in_flight() > 0;
            if waiting
                && flow
                    .last_ack
                    .lock()
                    .is_ok_and(|at| at.elapsed() >= ACK_TIMEOUT)
            {
                if let Ok(mut at) = flow.last_ack.lock() {
                    *at = tokio::time::Instant::now();
                }
                if flow.reset_unanswered.swap(true, Ordering::AcqRel) {
                    debug!("Still no frame acknowledgement; sending paused");
                    flow.stalled.store(true, Ordering::Release);
                } else {
                    debug!("No frame acknowledgement for a while; window reset");
                    self.release(&flow);
                    if unsent.is_none() {
                        unsent.clone_from(&last_sent);
                    }
                }
                continue;
            }
            let silence = flow.silence();
            if (waiting || stalled) && silence >= VIEW_SILENCE_END {
                debug!("No frame acknowledgement for a long time; view ends");
                break;
            }
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    unsent.clone_from(&rx.borrow_and_update());
                }
                () = flow.acked.notified() => {}
                // Another view freed a slot of the global budget.
                () = &mut freed, if wants_slot => {}
                () = tokio::time::sleep(ACK_TIMEOUT), if waiting => {}
                () = tokio::time::sleep(VIEW_SILENCE_END.saturating_sub(silence)), if stalled => {}
            }
        }
        flow.dead.store(true, Ordering::Release);
        self.release(&flow);
        if !flow.ended_by_page.load(Ordering::Acquire) {
            // Ended here: the view leaves the registry before the page is
            // told, so the page's next subscribe has its place again.
            self.remove_view(&flow);
            let _ = send(FRAME_END_MAGIC.to_vec());
        }
    }

    /// Remove the view whose flow state is `flow` (if it is still listed).
    fn remove_view(&self, flow: &Arc<ViewFlow>) {
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        for views in map.values_mut() {
            views.retain(|v| !Arc::ptr_eq(&v.flow, flow));
        }
        map.retain(|_, views| !views.is_empty());
    }

    #[cfg(test)]
    fn run_reserve_failed_hook(&self) {
        if let Ok(mut hook) = self.reserve_failed_hook.lock() {
            if let Some(hook) = hook.as_mut() {
                hook();
            }
        }
    }

    /// Return every slot `flow` holds to the budget and wake views that
    /// wait for the global budget.
    fn release(&self, flow: &ViewFlow) {
        let left = flow.forget_sent();
        if left > 0 {
            update_u32(&self.total_in_flight, |n| Some(n.saturating_sub(left)));
            self.global_freed.notify_waiters();
        }
    }

    /// Take one slot of the global budget.
    fn reserve_global(&self) -> bool {
        update_u32(&self.total_in_flight, |n| {
            (n < MAX_IN_FLIGHT_TOTAL).then_some(n + 1)
        })
    }
}

/// Apply `change` to `value` atomically; `false` when it declined (`None`).
fn update_u32(value: &AtomicU32, change: impl Fn(u32) -> Option<u32>) -> bool {
    let mut current = value.load(Ordering::Acquire);
    loop {
        let Some(next) = change(current) else {
            return false;
        };
        match value.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return true,
            Err(seen) => current = seen,
        }
    }
}

/// Emitter task for one Tauri channel (raw bytes).
pub fn spawn_frame_emitter(
    sinks: FrameSinks,
    channel: Channel<Response>,
    rx: watch::Receiver<Option<Arc<DecodedFrame>>>,
    flow: Arc<ViewFlow>,
) {
    tokio::spawn(async move {
        sinks
            .run_emitter(rx, flow, MAX_EMIT_FPS, move |bytes| {
                channel.send(Response::new(bytes)).is_ok()
            })
            .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, pts: u64) -> Arc<DecodedFrame> {
        let (yw, uvw) = (w as usize, (w as usize).div_ceil(2));
        let uvh = (h as usize).div_ceil(2);
        Arc::new(DecodedFrame {
            stream_id: "s".into(),
            width: w,
            height: h,
            y_plane: vec![1; yw * h as usize],
            u_plane: vec![2; uvw * uvh],
            v_plane: vec![3; uvw * uvh],
            y_stride: w,
            uv_stride: w.div_ceil(2),
            pts_ms: pts,
        })
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn pts_of(b: &[u8]) -> u64 {
        u64::from_le_bytes(b[32..40].try_into().unwrap())
    }

    fn seq_of(b: &[u8]) -> u32 {
        u32_at(b, 40)
    }

    #[test]
    fn framing_has_header_then_planes() {
        let f = frame(4, 2, 77);
        let b = encode_frame(&f, 5);
        assert_eq!(&b[0..4], FRAME_MAGIC);
        assert_eq!(u32_at(&b, 4), 4);
        assert_eq!(u32_at(&b, 8), 2);
        assert_eq!(u32_at(&b, 12), 4);
        assert_eq!(u32_at(&b, 16), 2);
        assert_eq!(u32_at(&b, 20), 8);
        assert_eq!(u32_at(&b, 24), 2);
        assert_eq!(u32_at(&b, 28), 2);
        assert_eq!(pts_of(&b), 77);
        assert_eq!(seq_of(&b), 5);
        assert_eq!(&b[44..52], &[1; 8]);
        assert_eq!(&b[52..54], &[2; 2]);
        assert_eq!(&b[54..56], &[3; 2]);
        assert_eq!(b.len(), FRAME_HEADER_LEN + 12);
    }

    #[test]
    fn views_subscribe_and_unsubscribe_per_stream() {
        let sinks = FrameSinks::default();
        let (a, _ra) = FrameBuffer::new();
        let (b, _rb) = FrameBuffer::new();
        let (ida, _) = sinks.subscribe("s1", "page", a).unwrap();
        let (idb, _) = sinks.subscribe("s1", "page", b).unwrap();
        assert_ne!(ida, idb);
        assert_eq!(sinks.sinks_for("s1").len(), 2);
        assert!(sinks.unsubscribe("s1", ida));
        assert!(!sinks.unsubscribe("s1", ida), "only once");
        assert_eq!(sinks.sinks_for("s1").len(), 1);
        assert!(sinks.unsubscribe("s1", idb));
        assert!(!sinks.has_sinks("s1"));
    }

    #[test]
    fn a_new_page_instance_ends_the_old_pages_views() {
        let sinks = FrameSinks::default();
        let (a, _ra) = FrameBuffer::new();
        let (_, old) = sinks.subscribe("s1", "page-1", a).unwrap();
        let (b, _rb) = FrameBuffer::new();
        sinks.subscribe("s2", "page-2", b).unwrap();
        assert!(old.is_dead(), "the reloaded page's view is gone");
        assert!(!sinks.has_sinks("s1"));
        assert!(sinks.has_sinks("s2"));
    }

    type Sent = Arc<Mutex<Vec<u64>>>;

    fn recorder() -> (Sent, impl FnMut(Vec<u8>) -> bool) {
        let sent: Sent = Arc::default();
        let s = sent.clone();
        (sent, move |b: Vec<u8>| {
            if b.len() >= FRAME_HEADER_LEN {
                s.lock().unwrap().push(pts_of(&b));
            }
            true
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_page_that_does_not_take_frames_gets_at_most_two_and_the_view_stays() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            async move { sinks.run_emitter(rx, flow, 1000, send).await }
        });
        for pts in 0..50 {
            fb.push(frame(2, 2, pts));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            sent.lock().unwrap().len(),
            2,
            "never more than two unacknowledged"
        );
        assert!(sinks.total_in_flight() <= MAX_IN_FLIGHT_TOTAL);
        assert!(!flow.is_dead(), "a stall does not end the view");
        sinks.unsubscribe("s1", id);
        task.await.unwrap();
        assert_eq!(sinks.total_in_flight(), 0, "its budget is returned");
    }

    #[tokio::test(start_paused = true)]
    async fn a_view_resumes_after_a_long_silence_with_the_latest_frame() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            async move { sinks.run_emitter(rx, flow, 1000, send).await }
        });
        fb.push(frame(2, 2, 1));
        tokio::time::sleep(Duration::from_millis(5)).await;
        fb.push(frame(2, 2, 2));
        tokio::time::sleep(Duration::from_millis(5)).await;
        // Both acknowledgements are lost (or the page slept); a still screen
        // sends no new frame.
        tokio::time::sleep(ACK_TIMEOUT + Duration::from_millis(100)).await;
        let after_reset = sent.lock().unwrap().clone();
        assert_eq!(
            after_reset,
            vec![1, 2, 2],
            "the latest frame is offered again"
        );
        assert!(!flow.is_dead());
        // The page acknowledges again: new frames flow.
        sinks.ack("s1", id, 3); // the frame offered again
        fb.push(frame(2, 2, 3));
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(sent.lock().unwrap().last(), Some(&3));
        sinks.unsubscribe("s1", id);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_freed_global_slot_wakes_a_waiting_view() {
        let sinks = FrameSinks::default();
        // Two views hold the whole global budget (2 + 2).
        let mut holders = Vec::new();
        for n in 0..2 {
            let (fb, rx) = FrameBuffer::new();
            let (id, flow) = sinks
                .subscribe(&format!("h{n}"), "p", FrameBuffer::new().0)
                .unwrap();
            let s2 = sinks.clone();
            tokio::spawn(async move { s2.run_emitter(rx, flow, 1000, |_| true).await });
            fb.push(frame(2, 2, 1));
            tokio::time::sleep(Duration::from_millis(5)).await;
            fb.push(frame(2, 2, 2));
            tokio::time::sleep(Duration::from_millis(5)).await;
            holders.push((id, fb));
        }
        assert_eq!(sinks.total_in_flight(), MAX_IN_FLIGHT_TOTAL);
        // A third view has one frame (a still stream: no more will come).
        let (fb, rx) = FrameBuffer::new();
        let (_, flow) = sinks.subscribe("w", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let s2 = sinks.clone();
        let waiting = tokio::spawn(async move { s2.run_emitter(rx, flow, 1000, send).await });
        fb.push(frame(2, 2, 7));
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(sent.lock().unwrap().is_empty(), "no global slot yet");
        // Another view's acknowledgement frees a slot: the waiting view sends.
        sinks.ack("h0", holders[0].0, 1);
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(*sent.lock().unwrap(), vec![7]);
        sinks.clear();
        waiting.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn acknowledged_frames_flow_and_the_newest_wins() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            async move { sinks.run_emitter(rx, flow, 1000, send).await }
        });
        fb.push(frame(2, 2, 1));
        tokio::time::sleep(Duration::from_millis(5)).await;
        fb.push(frame(2, 2, 2));
        tokio::time::sleep(Duration::from_millis(5)).await;
        // The page is busy: 3..=9 arrive, only the newest is kept.
        for pts in 3..10 {
            fb.push(frame(2, 2, pts));
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(*sent.lock().unwrap(), vec![1, 2]);
        sinks.ack("s1", id, 1);
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(*sent.lock().unwrap(), vec![1, 2, 9]);
        sinks.unsubscribe("s1", id);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn the_global_budget_holds_across_views() {
        let sinks = FrameSinks::default();
        let mut tasks = Vec::new();
        let mut buffers = Vec::new();
        let total: Sent = Arc::default();
        for n in 0..4 {
            let (fb, rx) = FrameBuffer::new();
            let (_, flow) = sinks
                .subscribe(&format!("s{n}"), "p", FrameBuffer::new().0)
                .unwrap();
            let t = total.clone();
            let sinks2 = sinks.clone();
            tasks.push(tokio::spawn(async move {
                sinks2
                    .run_emitter(rx, flow, 1000, move |b| {
                        if b.len() >= FRAME_HEADER_LEN {
                            t.lock().unwrap().push(pts_of(&b));
                        }
                        true
                    })
                    .await;
            }));
            buffers.push(fb);
        }
        for pts in 0..10 {
            for fb in &buffers {
                fb.push(frame(2, 2, pts));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(total.lock().unwrap().len(), MAX_IN_FLIGHT_TOTAL as usize);
        sinks.clear();
        for t in tasks {
            t.await.unwrap();
        }
        assert_eq!(sinks.total_in_flight(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn emitter_caps_the_frame_rate() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            async move { sinks.run_emitter(rx, flow, 10, send).await }
        });
        // An eager page that acknowledges at once; 100 frames in 100 ms.
        for pts in 0..100 {
            fb.push(frame(2, 2, pts));
            for seq in 1..=3 {
                sinks.ack("s1", id, seq);
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let count = sent.lock().unwrap().len();
        assert!(
            count <= 2,
            "10 fps cap: at most 2 frames in ~100 ms, got {count}"
        );
        sinks.unsubscribe("s1", id);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_slot_freed_right_after_a_failed_try_still_wakes_the_view() {
        let sinks = FrameSinks::default();
        // Two views hold the whole global budget (2 + 2).
        let mut holders = Vec::new();
        for n in 0..2 {
            let (fb, rx) = FrameBuffer::new();
            let (id, flow) = sinks
                .subscribe(&format!("h{n}"), "p", FrameBuffer::new().0)
                .unwrap();
            let s2 = sinks.clone();
            tokio::spawn(async move { s2.run_emitter(rx, flow, 1000, |_| true).await });
            fb.push(frame(2, 2, 1));
            tokio::time::sleep(Duration::from_millis(5)).await;
            fb.push(frame(2, 2, 2));
            tokio::time::sleep(Duration::from_millis(5)).await;
            holders.push((id, fb));
        }
        // The holder's acknowledgement lands exactly between the third
        // view's failed try and its wait (another thread in the app).
        let h0 = holders[0].0;
        let s2 = sinks.clone();
        let mut once = true;
        *sinks.reserve_failed_hook.lock().unwrap() = Some(Box::new(move || {
            if std::mem::take(&mut once) {
                s2.ack("h0", h0, 1);
            }
        }));
        let (fb, rx) = FrameBuffer::new();
        let (_, flow) = sinks.subscribe("w", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let s3 = sinks.clone();
        let waiting = tokio::spawn(async move { s3.run_emitter(rx, flow, 1000, send).await });
        fb.push(frame(2, 2, 7)); // a still stream: no more frames come
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(*sent.lock().unwrap(), vec![7], "the freed slot was seen");
        sinks.clear();
        waiting.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn an_acknowledgement_frees_only_a_frame_that_is_out() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let (sent, send) = recorder();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            async move { sinks.run_emitter(rx, flow, 1000, send).await }
        });
        for pts in 0..20 {
            fb.push(frame(2, 2, pts));
            // A page that acknowledges what it never got.
            sinks.ack("s1", id, 1000 + u32::try_from(pts).unwrap());
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(sent.lock().unwrap().len(), 2, "still two at most");
        assert_eq!(flow.in_flight(), 2);
        sinks.unsubscribe("s1", id);
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_page_gets_a_bounded_number_of_frames_then_the_view_ends() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (_, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let sent: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            let sent = sent.clone();
            async move {
                sinks
                    .run_emitter(rx, flow, 1000, move |b| {
                        sent.lock().unwrap().push(b);
                        true
                    })
                    .await;
            }
        });
        // A moving picture, and a page that takes nothing for a minute.
        for pts in 0..600 {
            fb.push(frame(2, 2, pts));
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        task.await.unwrap();
        let sent = sent.lock().unwrap();
        let frames = sent.iter().filter(|b| b.len() >= FRAME_HEADER_LEN).count();
        assert_eq!(frames, 4, "two, then two after the one reset");
        assert_eq!(sent.last().map(Vec::as_slice), Some(&FRAME_END_MAGIC[..]));
        assert!(flow.is_dead());
        assert_eq!(sinks.total_in_flight(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_view_ended_here_tells_the_page_one_the_page_ended_does_not() {
        let sinks = FrameSinks::default();
        let run = |sinks: &FrameSinks, stream: &str| {
            let (_fb, rx) = FrameBuffer::new();
            let (id, flow) = sinks.subscribe(stream, "p", FrameBuffer::new().0).unwrap();
            let got: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
            let g = got.clone();
            let s2 = sinks.clone();
            let task = tokio::spawn(async move {
                s2.run_emitter(rx, flow, 1000, move |b| {
                    g.lock().unwrap().push(b);
                    true
                })
                .await;
            });
            (id, got, task)
        };
        let (id, by_page, task) = run(&sinks, "a");
        sinks.unsubscribe("a", id);
        task.await.unwrap();
        assert_eq!(*by_page.lock().unwrap(), Vec::<Vec<u8>>::new());
        let (_, by_leave, task) = run(&sinks, "b");
        tokio::time::sleep(Duration::from_millis(5)).await;
        sinks.clear();
        task.await.unwrap();
        assert_eq!(*by_leave.lock().unwrap(), vec![FRAME_END_MAGIC.to_vec()]);
    }

    #[test]
    fn views_are_bounded_per_stream_and_in_total() {
        let sinks = FrameSinks::default();
        for _ in 0..MAX_VIEWS_PER_STREAM {
            sinks.subscribe("s", "p", FrameBuffer::new().0).unwrap();
        }
        assert_eq!(
            sinks.subscribe("s", "p", FrameBuffer::new().0).err(),
            Some(TOO_MANY_VIEWS)
        );
        for n in MAX_VIEWS_PER_STREAM..MAX_VIEWS {
            sinks
                .subscribe(&format!("o{n}"), "p", FrameBuffer::new().0)
                .unwrap();
        }
        assert_eq!(
            sinks.subscribe("x", "p", FrameBuffer::new().0).err(),
            Some(TOO_MANY_VIEWS)
        );
        // A new page instance starts fresh (the old views end).
        assert!(sinks.subscribe("x", "p2", FrameBuffer::new().0).is_ok());
    }

    /// A page that is alive but slow: it takes one frame every 4 s and
    /// acknowledges each when it takes it (acknowledgements of frames the
    /// window reset forgot arrive late). It never has more than four frames
    /// on the way, and it keeps getting pictures.
    #[tokio::test(start_paused = true)]
    async fn a_slow_page_never_has_more_than_four_frames_on_the_way() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0).unwrap();
        let inbox: Arc<Mutex<VecDeque<u32>>> = Arc::default();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            let inbox = inbox.clone();
            async move {
                sinks
                    .run_emitter(rx, flow, 1000, move |b| {
                        if b.len() >= FRAME_HEADER_LEN {
                            inbox.lock().unwrap().push_back(seq_of(&b));
                        }
                        true
                    })
                    .await;
            }
        });
        let (mut consumed, mut most_on_the_way) = (0usize, 0usize);
        // A moving picture (a frame every 100 ms) for two minutes.
        for tick in 1..=1200u32 {
            fb.push(frame(2, 2, u64::from(tick)));
            tokio::time::sleep(Duration::from_millis(100)).await;
            most_on_the_way = most_on_the_way.max(inbox.lock().unwrap().len());
            if tick % 40 == 0 {
                let taken = inbox.lock().unwrap().pop_front();
                if let Some(seq) = taken {
                    consumed += 1;
                    sinks.ack("s1", id, seq);
                }
            }
        }
        assert!(
            most_on_the_way <= 4,
            "at most four frames on the way, saw {most_on_the_way}"
        );
        assert!(
            consumed >= 25,
            "the slow page keeps getting pictures ({consumed})"
        );
        assert!(!flow.is_dead(), "an answering page keeps its view");
        sinks.unsubscribe("s1", id);
        task.await.unwrap();
    }

    /// Views that end here (a page that stopped answering) free their
    /// places: the page can open the stream again.
    #[tokio::test(start_paused = true)]
    async fn views_ended_here_free_their_places() {
        let sinks = FrameSinks::default();
        let mut tasks = Vec::new();
        let mut buffers = Vec::new();
        for _ in 0..MAX_VIEWS_PER_STREAM {
            let (fb, rx) = FrameBuffer::new();
            let (_, flow) = sinks.subscribe("s", "p", FrameBuffer::new().0).unwrap();
            let s2 = sinks.clone();
            tasks.push(tokio::spawn(async move {
                s2.run_emitter(rx, flow, 1000, |_| true).await;
            }));
            buffers.push(fb);
        }
        for pts in 0..10 {
            for fb in &buffers {
                fb.push(frame(2, 2, pts));
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // Nobody acknowledges anything: every view ends after its silence.
        for task in tasks {
            task.await.unwrap();
        }
        assert!(!sinks.has_sinks("s"));
        assert!(
            sinks.subscribe("s", "p", FrameBuffer::new().0).is_ok(),
            "the ended views hold no place"
        );
    }

    /// Frame transport measurement: one 1080p frame through the JSON path
    /// the app used before (`Channel<DecodedFrame>` = `serde_json`;
    /// `serde_bytes` planes become number arrays) vs. the raw framing. Run
    /// with `cargo test -p vc-client --release --lib bench_frame_transport
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement, not a test"]
    fn bench_frame_transport() {
        let mut f = (*frame(1920, 1080, 1)).clone();
        // Realistic content: varied bytes (JSON numbers get 1–3 digits).
        for (i, b) in f.y_plane.iter_mut().enumerate() {
            *b = (i * 7 % 251) as u8;
        }
        let rounds = 20u32;
        let t = std::time::Instant::now();
        let mut json_len = 0;
        for _ in 0..rounds {
            json_len = serde_json::to_vec(&f).unwrap().len();
        }
        let json = t.elapsed() / rounds;
        let t = std::time::Instant::now();
        let mut raw_len = 0;
        for _ in 0..rounds {
            raw_len = encode_frame(&f, 1).len();
        }
        let raw = t.elapsed() / rounds;
        println!(
            "1080p frame: json {json_len} bytes {json:?}/frame; raw {raw_len} bytes {raw:?}/frame"
        );
    }
}
