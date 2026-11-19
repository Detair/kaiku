//! RTP sequence numbers, timestamps and marker bits for the microphone.
//!
//! RFC 3550 §5.1: the timestamp follows the sampling clock, also across
//! frames that were not sent. The voice-activity gate skips frames during
//! silence; the capture side counts them and the timestamp advances over
//! them. The first packet after such a pause carries the marker bit (RFC
//! 3551 §4.1, start of a talk spurt). When frames were not counted (mute,
//! a restarted microphone), the wall clock fills in.

use std::time::Instant;

/// RTP timestamp units per 20 ms frame (48 kHz).
const FRAME_TS: u32 = 960;

/// A wall-clock gap this many frames longer than the counted frames means
/// capture was not running (mute, device switch): the wall clock decides.
const WALL_SLACK_FRAMES: u64 = 5;

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
    last: Option<Instant>,
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

    /// Header fields for the next packet. `skipped_frames`: frames the gate
    /// skipped right before this one; `now`: when it is sent; `backlog`:
    /// packets still queued behind it (then the sender is catching up and
    /// the wall clock says nothing about capture).
    pub fn stamp(&mut self, skipped_frames: u32, now: Instant, backlog: usize) -> RtpStamp {
        let Some(last) = self.last.replace(now) else {
            // First packet of the stream: start of a talk spurt.
            return RtpStamp {
                seq: self.seq,
                ts: self.ts,
                marker: true,
            };
        };
        let mut frames = u64::from(skipped_frames) + 1;
        if backlog == 0 {
            let elapsed_ms = now.saturating_duration_since(last).as_millis() as u64;
            let wall_frames = (elapsed_ms + 10) / 20;
            if wall_frames > frames + WALL_SLACK_FRAMES {
                frames = wall_frames;
            }
        }
        self.seq = self.seq.wrapping_add(1);
        // u32 wrap is part of RTP; the multiplication wraps the same way.
        self.ts = self.ts.wrapping_add((frames as u32).wrapping_mul(FRAME_TS));
        RtpStamp {
            seq: self.seq,
            ts: self.ts,
            marker: frames > 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn continuous_speech_advances_one_frame_per_packet() {
        let mut clock = RtpClock::new(65_534, u32::MAX - 959);
        let t0 = Instant::now();
        let first = clock.stamp(0, t0, 0);
        assert_eq!(
            first,
            RtpStamp {
                seq: 65_534,
                ts: u32::MAX - 959,
                marker: true
            }
        );
        for i in 1..5u32 {
            let s = clock.stamp(0, t0 + Duration::from_millis(u64::from(i) * 20), 0);
            assert_eq!(s.seq, 65_534u16.wrapping_add(i as u16));
            assert_eq!(s.ts, (u32::MAX - 959).wrapping_add(i * FRAME_TS));
            assert!(!s.marker);
        }
    }

    #[test]
    fn gated_pause_advances_the_timestamp_and_marks_the_talk_spurt() {
        let mut clock = RtpClock::new(100, 1_000);
        let t0 = Instant::now();
        clock.stamp(0, t0, 0);
        clock.stamp(0, t0 + Duration::from_millis(20), 0);
        // The gate skipped 7 frames (140 ms) before the next packet.
        let s = clock.stamp(7, t0 + Duration::from_millis(180), 0);
        assert_eq!(s.seq, 102);
        assert_eq!(s.ts, 1_000 + 9 * FRAME_TS);
        assert!(s.marker);
        let next = clock.stamp(0, t0 + Duration::from_millis(200), 0);
        assert_eq!(next.ts, 1_000 + 10 * FRAME_TS);
        assert!(!next.marker);
    }

    #[test]
    fn uncounted_pause_uses_the_wall_clock() {
        // Muted for 2 s: no frames were counted.
        let mut clock = RtpClock::new(0, 0);
        let t0 = Instant::now();
        clock.stamp(0, t0, 0);
        let s = clock.stamp(0, t0 + Duration::from_secs(2), 0);
        assert_eq!(s.ts, 100 * FRAME_TS);
        assert!(s.marker);
    }

    #[test]
    fn a_late_sender_task_does_not_invent_a_pause() {
        // The sender task stalled 200 ms; ten packets wait in its queue.
        let mut clock = RtpClock::new(0, 0);
        let t0 = Instant::now();
        clock.stamp(0, t0, 0);
        let s = clock.stamp(0, t0 + Duration::from_millis(220), 9);
        assert_eq!(s.ts, FRAME_TS);
        assert!(!s.marker);
        // Small scheduling jitter does not either.
        let s = clock.stamp(0, t0 + Duration::from_millis(300), 0);
        assert_eq!(s.ts, 2 * FRAME_TS);
        assert!(!s.marker);
    }
}
