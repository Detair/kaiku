//! VP8 Video Decoder
//!
//! Reads RTP packets from subscriber video tracks, depacketizes VP8 frames,
//! decodes them into YUV (I420) via libvpx, and pushes the result through a
//! `FrameBuffer` (drop-oldest watch channel) to the webview. The frontend
//! must call `subscribe_video_frames(stream_id, channel)` to register a
//! sink — until it does, the decode task drains incoming packets without
//! decoding (so the SFU does not see a stalled receiver).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use webrtc::track::track_remote::TrackRemote;

use crate::voice::frame_buffer::FrameBuffer;

/// Maximum concurrent video decode streams.
///
/// Caps CPU/memory usage — excess tracks still receive metadata events
/// so the UI can show indicators, but packets are not read.
pub const MAX_VIDEO_STREAMS: usize = 2;

/// Video frame payload for frontend (not yet emitted by stub).
#[derive(Clone, serde::Serialize)]
pub struct VideoFrame {
    pub user_id: String,
    pub stream_id: String,
    pub source_type: String,
    pub width: u32,
    pub height: u32,
    /// Base64-encoded JPEG data.
    pub data: String,
}

/// Video track metadata event sent to frontend on track start/stop.
#[derive(Clone, serde::Serialize)]
pub struct VideoTrackEvent {
    pub user_id: String,
    pub stream_id: String,
    pub source_type: String,
}

/// Maximum time to wait for a frontend `subscribe_video_frames` call before
/// giving up and treating the track as drain-only. Five seconds covers tile
/// mount latency on slow hardware while bounding wasted decode setup.
const SUBSCRIPTION_LOOKUP_DEADLINE: Duration = Duration::from_secs(5);

/// Spawn a background task that reads RTP packets from a video track,
/// depacketizes them, and feeds the resulting VP8 frames into a
/// `Vp8VideoDecoder` once the frontend registers a `FrameBuffer` for the
/// stream via `subscribe_video_frames`.
///
/// If no subscription arrives within `SUBSCRIPTION_LOOKUP_DEADLINE` after the
/// first packet, the task continues to drain packets without decoding so the
/// SFU does not detect a stalled receiver.
pub fn spawn_video_decode_task(
    track: Arc<TrackRemote>,
    user_id: String,
    source_type: String,
    stream_id: String,
    app: tauri::AppHandle,
    active_count: Arc<AtomicUsize>,
    video_frame_sinks: Arc<Mutex<HashMap<String, FrameBuffer>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            user_id = %user_id,
            source_type = %source_type,
            stream_id = %stream_id,
            "Video decode task started"
        );

        // Emit track-started event so frontend can show the indicator and
        // (for screen shares) call subscribe_video_frames in response.
        let start_event = if source_type.starts_with("screen_video") {
            "voice:screen_share_track"
        } else {
            "voice:webcam_track"
        };
        let _ = app.emit(
            start_event,
            VideoTrackEvent {
                user_id: user_id.clone(),
                stream_id: stream_id.clone(),
                source_type: source_type.clone(),
            },
        );

        let mut buf = vec![0u8; 65_535];
        let mut packet_count: u64 = 0;
        let mut decoder: Option<Vp8VideoDecoder> = None;
        // We start the deadline window when the first packet arrives, not at
        // task start — otherwise tracks that take a moment to deliver any
        // packets at all could miss the window.
        let mut subscription_deadline: Option<Instant> = None;
        let mut decoder_init_failed = false;

        loop {
            match track.read(&mut buf).await {
                Ok((rtp_packet, _attributes)) => {
                    let payload = &rtp_packet.payload[..];
                    if payload.is_empty() {
                        continue;
                    }

                    packet_count += 1;

                    if packet_count % 300 == 0 {
                        debug!(
                            user_id = %user_id,
                            packet_count,
                            payload_len = payload.len(),
                            decoder_active = decoder.is_some(),
                            "Video track receiving packets"
                        );
                    }

                    // Lazy decoder activation: once a frontend subscription
                    // appears in the shared map, claim the FrameBuffer and
                    // build a decoder. Stop polling after the deadline or
                    // after a previous init failure to avoid hammering the
                    // mutex on every packet for the rest of the track's life.
                    if decoder.is_none() && !decoder_init_failed {
                        let deadline = *subscription_deadline
                            .get_or_insert_with(|| Instant::now() + SUBSCRIPTION_LOOKUP_DEADLINE);
                        if Instant::now() < deadline {
                            let mut sinks = video_frame_sinks.lock().await;
                            if let Some(fb) = sinks.remove(&stream_id) {
                                drop(sinks);
                                match Vp8VideoDecoder::new(stream_id.clone(), fb) {
                                    Ok(d) => {
                                        info!(
                                            stream_id = %stream_id,
                                            user_id = %user_id,
                                            "VP8 decoder activated for stream"
                                        );
                                        decoder = Some(d);
                                    }
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            stream_id = %stream_id,
                                            "VP8 decoder init failed; falling back to drain-only"
                                        );
                                        decoder_init_failed = true;
                                    }
                                }
                            }
                        } else {
                            // Past deadline without a subscriber — log once
                            // and stop checking by treating as init-failed.
                            info!(
                                stream_id = %stream_id,
                                user_id = %user_id,
                                "No frame subscription within deadline; draining track"
                            );
                            decoder_init_failed = true;
                        }
                    }

                    if let Some(d) = decoder.as_mut() {
                        if let Err(e) = d.process_packet(&rtp_packet).await {
                            // Decode errors are typically recoverable
                            // (e.g., reference frame loss). Log and continue
                            // — libvpx will resync on the next keyframe.
                            warn!(
                                error = %e,
                                stream_id = %stream_id,
                                packet_count,
                                "VP8 decode error; continuing"
                            );
                        }
                    }
                    // No decoder: packet already consumed from the buffer,
                    // which is the SFU-friendly drain behavior we want.
                }
                Err(e) => {
                    info!(
                        user_id = %user_id,
                        error = %e,
                        packet_count,
                        "Video track ended"
                    );
                    break;
                }
            }
        }

        // Drop the decoder before emitting removal so vpx_codec_destroy runs
        // first (Drop on Vp8VideoDecoder), keeping teardown ordering clean.
        drop(decoder);

        // If we had registered a sink but never picked it up, remove the
        // stale entry so the next stream with the same ID doesn't reuse it.
        video_frame_sinks.lock().await.remove(&stream_id);

        // Emit track-removed event so frontend hides the indicator
        let remove_event = if source_type.starts_with("screen_video") {
            "voice:screen_share_track_removed"
        } else {
            "voice:webcam_track_removed"
        };
        let _ = app.emit(
            remove_event,
            VideoTrackEvent {
                user_id: user_id.clone(),
                stream_id: stream_id.clone(),
                source_type: source_type.clone(),
            },
        );

        active_count.fetch_sub(1, Ordering::Relaxed);
        debug!(packet_count, "Video decode task ended");
    })
}

