//! Video Encoder
//!
//! VP9 software encoding via `vpx-encode`.

use tracing::{debug, warn};
use vpx_encode::{Config, Encoder as VpxEncoder, VideoCodecId};

use super::{EncodedPacket, QualityParams, VideoError};
use crate::capture::I420Frame;

/// Trait for video encoders (allows future H.264 fallback).
///
/// Note: Not `Send` because `vpx_encode::Encoder` contains raw pointers.
/// The encoder must be used on a single task (which is the case in our pipeline).
#[allow(dead_code)]
pub trait VideoEncoder {
    /// Encode a single I420 frame. May return zero or more packets.
    fn encode(&mut self, frame: &I420Frame) -> Result<Vec<EncodedPacket>, VideoError>;

    /// MIME type for WebRTC codec capability.
    fn codec_mime(&self) -> &str;

    /// RTP payload type number.
    fn payload_type(&self) -> u8;
}

/// VP9 encoder using libvpx.
///
/// Note: `VpxEncoder` contains raw pointers and is not `Send`.
/// This encoder must be created and used on a single thread
/// (e.g. via `tokio::task::spawn_blocking`).
pub struct Vp9Encoder {
    encoder: VpxEncoder,
    frame_count: u64,
    fps: u32,
    i420_buf: Vec<u8>,
}

impl Vp9Encoder {
    /// Create a new VP9 encoder with the given quality parameters.
    pub fn new(params: &QualityParams) -> Result<Self, VideoError> {
        let config = Config {
            width: params.width,
            height: params.height,
            timebase: [1, params.fps as i32],
            bitrate: params.bitrate_kbps,
            codec: VideoCodecId::VP9,
        };

        let encoder = VpxEncoder::new(config)
            .map_err(|e| VideoError::InitFailed(format!("VP9 encoder: {e}")))?;

        debug!(
            width = params.width,
            height = params.height,
            fps = params.fps,
            bitrate = params.bitrate_kbps,
            "VP9 encoder initialized"
        );

        let i420_buf = Vec::with_capacity((params.width * params.height * 3 / 2) as usize);

        Ok(Self {
            encoder,
            frame_count: 0,
            fps: params.fps,
            i420_buf,
        })
    }
}

impl VideoEncoder for Vp9Encoder {
    fn encode(&mut self, frame: &I420Frame) -> Result<Vec<EncodedPacket>, VideoError> {
        // 90kHz clock timestamp
        let pts_90khz = self.frame_count * 90000 / u64::from(self.fps);

        // vpx-encode expects a single contiguous I420 buffer: Y + U + V
        // Reuse pre-allocated buffer to avoid per-frame allocation
        self.i420_buf.clear();
        self.i420_buf.extend_from_slice(&frame.y);
        self.i420_buf.extend_from_slice(&frame.u);
        self.i420_buf.extend_from_slice(&frame.v);

        let packets = self
            .encoder
            .encode(pts_90khz as i64, &self.i420_buf)
            .map_err(|e| VideoError::EncodeFailed(format!("VP9 encode: {e}")))?;

        self.frame_count += 1;

        let result: Vec<EncodedPacket> = packets
            .map(|pkt| EncodedPacket {
                data: pkt.data.to_vec(),
                is_keyframe: pkt.key,
                pts: pts_90khz,
            })
            .collect();

        if result.is_empty() && self.frame_count % u64::from(self.fps) == 0 {
            warn!("No encoded packets for frame {}", self.frame_count);
        }

        Ok(result)
    }

    fn codec_mime(&self) -> &'static str {
        "video/VP9"
    }

    fn payload_type(&self) -> u8 {
        98 // Matches server sfu.rs VP9 payload type
    }
}
