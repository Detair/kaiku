//! Decoded frames from the VP8 decoder to the webview.
//!
//! The decoder pushes into a `watch` channel (latest frame wins, the decoder
//! never waits). One emitter task per subscription sends the latest frame
//! over a Tauri channel as raw bytes (`tauri::ipc::Response`, an
//! `ArrayBuffer` in the webview) — not as JSON — at most
//! [`MAX_EMIT_FPS`] frames per second.
//!
//! Wire layout of one frame (little endian), see [`encode_frame`]:
//!
//! | bytes | field |
//! |---|---|
//! | 0..4 | magic `KVF1` |
//! | 4..8 | width |
//! | 8..12 | height |
//! | 12..16 | y_stride |
//! | 16..20 | uv_stride |
//! | 20..24 | y length |
//! | 24..28 | u length |
//! | 28..32 | v length |
//! | 32..40 | pts (ms) |
//! | 40.. | Y, U, V planes |

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::ipc::{Channel, Response};
use tokio::sync::watch;
use tracing::debug;

use crate::voice::video_decoder::DecodedFrame;

/// Frames per second sent to one view at most.
pub const MAX_EMIT_FPS: u32 = 30;

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

/// Sink-side handle for the decoder (latest frame wins).
pub struct FrameBuffer {
    tx: watch::Sender<Option<DecodedFrame>>,
}

impl FrameBuffer {
    pub fn new() -> (Self, watch::Receiver<Option<DecodedFrame>>) {
        let (tx, rx) = watch::channel(None);
        (Self { tx }, rx)
    }

    pub fn push(&self, frame: DecodedFrame) {
        let _ = self.tx.send(Some(frame));
    }
}

/// Where decoded frames go: per stream id, every open view (subscription).
/// The decode task looks the views up for every frame, so a view opened
/// late, re-opened after a layout change, or closed is seen at once.
#[derive(Clone, Default)]
pub struct FrameSinks {
    inner: Arc<Mutex<HashMap<String, Vec<(u64, Arc<FrameBuffer>)>>>>,
    next_id: Arc<AtomicU64>,
}

impl FrameSinks {
    /// Add a view of `stream_id`; returns its subscription id.
    pub fn subscribe(&self, stream_id: &str, sink: FrameBuffer) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut map) = self.inner.lock() {
            map.entry(stream_id.to_string())
                .or_default()
                .push((id, Arc::new(sink)));
        }
        id
    }

    /// Remove one view. Dropping its buffer ends its emitter task.
    pub fn unsubscribe(&self, stream_id: &str, subscription_id: u64) -> bool {
        let Ok(mut map) = self.inner.lock() else {
            return false;
        };
        let Some(list) = map.get_mut(stream_id) else {
            return false;
        };
        let before = list.len();
        list.retain(|(id, _)| *id != subscription_id);
        let removed = list.len() != before;
        if list.is_empty() {
            map.remove(stream_id);
        }
        removed
    }

    /// The views of `stream_id` right now.
    pub fn sinks_for(&self, stream_id: &str) -> Vec<Arc<FrameBuffer>> {
        self.inner
            .lock()
            .ok()
            .and_then(|map| {
                map.get(stream_id)
                    .map(|l| l.iter().map(|(_, s)| s.clone()).collect())
            })
            .unwrap_or_default()
    }

    pub fn has_sinks(&self, stream_id: &str) -> bool {
        self.inner
            .lock()
            .is_ok_and(|map| map.get(stream_id).is_some_and(|l| !l.is_empty()))
    }

    /// Drop every view (voice leave).
    pub fn clear(&self) {
        if let Ok(mut map) = self.inner.lock() {
            map.clear();
        }
    }
}

/// Forward the latest frame to `send` at most `max_fps` times per second,
/// until the buffer is dropped or `send` fails.
pub async fn run_emitter<F>(
    mut rx: watch::Receiver<Option<DecodedFrame>>,
    max_fps: u32,
    mut send: F,
) where
    F: FnMut(Vec<u8>) -> bool,
{
    let interval = Duration::from_secs(1) / max_fps.max(1);
    while rx.changed().await.is_ok() {
        let frame = rx.borrow_and_update().clone();
        if let Some(frame) = frame {
            if !send(encode_frame(&frame)) {
                debug!("Frame channel closed; emitter ends");
                break;
            }
            // Frames arriving meanwhile replace each other; the newest is
            // sent after the pause.
            tokio::time::sleep(interval).await;
        }
    }
}

