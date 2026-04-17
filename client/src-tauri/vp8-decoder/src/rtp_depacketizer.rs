//! VP8 RTP depacketizer per RFC 7741.
//!
//! Reassembles VP8 frames from a stream of RTP packets. Each packet carries
//! a VP8 payload descriptor byte (optionally with extensions), followed by a
//! fragment of the encoded VP8 frame. Fragments are concatenated until the
//! RTP marker bit signals end-of-frame.
//!
//! On sequence discontinuity, the in-progress frame is dropped. Recovery
//! relies on the existing interval PLI sender in the voice subscriber; no
//! new PLI request plumbing is added here.

use tracing::debug;
use webrtc::rtp::packet::Packet as RtpPacket;

/// VP8 RTP depacketizer.
///
/// Stateful reassembler: feed one RTP packet at a time via [`Self::depacketize`].
/// Returns `Some(frame)` on the packet that carries the RTP marker bit (end of
/// frame), otherwise `None`.
pub struct Vp8Depacketizer {
    frame_buffer: Vec<u8>,
    last_seq: Option<u16>,
    expecting_keyframe: bool,
}

impl Vp8Depacketizer {
    pub const fn new() -> Self {
        Self {
            frame_buffer: Vec::new(),
            last_seq: None,
            expecting_keyframe: true,
        }
    }

    /// Feed an RTP packet. Returns the complete VP8 frame bytes if one is assembled.
    ///
    /// Resets on sequence gap (frame corrupted). After a gap, non-keyframes are
    /// discarded until the next keyframe arrives — the existing interval PLI
    /// sender in the subscriber drives recovery.
    pub fn depacketize(&mut self, packet: &RtpPacket) -> Option<Vec<u8>> {
        let seq = packet.header.sequence_number;

        // 1. Sequence continuity check.
        match self.last_seq {
            None => {
                // First packet ever seen — accept.
            }
            Some(last) => {
                let expected = last.wrapping_add(1);
                if seq != expected {
                    debug!(
                        last_seq = last,
                        got_seq = seq,
                        "VP8 depacketizer: sequence gap, dropping partial frame"
                    );
                    self.frame_buffer.clear();
                    self.expecting_keyframe = true;
                }
            }
        }
        self.last_seq = Some(seq);

        // 2. Parse the VP8 payload descriptor (RFC 7741 §4.2).
        //
        //  Byte 0 (MSB-first):
        //      bit 7 = X  (extension byte follows)
        //      bit 6 = R  (reserved, must be 0)
        //      bit 5 = N  (non-reference frame)
        //      bit 4 = S  (start of VP8 partition)
        //      bit 3 = R  (reserved, must be 0)
        //      bits 2..0 = PID (partition index, 3 bits)
        //
        //  If X: Byte 1 extension flags (MSB-first):
        //      bit 7 = I  (PictureID present)
        //      bit 6 = L  (TL0PICIDX present)
        //      bit 5 = T  (TID/Y present)
        //      bit 4 = K  (KEYIDX present)
        //      bits 3..0 = RSV (reserved)
        //
        //  If I: 1 byte PictureID; if its bit 7 (M) is set, it's a 15-bit
        //        PictureID occupying 2 bytes total.
        //  If L: 1 byte TL0PICIDX.
        //  If T or K: 1 byte TID|Y|KEYIDX.
        let payload = &packet.payload[..];
        if payload.is_empty() {
            debug!("VP8 depacketizer: empty payload, resyncing");
            self.frame_buffer.clear();
            self.expecting_keyframe = true;
            return None;
        }

        let byte0 = payload[0];
        let x_bit = (byte0 & 0x80) != 0;
        let mut offset = 1usize;

        if x_bit {
            if payload.len() < offset + 1 {
                debug!("VP8 depacketizer: truncated extension byte, resyncing");
                self.frame_buffer.clear();
                self.expecting_keyframe = true;
                return None;
            }
            let ext = payload[offset];
            offset += 1;

            let i_bit = (ext & 0x80) != 0;
            let l_bit = (ext & 0x40) != 0;
            let t_bit = (ext & 0x20) != 0;
            let k_bit = (ext & 0x10) != 0;

            if i_bit {
                if payload.len() < offset + 1 {
                    debug!("VP8 depacketizer: truncated PictureID, resyncing");
                    self.frame_buffer.clear();
                    self.expecting_keyframe = true;
                    return None;
                }
                let m_bit = (payload[offset] & 0x80) != 0;
                offset += 1;
                if m_bit {
                    // 15-bit PictureID: 2 bytes total.
                    if payload.len() < offset + 1 {
                        debug!("VP8 depacketizer: truncated 15-bit PictureID, resyncing");
                        self.frame_buffer.clear();
                        self.expecting_keyframe = true;
                        return None;
                    }
                    offset += 1;
                }
            }

            if l_bit {
                if payload.len() < offset + 1 {
                    debug!("VP8 depacketizer: truncated TL0PICIDX, resyncing");
                    self.frame_buffer.clear();
                    self.expecting_keyframe = true;
                    return None;
                }
                offset += 1;
            }

            if t_bit || k_bit {
                if payload.len() < offset + 1 {
                    debug!("VP8 depacketizer: truncated TID/KEYIDX byte, resyncing");
                    self.frame_buffer.clear();
                    self.expecting_keyframe = true;
                    return None;
                }
                offset += 1;
            }
        }

        // 3. Accumulate the remaining payload bytes.
        if offset > payload.len() {
            // Defensive — already guarded above, but keeps the slice safe.
            debug!("VP8 depacketizer: descriptor overran payload, resyncing");
            self.frame_buffer.clear();
            self.expecting_keyframe = true;
            return None;
        }
        self.frame_buffer.extend_from_slice(&payload[offset..]);

        // 4. On marker bit, the frame is complete.
        if packet.header.marker {
            let frame = std::mem::take(&mut self.frame_buffer);

            if frame.is_empty() {
                // Nothing to deliver — stay armed for the next keyframe.
                self.expecting_keyframe = true;
                return None;
            }

            // VP8 uncompressed frame tag (RFC 6386 §9.1):
            //   byte 0 bit 0 = frame_type (0 = key, 1 = interframe).
            let is_keyframe = (frame[0] & 0x01) == 0;

            if self.expecting_keyframe && !is_keyframe {
                debug!("VP8 depacketizer: waiting for keyframe, discarding assembled interframe");
                // Keep expecting_keyframe = true; drop the frame.
                return None;
            }

            if is_keyframe {
                self.expecting_keyframe = false;
            }

            return Some(frame);
        }

        None
    }
}

