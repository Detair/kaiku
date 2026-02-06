//! VP9 RTP Payloader
//!
//! Packetizes VP9 encoded data into RTP packets per RFC 7741.
//! Sends via `TrackLocalStaticRTP` with 90kHz clock and 1200-byte MTU.

use std::sync::Arc;

use tracing::trace;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;

use super::{EncodedPacket, VideoError};

/// Maximum RTP payload size before fragmentation.
const MAX_PAYLOAD_SIZE: usize = 1200;

/// VP9 RTP payload descriptor (simplified, Profile 0).
///
/// For each packet we send a 1-byte descriptor:
/// - Bit 0 (I): Picture ID present (0 for simplicity)
/// - Bit 1 (P): Inter-picture predicted (0 for keyframes, 1 otherwise)
/// - Bit 2 (L): Layer indices present (0)
/// - Bit 3 (F): Flexible mode (0)
/// - Bit 4 (B): Beginning of frame (1 if first packet)
/// - Bit 5 (E): End of frame (1 if last packet)
/// - Bit 6 (V): Scalability structure present (0)
/// - Bit 7 (Z): Not a reference frame for upper layers (0)
const fn build_vp9_payload_descriptor(is_keyframe: bool, is_first: bool, is_last: bool) -> u8 {
    let mut desc: u8 = 0;

    if !is_keyframe {
        desc |= 0x02; // P bit: inter-picture predicted
    }
    if is_first {
        desc |= 0x10; // B bit: beginning of frame
    }
    if is_last {
        desc |= 0x20; // E bit: end of frame
    }

    desc
}

/// Sends VP9 encoded video as RTP packets to a WebRTC track.
pub struct VideoRtpSender {
    track: Arc<TrackLocalStaticRTP>,
}

impl VideoRtpSender {
    /// Create a new RTP sender for the given video track.
    pub const fn new(track: Arc<TrackLocalStaticRTP>) -> Self {
        Self { track }
    }

    /// Send an encoded packet as one or more RTP packets.
    ///
    /// Large frames are fragmented at `MAX_PAYLOAD_SIZE` boundaries.
    /// `TrackLocalStaticRTP::write()` handles RTP header (SSRC, PT, timestamp) internally.
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

            let descriptor = build_vp9_payload_descriptor(packet.is_keyframe, is_first, is_last);

            // Build payload: 1 byte descriptor + encoded data
            let mut payload = Vec::with_capacity(1 + chunk.len());
            payload.push(descriptor);
            payload.extend_from_slice(chunk);

            self.track
                .write(&payload)
                .await
                .map_err(|e| VideoError::RtpSendFailed(e.to_string()))?;

            trace!(
                ts = timestamp,
                len = payload.len(),
                first = is_first,
                last = is_last,
                keyframe = packet.is_keyframe,
                "Sent RTP packet"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_keyframe_single_packet() {
        // Keyframe, first and last (single packet): B=1, E=1, P=0
        let desc = build_vp9_payload_descriptor(true, true, true);
        assert_eq!(desc & 0x02, 0x00, "P bit should be 0 for keyframe");
        assert_eq!(desc & 0x10, 0x10, "B bit should be set");
        assert_eq!(desc & 0x20, 0x20, "E bit should be set");
    }

    #[test]
    fn descriptor_keyframe_first_last() {
        // Keyframe, first packet only
        let first = build_vp9_payload_descriptor(true, true, false);
        assert_eq!(first & 0x02, 0x00);
        assert_eq!(first & 0x10, 0x10);
        assert_eq!(first & 0x20, 0x00);

        // Keyframe, last packet only
        let last = build_vp9_payload_descriptor(true, false, true);
        assert_eq!(last & 0x10, 0x00);
        assert_eq!(last & 0x20, 0x20);
    }

    #[test]
    fn descriptor_non_keyframe_middle() {
        // Non-keyframe, middle packet: P=1, B=0, E=0
        let desc = build_vp9_payload_descriptor(false, false, false);
        assert_eq!(desc & 0x02, 0x02, "P bit should be set for non-keyframe");
        assert_eq!(desc & 0x10, 0x00, "B bit should be 0 for middle");
        assert_eq!(desc & 0x20, 0x00, "E bit should be 0 for middle");
    }
}
