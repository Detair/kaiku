//! Decoded frames from the VP8 decoder to the webview, with flow control.
//!
//! The decoder pushes into a `watch` channel per view (latest frame wins,
//! the decoder never waits). One emitter task per view sends frames over a
//! Tauri channel as raw bytes (`tauri::ipc::Response`, an `ArrayBuffer` in
//! the webview), not as JSON.
//!
//! Flow control (frame budget):
//! - per view at most [`MAX_IN_FLIGHT_PER_VIEW`] frames sent but not yet acknowledged by the page
//!   (`ack_video_frame`); a newer frame replaces an unsent one (latest wins);
//! - all views together at most [`MAX_IN_FLIGHT_TOTAL`] frames in flight;
//! - at most [`MAX_EMIT_FPS`] frames per second per view;
//! - a view whose page acknowledges nothing for [`ACK_TIMEOUT`] (page reloaded, window gone) ends,
//!   and every subscription of an older page instance ends when a new page instance subscribes.
//!
//! At 1080p (~3.2 MB per frame with decoder padding) that bounds the bytes
//! queued towards the webview at ~13 MB in total, whatever the page does.
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
//! | 40.. | Y, U, V planes |

use std::collections::HashMap;
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

/// Header size of [`encode_frame`].
pub const FRAME_HEADER_LEN: usize = 40;

/// Magic at the start of every frame.
pub const FRAME_MAGIC: &[u8; 4] = b"KVF1";

/// Raw framing of one decoded frame (see the module docs).
pub fn encode_frame(frame: &DecodedFrame) -> Vec<u8> {
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
    in_flight: AtomicU32,
    acked: Notify,
    /// Tokio time (pausable in tests).
    last_ack: Mutex<tokio::time::Instant>,
    dead: AtomicBool,
}

impl ViewFlow {
    fn new() -> Self {
        Self {
            in_flight: AtomicU32::new(0),
            acked: Notify::new(),
            last_ack: Mutex::new(tokio::time::Instant::now()),
            dead: AtomicBool::new(false),
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
    }

    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Acquire)
    }
}

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
}

