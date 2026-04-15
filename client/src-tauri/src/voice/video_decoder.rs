//! VP8 Video Decoder (Stub)
//!
//! Reads RTP packets from subscriber video tracks and emits lifecycle events
//! to the frontend. Actual VP8 frame decoding is not yet implemented — this
//! stub allows end-to-end pipeline testing while audio (the critical path)
//! is fully functional.
//!
//! Future implementation steps:
//! 1. VP8 RTP depacketization via `webrtc::rtp::codecs::vp8::Vp8Packet`
//! 2. VP8 frame decode to YUV via libvpx FFI (`vpx_codec_decode`)
//! 3. YUV → RGB conversion
//! 4. JPEG encode via `image` crate
//! 5. Base64 encode + emit as `VideoFrame` event
//! 6. Throttle to ~15 fps

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tauri::Emitter;
use tracing::{debug, info};
use webrtc::track::track_remote::TrackRemote;

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

/// Spawn a background task that reads RTP packets from a video track.
///
/// Currently a **stub**: reads and counts packets, emits track lifecycle
/// events, but does not decode VP8 frames or produce `VideoFrame` events.
pub fn spawn_video_decode_task(
    track: Arc<TrackRemote>,
    user_id: String,
    source_type: String,
    stream_id: String,
    app: tauri::AppHandle,
    active_count: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            user_id = %user_id,
            source_type = %source_type,
            stream_id = %stream_id,
            "Video decode task started"
        );

        // Emit track-started event so frontend can show the indicator
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

        // Read RTP packets from the track (drains the buffer even though we
        // don't decode yet — important so the SFU doesn't think we stalled).
        let mut buf = vec![0u8; 65_535];
        let mut packet_count: u64 = 0;

        loop {
            match track.read(&mut buf).await {
                Ok((rtp_packet, _attributes)) => {
                    let payload = &rtp_packet.payload[..];
                    if payload.is_empty() {
                        continue;
                    }

                    packet_count += 1;

                    // Log progress every ~10 seconds at 30 fps (300 packets)
                    if packet_count % 300 == 0 {
                        debug!(
                            user_id = %user_id,
                            packet_count,
                            payload_len = payload.len(),
                            "Video track receiving packets"
                        );
                    }

                    // TODO: VP8 decode pipeline
                    // 1. Depacketize RTP → complete VP8 frames
                    //    (webrtc::rtp::codecs::vp8::Vp8Packet)
                    // 2. Decode VP8 → YUV (libvpx via vpx-encode FFI)
                    // 3. Convert YUV → RGB
                    // 4. JPEG encode (image crate)
                    // 5. Base64 encode
                    // 6. Emit VideoFrame event
                    // 7. Throttle to ~15 fps
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
// VP8 native decoder (added by Task D4)
//
// The `Vp8VideoDecoder` below is a safe wrapper around env-libvpx-sys FFI. D5
// will wire it into `spawn_video_decode_task` and introduce a `FrameBuffer`.
// For now, the type is exposed publicly so D5 can consume it.
// =============================================================================

use crate::voice::rtp_depacketizer::Vp8Depacketizer;
use env_libvpx_sys::*;
use tokio::sync::mpsc;
use webrtc::rtp::packet::Packet as RtpPacket;

#[derive(Debug, thiserror::Error)]
pub enum VideoDecodeError {
    #[error("VP8 decoder init failed: code {0}")]
    Init(u32),
    #[error("VP8 decode failed: code {0}")]
    Decode(u32),
    #[error("frame sink closed")]
    FrameSinkClosed,
}

/// A fully decoded YUV I420 video frame.
#[derive(Debug)]
#[allow(dead_code)] // consumed by D5 (frame sink → Tauri Channel) and D6 (WebGL renderer)
pub struct DecodedFrame {
    pub stream_id: String,
    pub width: u32,
    pub height: u32,
    pub y_plane: Vec<u8>,
    pub u_plane: Vec<u8>,
    pub v_plane: Vec<u8>,
    pub y_stride: u32,
    pub uv_stride: u32,
    pub pts_ms: u64,
}

/// Native VP8 decoder wrapping env-libvpx-sys FFI.
///
/// The decoder owns a `vpx_codec_ctx_t` raw context and must be driven from a
/// single tokio task (not `Send + Sync`-safe in the general case, but safe
/// when task-bound — marked `unsafe impl Send` for this use pattern).
#[allow(dead_code)] // wired into spawn_video_decode_task by D5
pub struct Vp8VideoDecoder {
    ctx: vpx_codec_ctx_t,
    depacketizer: Vp8Depacketizer,
    stream_id: String,
    frame_sink: mpsc::Sender<DecodedFrame>,
}

// Safety: the vpx context is not thread-safe in general, but the decoder is
// constructed-used-dropped on a single tokio task. Manual Send impl is sound
// under that usage discipline. D5 preserves this invariant.
unsafe impl Send for Vp8VideoDecoder {}

#[allow(dead_code)] // public API consumed by D5
impl Vp8VideoDecoder {
    pub fn new(
        stream_id: String,
        frame_sink: mpsc::Sender<DecodedFrame>,
    ) -> Result<Self, VideoDecodeError> {
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
            if self.frame_sink.send(frame).await.is_err() {
                return Err(VideoDecodeError::FrameSinkClosed);
            }
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