/// Emitter task for one Tauri channel (raw bytes).
pub fn spawn_frame_emitter(channel: Channel<Response>, rx: watch::Receiver<Option<DecodedFrame>>) {
    tokio::spawn(run_emitter(rx, MAX_EMIT_FPS, move |bytes| {
        channel.send(Response::new(bytes)).is_ok()
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, pts: u64) -> DecodedFrame {
        let (yw, uvw) = (w as usize, (w as usize).div_ceil(2));
        let uvh = (h as usize).div_ceil(2);
        DecodedFrame {
            stream_id: "s".into(),
            width: w,
            height: h,
            y_plane: vec![1; yw * h as usize],
            u_plane: vec![2; uvw * uvh],
            v_plane: vec![3; uvw * uvh],
            y_stride: w,
            uv_stride: w.div_ceil(2),
            pts_ms: pts,
        }
    }

    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
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
        assert_eq!(u64::from_le_bytes(b[32..40].try_into().unwrap()), 77);
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
        let ida = sinks.subscribe("s1", a);
        let idb = sinks.subscribe("s1", b);
        assert_ne!(ida, idb);
        assert_eq!(sinks.sinks_for("s1").len(), 2);
        assert!(sinks.unsubscribe("s1", ida));
        assert!(!sinks.unsubscribe("s1", ida), "only once");
        assert_eq!(sinks.sinks_for("s1").len(), 1);
        assert!(sinks.unsubscribe("s1", idb));
        assert!(!sinks.has_sinks("s1"));
    }

    #[tokio::test(start_paused = true)]
    async fn emitter_caps_the_frame_rate_and_sends_the_newest() {
        let (fb, rx) = FrameBuffer::new();
        let sent = Arc::new(Mutex::new(Vec::<u64>::new()));
        let sink = sent.clone();
        let task = tokio::spawn(run_emitter(rx, 10, move |bytes| {
            sink.lock()
                .unwrap()
                .push(u64::from_le_bytes(bytes[32..40].try_into().unwrap()));
            true
        }));
        // 100 frames within one 100 ms slot after the first.
        fb.push(frame(2, 2, 0));
        tokio::task::yield_now().await;
        for pts in 1..=100 {
            fb.push(frame(2, 2, pts));
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        drop(fb);
        let _ = task.await;
        let sent = sent.lock().unwrap().clone();
        assert_eq!(sent, vec![0, 100], "one frame per slot, the newest");
    }

    /// C11 measurement: one 1080p frame through the JSON path the app used
    /// (`Channel<DecodedFrame>` = `serde_json`; `serde_bytes` planes become
    /// number arrays) vs. the raw framing. Run with
    /// `cargo test -p vc-client --release --lib bench_frame_transport -- --ignored --nocapture`.
    #[test]
    #[ignore = "measurement, not a test"]
    fn bench_frame_transport() {
        use std::time::Instant;
        let mut f = frame(1920, 1080, 1);
        // Realistic content: varied bytes (JSON numbers get 1–3 digits).
        for (i, b) in f.y_plane.iter_mut().enumerate() {
            *b = (i * 7 % 251) as u8;
        }
        let rounds = 20u32;
        let t = Instant::now();
        let mut json_len = 0;
        for _ in 0..rounds {
            json_len = serde_json::to_vec(&f).unwrap().len();
        }
        let json = t.elapsed() / rounds;
        let t = Instant::now();
        let mut raw_len = 0;
        for _ in 0..rounds {
            raw_len = encode_frame(&f).len();
        }
        let raw = t.elapsed() / rounds;
        println!("C11 1080p frame: json {json_len} bytes {json:?}/frame; raw {raw_len} bytes {raw:?}/frame");
        let json_text = String::from_utf8(serde_json::to_vec(&f).unwrap()).unwrap();
        std::fs::write(std::env::temp_dir().join("kaiku-c11-frame.json"), json_text).unwrap();
    }
}
