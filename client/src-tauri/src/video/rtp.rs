//! VP8 RTP Payloader
//!
//! Packetizes VP8 encoded data into RTP packets per RFC 7741.
//! Sends via `TrackLocalStaticRTP` with 90kHz clock and 1200-byte MTU.

use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use tracing::trace;
use webrtc::rtp::header::Header;
use webrtc::rtp::packet::Packet as RtpPacket;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use super::{EncodedPacket, VideoError};

/// Maximum RTP payload size before fragmentation.
const MAX_PAYLOAD_SIZE: usize = 1200;

/// VP8 RTP payload descriptor (1-byte, no extensions).
///
/// Layout (MSB-first):
///   X=0 | R=0 | N=0 | S | R=0 | PID=000
///
/// S (bit 4): Start of VP8 partition — 1 for first packet of frame.
/// All other bits are 0 (no extensions, single partition).
const fn build_vp8_payload_descriptor(is_first: bool) -> u8 {
    if is_first {
        0x10
    } else {
        0x00
    }
}

/// Sends VP8 encoded video as RTP packets to a WebRTC track.
pub struct VideoRtpSender {
    track: Arc<TrackLocalStaticRTP>,
    seq: AtomicU16,
}

impl VideoRtpSender {
    /// Create a new RTP sender for the given video track.
    pub fn new(track: Arc<TrackLocalStaticRTP>) -> Self {
        Self {
            track,
            seq: AtomicU16::new(0),
        }
    }

    /// Send an encoded packet as one or more RTP packets.
    ///
    /// Large frames are fragmented at `MAX_PAYLOAD_SIZE` boundaries.
    /// Uses `write_rtp()` to set the RTP marker bit on the last fragment,
    /// which VP8 decoders need for frame boundary detection.
    pub async fn send_packet(&self, packet: &EncodedPacket) -> Result<(), VideoError> {
        let data = &packet.data;
        let timestamp = packet.pts as u32;

        if data.is_empty() {
            return Ok(());
        }

        // Fragment into MTU-sized chunks
        let chunks: Vec<&[u8]> = data.chunks(MAX_PAYLOAD_SIZE).collect();
        let total_chunks = chunks.len();

        for (i, chunk) in chunks.iter().enumerate() {
            let is_first = i == 0;
            let is_last = i == total_chunks - 1;

            let descriptor = build_vp8_payload_descriptor(is_first);

            // Build payload: 1 byte descriptor + encoded data
            let mut payload = Vec::with_capacity(1 + chunk.len());
            payload.push(descriptor);
            payload.extend_from_slice(chunk);

            let seq = self.seq.fetch_add(1, Ordering::Relaxed);

            let rtp_packet = RtpPacket {
                header: Header {
                    version: 2,
                    marker: is_last,
                    sequence_number: seq,
                    timestamp,
                    ..Default::default()
                },
                payload: payload.into(),
            };

            self.track
                .write_rtp(&rtp_packet)
                .await
                .map_err(|e| VideoError::RtpSendFailed(e.to_string()))?;

            trace!(
                ts = timestamp,
                seq = seq,
                len = chunk.len() + 1,
                first = is_first,
                last = is_last,
                keyframe = packet.is_keyframe,
                "Sent VP8 RTP packet"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vp8_descriptor_first_packet() {
        let desc = build_vp8_payload_descriptor(true);
        assert_eq!(desc, 0x10, "S bit should be set for first packet");
    }

    #[test]
    fn vp8_descriptor_continuation_packet() {
        let desc = build_vp8_payload_descriptor(false);
        assert_eq!(desc, 0x00, "No bits set for continuation packet");
    }
}
