//! VP8 RTP depacketizer + native libvpx decoder.
//!
//! This crate is the designated FFI boundary between the Kaiku Tauri client
//! and libvpx (via [`env-libvpx-sys`]). It is factored out of the main
//! `vc-client` crate so the unsafe FFI wrappers live under a locally relaxed
//! `unsafe_code = "allow"` lint while the rest of the workspace keeps
//! `unsafe_code = "forbid"`.
//!
//! # Public API
//!
//! - [`Vp8Depacketizer`] — reassembles VP8 frames from an ordered RTP stream.
//! - [`Vp8VideoDecoder`] — decodes complete VP8 frames to I420 YUV planes.
//! - [`DecodedFrame`] — owned I420 frame with planes + strides.
//! - [`VideoDecodeError`] — error type for init/decode failures.
//!
//! # Callback-free decoder API
//!
//! [`Vp8VideoDecoder::process_packet`] returns a `Vec<DecodedFrame>` rather
//! than pushing into a caller-owned sink. This keeps the sub-crate free of
//! Tauri (`FrameBuffer`, `Channel`) or Tokio dependencies and cleanly
//! separates decode from delivery. Callers iterate the returned frames and
//! forward them to whatever sink they use.

mod decoder;
mod rtp_depacketizer;

pub use decoder::{DecodedFrame, VideoDecodeError, Vp8VideoDecoder};
pub use rtp_depacketizer::Vp8Depacketizer;
