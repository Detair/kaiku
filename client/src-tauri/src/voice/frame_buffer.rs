//! Frame buffer bridging the VP8 decoder to the webview via Tauri Channel.
//!
//! Uses `tokio::sync::watch` internally so the decoder can always send without
//! backpressure — the watch channel retains only the latest value, giving
//! natural drop-oldest semantics (only the last frame matters for real-time
//! video display). The emitter task awakes on `changed()` and forwards the
//! latest frame via `tauri::ipc::Channel<DecodedFrame>` to the frontend.

use tauri::ipc::Channel;
use tokio::sync::watch;
use tracing::warn;

use crate::voice::video_decoder::DecodedFrame;

/// Sink-side handle for the decoder. Pushing a new frame replaces the
/// previously-unread frame (drop-oldest semantics).
pub struct FrameBuffer {
    tx: watch::Sender<Option<DecodedFrame>>,
}

impl FrameBuffer {
    /// Returns a new `FrameBuffer` and the receiver used by `spawn_frame_emitter`.
    pub fn new() -> (Self, watch::Receiver<Option<DecodedFrame>>) {
        let (tx, rx) = watch::channel(None);
        (Self { tx }, rx)
    }

    /// Push a decoded frame. Never blocks; replaces any previously-unread frame.
    /// A failed `send` means no receiver (e.g., emitter task exited) — safe to
    /// drop.
    pub fn push(&self, frame: DecodedFrame) {
        let _ = self.tx.send(Some(frame));
    }
}

/// Spawn the emitter task that forwards frames to a Tauri `Channel<T>`.
/// Stops when either the watch Sender is dropped (decoder gone) or the
/// Channel fails to send (webview closed).
pub fn spawn_frame_emitter(
    channel: Channel<DecodedFrame>,
    mut rx: watch::Receiver<Option<DecodedFrame>>,
) {
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            // borrow_and_update clones the frame out and marks the slot as seen.
            let maybe_frame = rx.borrow_and_update().clone();
            if let Some(frame) = maybe_frame {
                if let Err(e) = channel.send(frame) {
                    warn!(error = %e, "Failed to send frame over Tauri channel; emitter task exiting");
                    break;
                }
            }
        }
    });
}