impl Default for Vp8Depacketizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use webrtc::rtp::header::Header;
    use webrtc::rtp::packet::Packet;

    use super::*;

    /// Build a minimal RTP packet for tests.
    fn pkt(seq: u16, marker: bool, payload: Vec<u8>) -> Packet {
        Packet {
            header: Header {
                version: 2,
                marker,
                payload_type: 96,
                sequence_number: seq,
                ..Default::default()
            },
            payload: Bytes::from(payload),
        }
    }

    /// VP8 frame starting with a keyframe tag byte (bit 0 = 0).
    fn keyframe_payload_fragment(descriptor: &[u8], vp8_data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(descriptor.len() + vp8_data.len());
        out.extend_from_slice(descriptor);
        out.extend_from_slice(vp8_data);
        out
    }

    #[test]
    fn reassembles_keyframe_across_two_packets() {
        let mut dp = Vp8Depacketizer::new();

        // Fragment 1: S=1, no extensions. Keyframe tag byte = 0x00.
        let p1 = pkt(
            100,
            false,
            keyframe_payload_fragment(&[0x10], &[0x00, 0xAA, 0xBB]),
        );
        // Fragment 2: S=0, no extensions. Marker bit set.
        let p2 = pkt(101, true, keyframe_payload_fragment(&[0x00], &[0xCC, 0xDD]));

        assert!(dp.depacketize(&p1).is_none());
        let frame = dp.depacketize(&p2).expect("frame assembled");
        assert_eq!(frame, vec![0x00, 0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn strips_full_descriptor_with_all_extensions() {
        let mut dp = Vp8Depacketizer::new();

        // Descriptor: X=1, S=1. Extension byte: I=1, L=1, T=1, K=1.
        // PictureID: 15-bit (M=1) → 2 bytes (0x80, 0x00).
        // TL0PICIDX: 1 byte (0x00).
        // TID|Y|KEYIDX: 1 byte (0x00).
        // Total descriptor length: 1 + 1 + 2 + 1 + 1 = 6 bytes.
        let descriptor: &[u8] = &[0x90, 0xF0, 0x80, 0x00, 0x00, 0x00];
        let vp8 = [0x00, 0x11, 0x22]; // keyframe tag
        let p = pkt(42, true, keyframe_payload_fragment(descriptor, &vp8));

        let frame = dp.depacketize(&p).expect("frame assembled");
        assert_eq!(frame, vec![0x00, 0x11, 0x22]);
    }

    #[test]
    fn drops_partial_frame_on_sequence_gap_internal() {
        let mut dp = Vp8Depacketizer::new();

        // Feed seq=100 (keyframe start, no marker).
        let p1 = pkt(
            100,
            false,
            keyframe_payload_fragment(&[0x10], &[0x00, 0xAA]),
        );
        assert!(dp.depacketize(&p1).is_none());

        // Feed seq=101 (continuation, no marker).
        let p2 = pkt(101, false, keyframe_payload_fragment(&[0x00], &[0xBB]));
        assert!(dp.depacketize(&p2).is_none());

        // Gap: skip 102, feed seq=103 with marker. Frame buffer must reset,
        // and since it's not a keyframe start (we're mid-stream) the
        // assembled chunk is still rejected because we're now expecting a
        // keyframe again.
        let p4 = pkt(103, true, keyframe_payload_fragment(&[0x00], &[0x01, 0xCC]));
        let out = dp.depacketize(&p4);
        assert!(
            out.is_none(),
            "frame should not be returned after sequence gap, got {out:?}"
        );
    }

    #[test]
    fn interframe_before_keyframe_is_dropped() {
        let mut dp = Vp8Depacketizer::new();

        // Single packet with marker — but frame tag byte 0 bit 0 = 1 (interframe).
        let p = pkt(1, true, keyframe_payload_fragment(&[0x10], &[0x01, 0xDE]));
        assert!(dp.depacketize(&p).is_none());

        // Follow-up keyframe should be accepted.
        let p2 = pkt(2, true, keyframe_payload_fragment(&[0x10], &[0x00, 0xAD]));
        let frame = dp.depacketize(&p2).expect("keyframe assembled");
        assert_eq!(frame, vec![0x00, 0xAD]);
    }
}
