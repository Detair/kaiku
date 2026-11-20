//! RTP sequence numbers, timestamps and marker bits for the microphone.
//!
//! RFC 3550 §5.1: the timestamp follows the sampling clock, also across
//! frames that were not sent. The sampling clock is the capture's own frame
//! count: the capture numbers every 20 ms frame it delivers, sent or not
//! (voice-activity gate closed, muted, could not be queued), and the
//! timestamp advances by the difference. Send times play no part, so
//! capture that delivers in bursts or catches up after a hiccup advances
//! exactly by the audio produced. Only when the capture itself was
//! restarted (a new microphone stream, which counts from zero again) does
//! the time between the two capture moments bridge the restart.
//!
//! The marker bit (RFC 3551 §4.1, start of a talk spurt) is set on the
//! first packet of the stream, after a restart, and when the capture says a
//! talk spurt starts (after gated or muted frames); not after a frame that
//! was merely lost on its way to the sender.

use std::time::Instant;

/// RTP timestamp units per 20 ms frame (48 kHz).
const FRAME_TS: u32 = 960;

/// Where an encoded frame sits in the capture's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturePosition {
    /// Identifies one capture stream (a restart gets a new one).
    pub capture: u32,
    /// Index of this 20 ms frame in that capture stream.
    pub frame: u64,
    /// When the capture produced it.
    pub at: Instant,
    /// Gated or muted frames came right before this one.
    pub talkspurt: bool,
}

/// Header fields for one outgoing packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtpStamp {
    pub seq: u16,
    pub ts: u32,
    pub marker: bool,
}

/// Per-stream RTP clock of the microphone sender.
#[derive(Debug)]
pub struct RtpClock {
    seq: u16,
    ts: u32,
    last: Option<CapturePosition>,
}

impl RtpClock {
    /// Start at the given (random, RFC 3550 §5.1) sequence number and
    /// timestamp.
    pub const fn new(seq: u16, ts: u32) -> Self {
        Self {
            seq,
            ts,
            last: None,
        }
    }

