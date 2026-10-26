//! Video Encoder
//!
//! VP8 software encoding via `vpx-encode`.

use tracing::{debug, info, warn};
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

/// VP8 encoder using libvpx.
///
/// The encoder follows the size of the frames it is given: when a frame's
/// size differs from the current one (window resized, webcam switched
/// resolution) libvpx is re-created at the new size and starts with a
/// keyframe. Frame and bitrate settings come from the quality tier.
///
/// Note: `VpxEncoder` contains raw pointers and is not `Send`.
/// This encoder must be created and used on a single thread
/// (e.g. via `tokio::task::spawn_blocking`).
pub struct Vp8Encoder {
    encoder: VpxEncoder,
    width: u32,
    height: u32,
    frame_count: u64,
    fps: u32,
    bitrate_kbps: u32,
    i420_buf: Vec<u8>,
}

impl Vp8Encoder {
    /// Create a new VP8 encoder with the given quality parameters.
    pub fn new(params: &QualityParams) -> Result<Self, VideoError> {
        let encoder = Self::build(params.width, params.height, params.fps, params.bitrate_kbps)?;

        Ok(Self {
            encoder,
            width: params.width,
            height: params.height,
            frame_count: 0,
            fps: params.fps,
            bitrate_kbps: params.bitrate_kbps,
            i420_buf: Vec::with_capacity((params.width * params.height * 3 / 2) as usize),
        })
    }

    /// Current encode size `(width, height)`.
    #[cfg(test)]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn build(
        width: u32,
        height: u32,
        fps: u32,
        bitrate_kbps: u32,
    ) -> Result<VpxEncoder, VideoError> {
        let config = Config {
            width,
            height,
            timebase: [1, fps as i32],
            bitrate: bitrate_kbps,
            codec: VideoCodecId::VP8,
        };

        let encoder = VpxEncoder::new(config)
            .map_err(|e| VideoError::InitFailed(format!("VP8 encoder: {e}")))?;

        debug!(
            width,
            height,
            fps,
            bitrate = bitrate_kbps,
            "VP8 encoder initialized"
        );
        Ok(encoder)
    }

    /// Re-create libvpx when the frame size changed.
    fn ensure_size(&mut self, width: u32, height: u32) -> Result<(), VideoError> {
        if (width, height) == (self.width, self.height) {
            return Ok(());
        }
        info!(
            from_w = self.width,
            from_h = self.height,
            to_w = width,
            to_h = height,
            "Frame size changed, re-creating VP8 encoder"
        );
        self.encoder = Self::build(width, height, self.fps, self.bitrate_kbps)?;
        self.width = width;
        self.height = height;
        Ok(())
    }
}

/// Check that a frame is a well-formed I420 image of its stated size.
fn validate_frame(frame: &I420Frame) -> Result<(), VideoError> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w == 0 || h == 0 || !w.is_multiple_of(2) || !h.is_multiple_of(2) {
        return Err(VideoError::EncodeFailed(format!(
            "invalid frame size {w}x{h} (must be even and non-zero)"
        )));
    }
    let uv = (w / 2) * (h / 2);
    if frame.y.len() != w * h || frame.u.len() != uv || frame.v.len() != uv {
        return Err(VideoError::EncodeFailed(format!(
            "frame planes do not match {w}x{h} (y={}, u={}, v={})",
            frame.y.len(),
            frame.u.len(),
            frame.v.len()
        )));
    }
    Ok(())
}

impl VideoEncoder for Vp8Encoder {
    fn encode(&mut self, frame: &I420Frame) -> Result<Vec<EncodedPacket>, VideoError> {
        validate_frame(frame)?;
        self.ensure_size(frame.width, frame.height)?;

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
            .map_err(|e| VideoError::EncodeFailed(format!("VP8 encode: {e}")))?;

        self.frame_count += 1;

        let result: Vec<EncodedPacket> = packets
            .map(|pkt| EncodedPacket {
                data: pkt.data.to_vec(),
                is_keyframe: pkt.key,
                pts: pts_90khz,
            })
            .collect();

        if result.is_empty() && self.frame_count.is_multiple_of(u64::from(self.fps)) {
            warn!("No encoded packets for frame {}", self.frame_count);
        }

        Ok(result)
    }