// =============================================================================
// VP8 native decoder (FFI wrapper around env-libvpx-sys).
// =============================================================================

use crate::voice::rtp_depacketizer::Vp8Depacketizer;
use env_libvpx_sys::*;
use webrtc::rtp::packet::Packet as RtpPacket;

#[derive(Debug, thiserror::Error)]
pub enum VideoDecodeError {
    #[error("VP8 decoder init failed: code {0}")]
    Init(u32),
    #[error("VP8 decode failed: code {0}")]
    Decode(u32),
}

/// A fully decoded YUV I420 video frame.
///
/// `Serialize` (with `serde_bytes` on the planes) is required for transport
/// over Tauri's `Channel<T>` IPC: planes serialize as binary blobs
/// (`Uint8Array` in the webview) instead of JSON number arrays.
#[derive(serde::Serialize, Clone)]
pub struct DecodedFrame {
    pub stream_id: String,
    pub width: u32,
    pub height: u32,
    #[serde(with = "serde_bytes")]
    pub y_plane: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub u_plane: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub v_plane: Vec<u8>,
    pub y_stride: u32,
    pub uv_stride: u32,
    pub pts_ms: u64,
}

// Manual Debug impl: never dump plane bytes (multi-MB) to logs — print sizes only.
impl std::fmt::Debug for DecodedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecodedFrame")
            .field("stream_id", &self.stream_id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("y_plane.len", &self.y_plane.len())
            .field("u_plane.len", &self.u_plane.len())
            .field("v_plane.len", &self.v_plane.len())
            .field("y_stride", &self.y_stride)
            .field("uv_stride", &self.uv_stride)
            .field("pts_ms", &self.pts_ms)
            .finish()
    }
}

/// Native VP8 decoder wrapping env-libvpx-sys FFI.
///
/// The decoder owns a `vpx_codec_ctx_t` raw context and must be driven from a
/// single tokio task (not `Send + Sync`-safe in the general case, but safe
/// when task-bound — marked `unsafe impl Send` for this use pattern).
pub struct Vp8VideoDecoder {
    ctx: vpx_codec_ctx_t,
    depacketizer: Vp8Depacketizer,
    stream_id: String,
    frame_sink: FrameBuffer,
}

// Safety: the vpx context is not thread-safe in general, but the decoder is
// constructed-used-dropped on a single tokio task. Manual Send impl is sound
// under that usage discipline. D5 preserves this invariant.
unsafe impl Send for Vp8VideoDecoder {}