    /// Header fields for the packet carrying the frame at `position`.
    pub fn stamp(&mut self, position: CapturePosition) -> RtpStamp {
        let Some(last) = self.last.replace(position) else {
            // First packet of the stream: start of a talk spurt.
            return RtpStamp {
                seq: self.seq,
                ts: self.ts,
                marker: true,
            };
        };
        let (frames, marker) = if position.capture == last.capture {
            // At least one frame on, even if the capture repeated a number.
            (
                position.frame.saturating_sub(last.frame).max(1),
                position.talkspurt,
            )
        } else {
            // The capture restarted: bridge the time between the two
            // capture moments (at least one frame).
            let gap_ms = position.at.saturating_duration_since(last.at).as_millis() as u64;
            (((gap_ms + 10) / 20).max(1), true)
        };
        self.seq = self.seq.wrapping_add(1);
        // u32 wrap is part of RTP; the multiplication wraps the same way.
        self.ts = self.ts.wrapping_add((frames as u32).wrapping_mul(FRAME_TS));
        RtpStamp {
            seq: self.seq,
            ts: self.ts,
            marker,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    fn frame(capture: u32, frame: u64, at: Instant, talkspurt: bool) -> CapturePosition {
        CapturePosition {
            capture,
            frame,
            at,
            talkspurt,
        }
    }

    #[test]
    fn continuous_speech_advances_one_frame_per_packet() {
        let mut clock = RtpClock::new(65_534, u32::MAX - 959);
        let t0 = Instant::now();
        let first = clock.stamp(frame(1, 0, t0, false));
        assert_eq!(
            first,
            RtpStamp {
                seq: 65_534,
                ts: u32::MAX - 959,
                marker: true
            }
        );
        for i in 1..5u32 {
            let s = clock.stamp(frame(1, u64::from(i), at(t0, u64::from(i) * 20), false));
            assert_eq!(s.seq, 65_534u16.wrapping_add(i as u16));
            assert_eq!(s.ts, (u32::MAX - 959).wrapping_add(i * FRAME_TS));
            assert!(!s.marker);
        }
    }

    #[test]
    fn bursty_capture_does_not_invent_pauses() {
        // Capture blocks from 10 to 200 ms: frames arrive in bursts, but
        // the timestamp follows the frames produced, exactly.
        for block_ms in [10u64, 20, 40, 60, 100, 140, 200] {
            let mut clock = RtpClock::new(0, 0);
            let t0 = Instant::now();
            let mut last = clock.stamp(frame(1, 0, t0, false));
            let per_block = (block_ms / 20).max(1);
            let mut n = 1u64;
            for block in 1..30u64 {
                for k in 0..per_block {
                    let s = clock.stamp(frame(1, n, at(t0, block * block_ms + k), false));
                    assert_eq!(s.ts.wrapping_sub(last.ts), FRAME_TS, "{block_ms} ms");
                    assert!(!s.marker, "{block_ms} ms");
                    last = s;
                    n += 1;
                }
            }
        }
    }

    #[test]
    fn a_capture_hiccup_leaves_no_hole() {
        // The capture stalls once for 150 ms and then delivers what it held.
        let mut clock = RtpClock::new(0, 0);
        let t0 = Instant::now();
        clock.stamp(frame(1, 0, t0, false));
        let s = clock.stamp(frame(1, 1, at(t0, 170), false));
        assert_eq!(s.ts, FRAME_TS);
        assert!(!s.marker);
    }

    #[test]
    fn gated_pause_advances_the_timestamp_and_marks_the_talk_spurt() {
        let mut clock = RtpClock::new(100, 1_000);
        let t0 = Instant::now();
        clock.stamp(frame(1, 0, t0, false));
        clock.stamp(frame(1, 1, at(t0, 20), false));
        // The gate skipped 7 frames (140 ms) before the next packet.
        let s = clock.stamp(frame(1, 9, at(t0, 180), true));
        assert_eq!(s.seq, 102);
        assert_eq!(s.ts, 1_000 + 9 * FRAME_TS);
        assert!(s.marker);
        let next = clock.stamp(frame(1, 10, at(t0, 200), false));
        assert_eq!(next.ts, 1_000 + 10 * FRAME_TS);
        assert!(!next.marker);
    }

    #[test]
    fn mute_advances_by_the_frames_captured_meanwhile() {
        // Long and short (60 ms) mutes: the capture counted the muted frames.
        for muted in [100u64, 3] {
            let mut clock = RtpClock::new(0, 0);
            let t0 = Instant::now();
            clock.stamp(frame(1, 0, t0, false));
            // Two frames delivered at once after unmuting.
            let s = clock.stamp(frame(1, 1 + muted, at(t0, 20 * (1 + muted)), true));
            assert_eq!(s.ts, (1 + muted as u32) * FRAME_TS);
            assert!(s.marker);
            let s = clock.stamp(frame(1, 2 + muted, at(t0, 20 * (1 + muted) + 1), false));
            assert_eq!(s.ts, (2 + muted as u32) * FRAME_TS);
            assert!(!s.marker);
        }
    }

    #[test]
    fn a_frame_lost_before_sending_advances_without_a_marker() {
        let mut clock = RtpClock::new(0, 0);
        let t0 = Instant::now();
        clock.stamp(frame(1, 0, t0, false));
        let s = clock.stamp(frame(1, 2, at(t0, 40), false));
        assert_eq!(s.ts, 2 * FRAME_TS);
        assert!(!s.marker);
    }

    #[test]
    fn a_restarted_microphone_bridges_the_time_between_captures() {
        let mut clock = RtpClock::new(0, 0);
        let t0 = Instant::now();
        clock.stamp(frame(1, 50, t0, false));
        // New capture stream 2 s later, counting from zero.
        let s = clock.stamp(frame(2, 0, at(t0, 2_000), false));
        assert_eq!(s.ts, 100 * FRAME_TS);
        assert!(s.marker);
        let s = clock.stamp(frame(2, 1, at(t0, 2_020), false));
        assert_eq!(s.ts, 101 * FRAME_TS);
        assert!(!s.marker);
        // An immediate restart still moves on by at least one frame.
        let s = clock.stamp(frame(3, 0, at(t0, 2_021), false));
        assert_eq!(s.ts, 102 * FRAME_TS);
    }
}