    fn codec_mime(&self) -> &'static str {
        "video/VP8"
    }

    fn payload_type(&self) -> u8 {
        96 // Matches server sfu.rs VP8 payload type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_frame(width: u32, height: u32) -> I420Frame {
        let uv = ((width / 2) * (height / 2)) as usize;
        I420Frame {
            y: vec![128; (width * height) as usize],
            u: vec![128; uv],
            v: vec![128; uv],
            width,
            height,
        }
    }

    #[test]
    fn encoder_recreated_on_size_change() {
        let params = QualityParams {
            width: 64,
            height: 64,
            fps: 30,
            bitrate_kbps: 200,
        };
        let mut encoder = Vp8Encoder::new(&params).expect("encoder");
        assert_eq!(encoder.dimensions(), (64, 64));

        encoder.encode(&gray_frame(64, 64)).expect("encode 64x64");

        // A smaller frame (window resized) must not panic and must re-create
        // the encoder at the new size, starting with a keyframe.
        let packets = encoder.encode(&gray_frame(32, 48)).expect("encode 32x48");
        assert_eq!(encoder.dimensions(), (32, 48));
        assert!(packets.first().is_some_and(|p| p.is_keyframe));

        // Timestamps keep increasing across the re-creation.
        let later = encoder.encode(&gray_frame(32, 48)).expect("encode again");
        if let (Some(a), Some(b)) = (packets.first(), later.first()) {
            assert!(b.pts > a.pts);
        }

        // A frame whose planes do not match its size is an error, not a panic.
        let mut broken = gray_frame(32, 48);
        broken.y.truncate(10);
        assert!(encoder.encode(&broken).is_err());
    }

    /// Mean absolute difference between a `w`x`h` source plane and a decoded
    /// plane that may carry row padding (`stride >= w`).
    fn plane_mad(src: &[u8], decoded: &[u8], w: usize, h: usize, stride: usize) -> f64 {
        let mut sum = 0u64;
        for row in 0..h {
            for col in 0..w {
                sum += u64::from(src[row * w + col].abs_diff(decoded[row * stride + col]));
            }
        }
        sum as f64 / (w * h) as f64
    }

    /// Encode a known pattern with libvpx (vpx-encode), packetize it like
    /// `video::rtp`, then decode it with kaiku-vp8-decoder and compare.
    ///
    /// Guards the libvpx bindings end to end: with Clang >= 22, bindgen <
    /// 0.72.1 emitted opaque stubs for `vpx_codec_enc_cfg` /
    /// `vpx_codec_dec_cfg`, which the decoder crate compiled with no error.
    #[test]
    fn vp8_encode_decode_round_trip() {
        use kaiku_vp8_decoder::Vp8VideoDecoder;
        use webrtc::rtp::header::Header;
        use webrtc::rtp::packet::Packet;

        const W: u32 = 96;
        const H: u32 = 64;
        let (w, h) = (W as usize, H as usize);
        let (cw, ch) = (w / 2, h / 2);
        // Smooth gradients: easy to code, but every pixel position differs.
        let frame = I420Frame {
            y: (0..w * h)
                .map(|i| (16 + (i % w) * 2 + (i / w)) as u8)
                .collect(),
            u: (0..cw * ch).map(|i| (64 + (i % cw) * 2) as u8).collect(),
            v: (0..cw * ch).map(|i| (192 - (i / cw) * 2) as u8).collect(),
            width: W,
            height: H,
        };

        let mut encoder = Vp8Encoder::new(&QualityParams {
            width: W,
            height: H,
            fps: 30,
            bitrate_kbps: 2000,
        })
        .expect("encoder");
        let mut decoder = Vp8VideoDecoder::new("round-trip".into()).expect("decoder");

        let mut seq = 1u16;
        let mut decoded = Vec::new();
        // A few frames so rate control settles; each must decode.
        for _ in 0..5 {
            for pkt in encoder.encode(&frame).expect("encode") {
                // Small fragments so the depacketizer has to reassemble.
                let chunks: Vec<&[u8]> = pkt.data.chunks(200).collect();
                for (i, chunk) in chunks.iter().enumerate() {
                    let mut payload = vec![if i == 0 { 0x10 } else { 0x00 }];
                    payload.extend_from_slice(chunk);
                    let rtp = Packet {
                        header: Header {
                            version: 2,
                            marker: i == chunks.len() - 1,
                            payload_type: 96,
                            sequence_number: seq,
                            ..Default::default()
                        },
                        payload: payload.into(),
                    };
                    seq = seq.wrapping_add(1);
                    decoded.extend(decoder.process_packet(&rtp).expect("decode"));
                }
            }
        }

        let last = decoded.last().expect("at least one decoded frame");
        assert_eq!((last.width, last.height), (W, H));
        let (ys, uvs) = (last.y_stride as usize, last.uv_stride as usize);
        assert!(ys >= w && uvs >= cw);
        let y_mad = plane_mad(&frame.y, &last.y_plane, w, h, ys);
        let u_mad = plane_mad(&frame.u, &last.u_plane, cw, ch, uvs);
        let v_mad = plane_mad(&frame.v, &last.v_plane, cw, ch, uvs);
        // Lossy codec, but at this bitrate the pattern must survive closely
        // (a broken binding yields garbage, zeros, or a crash).
        assert!(y_mad < 4.0, "Y mean abs diff {y_mad}");
        assert!(u_mad < 4.0, "U mean abs diff {u_mad}");
        assert!(v_mad < 4.0, "V mean abs diff {v_mad}");
    }
}