impl Vp8VideoDecoder {
    pub fn new(stream_id: String, frame_sink: FrameBuffer) -> Result<Self, VideoDecodeError> {
        // SAFETY: vpx_codec_ctx_t is a C struct that vpx fills in via
        // vpx_codec_dec_init_ver. Zero-initialization before the init call is
        // the documented pattern (matches `Default for vpx_codec_ctx` provided
        // by env-libvpx-sys itself).
        let mut ctx: vpx_codec_ctx_t = unsafe { std::mem::zeroed() };
        // SAFETY: vpx_codec_vp8_dx returns a static pointer to the VP8
        // decoder interface; no preconditions and no ownership transfer.
        let iface = unsafe { vpx_codec_vp8_dx() };
        // SAFETY: ctx is a valid (zeroed) destination. iface comes from the
        // vpx static accessor above. cfg=null and flags=0 select defaults
        // (RFC-default decoder config). ABI version matches the headers the
        // bindings were generated against.
        let res = unsafe {
            vpx_codec_dec_init_ver(
                &mut ctx,
                iface,
                std::ptr::null(),
                0,
                VPX_DECODER_ABI_VERSION as i32,
            )
        };
        if res != VPX_CODEC_OK {
            return Err(VideoDecodeError::Init(res as u32));
        }
        Ok(Self {
            ctx,
            depacketizer: Vp8Depacketizer::new(),
            stream_id,
            frame_sink,
        })
    }

    /// Feed an RTP packet into the decoder. On a complete VP8 frame, decode
    /// and emit one or more `DecodedFrame`s via the frame sink.
    pub async fn process_packet(&mut self, packet: &RtpPacket) -> Result<(), VideoDecodeError> {
        let Some(frame_bytes) = self.depacketizer.depacketize(packet) else {
            return Ok(());
        };

        // SAFETY: ctx was successfully initialized in `new` and has not been
        // destroyed (Drop runs only when the owning task ends). The data
        // pointer + length describe a valid Rust slice for the duration of
        // the call (libvpx copies/consumes synchronously). user_priv=null
        // and deadline=0 mean "no caller context, decode immediately".
        let res = unsafe {
            vpx_codec_decode(
                &mut self.ctx,
                frame_bytes.as_ptr(),
                frame_bytes.len() as u32,
                std::ptr::null_mut(),
                0,
            )
        };
        if res != VPX_CODEC_OK {
            return Err(VideoDecodeError::Decode(res as u32));
        }

        // Iterate decoded frames via vpx_codec_get_frame.
        let mut iter: vpx_codec_iter_t = std::ptr::null();
        loop {
            // SAFETY: ctx is live (see above). iter is initialized to null
            // per the API contract for the first call and is mutated by
            // libvpx between iterations. Returns null when the queue drains.
            let img = unsafe { vpx_codec_get_frame(&mut self.ctx, &mut iter) };
            if img.is_null() {
                break;
            }
            // SAFETY: vpx_codec_get_frame returned non-null, so img points
            // to a valid vpx_image_t whose planes/strides are valid until
            // the next call to vpx_codec_get_frame (or vpx_codec_decode /
            // vpx_codec_destroy). copy_image_to_decoded_frame copies all
            // pixel data into owned Vecs before returning, so the borrow
            // does not outlive the next FFI call.
            let frame = unsafe { copy_image_to_decoded_frame(img, &self.stream_id) };
            // FrameBuffer is a watch channel: push never blocks and silently
            // drops the previous unread frame (drop-oldest). A failed send
            // means the emitter task has exited (webview gone) — safe to ignore.
            self.frame_sink.push(frame);
        }
        Ok(())
    }
}

impl Drop for Vp8VideoDecoder {
    fn drop(&mut self) {
        // SAFETY: ctx was successfully initialized in `new` (constructor
        // returns Err otherwise, in which case Drop never runs). Destroying
        // exactly once on drop is the documented teardown.
        unsafe {
            vpx_codec_destroy(&mut self.ctx);
        }
    }
}

/// Copy Y/U/V planes from a borrowed `vpx_image_t` into owned `Vec<u8>`s.
/// The image is invalidated by the next `vpx_codec_get_frame`, so copying
/// is mandatory.
///
/// # Safety
/// `img` must be a non-null pointer to a valid `vpx_image_t` with properly
/// initialized planes and strides as returned from `vpx_codec_get_frame`.
unsafe fn copy_image_to_decoded_frame(img: *const vpx_image_t, stream_id: &str) -> DecodedFrame {
    let img = &*img;
    let width = img.d_w;
    let height = img.d_h;

    // I420: Y is full-size, U/V are half-size (4:2:0 subsampling).
    let y_stride = img.stride[0] as u32;
    let uv_stride = img.stride[1] as u32;

    let y_size = (y_stride * height) as usize;
    let uv_size = (uv_stride * (height / 2)) as usize;

    let y_plane = std::slice::from_raw_parts(img.planes[0], y_size).to_vec();
    let u_plane = std::slice::from_raw_parts(img.planes[1], uv_size).to_vec();
    let v_plane = std::slice::from_raw_parts(img.planes[2], uv_size).to_vec();

    DecodedFrame {
        stream_id: stream_id.to_string(),
        width,
        height,
        y_plane,
        u_plane,
        v_plane,
        y_stride,
        uv_stride,
        pts_ms: 0,
    }
}
