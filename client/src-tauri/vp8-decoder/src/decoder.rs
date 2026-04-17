//! Native VP8 decoder wrapping `env-libvpx-sys` FFI.
//!
//! The decoder consumes RTP packets via an internal [`Vp8Depacketizer`] and
//! yields decoded I420 YUV frames as [`DecodedFrame`] on complete frame
//! boundaries. All unsafe FFI calls are wrapped with `SAFETY:` comments
//! describing the ctx/iface/image pointer invariants.

// env-libvpx-sys is published on crates.io as `env-libvpx-sys` but declares
// `[lib] name = "vpx_sys"` in its Cargo.toml, so the importable crate is `vpx_sys`.
use vpx_sys::*;
use webrtc::rtp::packet::Packet as RtpPacket;

use crate::rtp_depacketizer::Vp8Depacketizer;

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

/// Native VP8 decoder wrapping `env-libvpx-sys` FFI.
///
/// The decoder owns a `vpx_codec_ctx_t` raw context and must be driven from a
/// single tokio task (not `Send + Sync`-safe in the general case, but safe
/// when task-bound — manual `Send` impl below documents that discipline).
pub struct Vp8VideoDecoder {
    ctx: vpx_codec_ctx_t,
    depacketizer: Vp8Depacketizer,
    stream_id: String,
}

// Safety: the vpx context is not thread-safe in general, but the decoder is
// constructed-used-dropped on a single tokio task. Manual Send impl is sound
// under that usage discipline.
unsafe impl Send for Vp8VideoDecoder {}

impl Vp8VideoDecoder {
    pub fn new(stream_id: String) -> Result<Self, VideoDecodeError> {
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
        })
    }

    /// Feed an RTP packet into the decoder. Returns zero or more decoded
    /// frames emitted on this call. An empty `Vec` is the common case —
    /// most packets are mid-frame fragments that do not yet complete a
    /// frame. On packet-level decode errors (typically reference frame
    /// loss), returns `Err(Decode(code))`; libvpx will resync on the next
    /// keyframe.
    pub fn process_packet(
        &mut self,
        packet: &RtpPacket,
    ) -> Result<Vec<DecodedFrame>, VideoDecodeError> {
        let Some(frame_bytes) = self.depacketizer.depacketize(packet) else {
            return Ok(Vec::new());
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
        let mut frames = Vec::new();
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
            frames.push(frame);
        }
        Ok(frames)
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