impl FrameSinks {
    /// Add a view of `stream_id` for page instance `page`. Views of any
    /// other page instance end (the page was reloaded). Returns the
    /// subscription id and the view's flow state (for its emitter).
    pub fn subscribe(
        &self,
        stream_id: &str,
        page: &str,
        sink: FrameBuffer,
    ) -> (u64, Arc<ViewFlow>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let flow = Arc::new(ViewFlow::new());
        if let Ok(mut map) = self.inner.lock() {
            for views in map.values_mut() {
                for view in views.iter().filter(|v| v.page != page) {
                    view.flow.dead.store(true, Ordering::Release);
                    view.flow.acked.notify_one();
                }
                views.retain(|v| v.page == page);
            }
            map.retain(|_, views| !views.is_empty());
            map.entry(stream_id.to_string()).or_default().push(View {
                id,
                page: page.to_string(),
                buffer: Arc::new(sink),
                flow: flow.clone(),
            });
        }
        (id, flow)
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

    /// The page took a frame of this view.
    pub fn ack(&self, stream_id: &str, subscription_id: u64) {
        let Ok(map) = self.inner.lock() else { return };
        let Some(view) = map
            .get(stream_id)
            .and_then(|l| l.iter().find(|v| v.id == subscription_id))
        else {
            return;
        };
        if view
            .flow
            .in_flight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
            .is_ok()
        {
            let _ = self
                .total_in_flight
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1));
            self.global_freed.notify_waiters();
        }
        if let Ok(mut at) = view.flow.last_ack.lock() {
            *at = tokio::time::Instant::now();
        }
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
            if unsent.is_some()
                && flow.in_flight() < MAX_IN_FLIGHT_PER_VIEW
                && self.reserve_global()
            {
                let Some(frame) = unsent.take() else { continue };
                flow.in_flight.fetch_add(1, Ordering::AcqRel);
                if !send(encode_frame(&frame)) {
                    debug!("Frame channel closed; emitter ends");
                    break;
                }
                last_sent = Some(frame);
                tokio::time::sleep(interval).await;
                continue;
            }
            // A silence while frames are out (GC pause, slow render, sleep,
            // a lost acknowledgement): the window resets and the latest
            // frame is offered again. The view never ends for this.
            let waiting = flow.in_flight() > 0;
            if waiting
                && flow
                    .last_ack
                    .lock()
                    .is_ok_and(|at| at.elapsed() >= ACK_TIMEOUT)
            {
                debug!("No frame acknowledgement for a while; window reset");
                self.release(&flow);
                if let Ok(mut at) = flow.last_ack.lock() {
                    *at = tokio::time::Instant::now();
                }
                if unsent.is_none() {
                    unsent.clone_from(&last_sent);
                }
                continue;
            }
            let wants_global = unsent.is_some() && flow.in_flight() < MAX_IN_FLIGHT_PER_VIEW;
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    unsent.clone_from(&rx.borrow_and_update());
                }
                () = flow.acked.notified() => {}
                // Another view freed a slot of the global budget.
                () = self.global_freed.notified(), if wants_global => {}
                () = tokio::time::sleep(ACK_TIMEOUT), if waiting => {}
            }
        }
        flow.dead.store(true, Ordering::Release);
        self.release(&flow);
    }

    /// Return every slot `flow` holds to the budget and wake views that
    /// wait for the global budget.
    fn release(&self, flow: &ViewFlow) {
        let left = flow.in_flight.swap(0, Ordering::AcqRel);
        if left > 0 {
            let _ = self
                .total_in_flight
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    Some(n.saturating_sub(left))
                });
            self.global_freed.notify_waiters();
        }
    }

    /// Take one slot of the global budget.
    fn reserve_global(&self) -> bool {
        self.total_in_flight
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_IN_FLIGHT_TOTAL).then_some(n + 1)
            })
            .is_ok()
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

    #[test]
    fn framing_has_header_then_planes() {
        let f = frame(4, 2, 77);
        let b = encode_frame(&f);
        assert_eq!(&b[0..4], FRAME_MAGIC);
        assert_eq!(u32_at(&b, 4), 4);
        assert_eq!(u32_at(&b, 8), 2);
        assert_eq!(u32_at(&b, 12), 4);
        assert_eq!(u32_at(&b, 16), 2);
        assert_eq!(u32_at(&b, 20), 8);
        assert_eq!(u32_at(&b, 24), 2);
        assert_eq!(u32_at(&b, 28), 2);
        assert_eq!(pts_of(&b), 77);
        assert_eq!(&b[40..48], &[1; 8]);
        assert_eq!(&b[48..50], &[2; 2]);
        assert_eq!(&b[50..52], &[3; 2]);
        assert_eq!(b.len(), FRAME_HEADER_LEN + 12);
    }

    #[test]
    fn views_subscribe_and_unsubscribe_per_stream() {
        let sinks = FrameSinks::default();
        let (a, _ra) = FrameBuffer::new();
        let (b, _rb) = FrameBuffer::new();
        let (ida, _) = sinks.subscribe("s1", "page", a);
        let (idb, _) = sinks.subscribe("s1", "page", b);
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
        let (_, old) = sinks.subscribe("s1", "page-1", a);
        let (b, _rb) = FrameBuffer::new();
        sinks.subscribe("s2", "page-2", b);
        assert!(old.is_dead(), "the reloaded page's view is gone");
        assert!(!sinks.has_sinks("s1"));
        assert!(sinks.has_sinks("s2"));
    }

    type Sent = Arc<Mutex<Vec<u64>>>;

    fn recorder() -> (Sent, impl FnMut(Vec<u8>) -> bool) {
        let sent: Sent = Arc::default();
        let s = sent.clone();
        (sent, move |b: Vec<u8>| {
            s.lock().unwrap().push(pts_of(&b));
            true
        })
    }

    #[tokio::test(start_paused = true)]
    async fn a_page_that_does_not_take_frames_gets_at_most_two_and_the_view_stays() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0);
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
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0);
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
        sinks.ack("s1", id);
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
            let (id, flow) = sinks.subscribe(&format!("h{n}"), "p", FrameBuffer::new().0);
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
        let (_, flow) = sinks.subscribe("w", "p", FrameBuffer::new().0);
        let (sent, send) = recorder();
        let s2 = sinks.clone();
        let waiting = tokio::spawn(async move { s2.run_emitter(rx, flow, 1000, send).await });
        fb.push(frame(2, 2, 7));
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(sent.lock().unwrap().is_empty(), "no global slot yet");
        // Another view's acknowledgement frees a slot: the waiting view sends.
        sinks.ack("h0", holders[0].0);
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert_eq!(*sent.lock().unwrap(), vec![7]);
        sinks.clear();
        waiting.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn acknowledged_frames_flow_and_the_newest_wins() {
        let sinks = FrameSinks::default();
        let (fb, rx) = FrameBuffer::new();
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0);
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
        sinks.ack("s1", id);
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
            let (_, flow) = sinks.subscribe(&format!("s{n}"), "p", FrameBuffer::new().0);
            let t = total.clone();
            let sinks2 = sinks.clone();
            tasks.push(tokio::spawn(async move {
                sinks2
                    .run_emitter(rx, flow, 1000, move |b| {
                        t.lock().unwrap().push(pts_of(&b));
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
        let (id, flow) = sinks.subscribe("s1", "p", FrameBuffer::new().0);
        let (sent, send) = recorder();
        let task = tokio::spawn({
            let sinks = sinks.clone();
            let flow = flow.clone();
            async move { sinks.run_emitter(rx, flow, 10, send).await }
        });
        // An eager page that acknowledges at once; 100 frames in 100 ms.
        for pts in 0..100 {
            fb.push(frame(2, 2, pts));
            sinks.ack("s1", id);
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
            raw_len = encode_frame(&f).len();
        }
        let raw = t.elapsed() / rounds;
        println!(
            "1080p frame: json {json_len} bytes {json:?}/frame; raw {raw_len} bytes {raw:?}/frame"
        );
    }
}
