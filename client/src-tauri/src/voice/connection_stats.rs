//! Connection quality statistics for the native voice client.
//!
//! webrtc-rs 0.17 does not expose receive-side jitter or packet loss in its
//! stats API (upstream TODOs in `webrtc::stats::InboundRTPStats`), so both
//! are measured here, in the audio decode loop, where every inbound RTP
//! packet already passes through:
//!
//! - jitter: RFC 3550 §6.4.1 interarrival jitter from RTP timestamps
//! - packet loss: RFC 3550 expected-vs-received from RTP sequence numbers
//!
//! Latency comes from the publisher peer connection's native stats — see
//! `WebRtcClient::publisher_rtt_ms`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Opus RTP clock rate (Hz). All Kaiku audio tracks are 48 kHz.
const RTP_CLOCK_RATE: f64 = 48_000.0;

/// RFC 3550 §6.4.1 interarrival jitter estimator for a single RTP stream.
///
/// Uses the delta form: `D` is computed from consecutive packets via
/// `wrapping_sub` on the u32 RTP timestamp (reinterpreted as `i32`), so
/// timestamp wraparound — which can occur at any point in a call because
/// RTP timestamps start at a random offset — produces a correct small
/// delta instead of a ~2^32 spike.
#[derive(Debug, Default)]
pub struct JitterEstimator {
    /// RTP timestamp and arrival instant of the previous packet.
    last: Option<(u32, Instant)>,
    /// Smoothed jitter estimate (RTP timestamp units).
    jitter_units: f64,
}

impl JitterEstimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one RTP packet: its 48 kHz RTP timestamp and arrival instant.
    pub fn on_packet(&mut self, rtp_timestamp: u32, arrival: Instant) {
        if let Some((last_ts, last_arrival)) = self.last {
            let arrival_delta_units =
                arrival.duration_since(last_arrival).as_secs_f64() * RTP_CLOCK_RATE;
            // Wrap-safe signed delta: reordered packets give a small negative.
            let ts_delta = f64::from(rtp_timestamp.wrapping_sub(last_ts) as i32);
            let d = (arrival_delta_units - ts_delta).abs();
            self.jitter_units += (d - self.jitter_units) / 16.0;
        }
        self.last = Some((rtp_timestamp, arrival));
    }

    /// Current jitter estimate in milliseconds.
    pub fn jitter_ms(&self) -> f64 {
        self.jitter_units / RTP_CLOCK_RATE * 1000.0
    }
}

/// RFC 3550 §A.3-style receive loss tracker for a single RTP stream.
///
/// Counts received packets and the expected count derived from forward
/// jumps in the u16 sequence number (wrap-safe). Reordered or duplicate
/// packets don't advance `expected`, so transient reordering doesn't
/// inflate loss.
#[derive(Debug, Default)]
pub struct LossTracker {
    last_seq: Option<u16>,
    received: u64,
    expected: u64,
}

impl LossTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_packet(&mut self, sequence_number: u16) {
        self.received += 1;
        match self.last_seq {
            None => {
                self.expected = 1;
                self.last_seq = Some(sequence_number);
            }
            Some(last) => {
                let delta = sequence_number.wrapping_sub(last) as i16;
                if delta > 0 {
                    self.expected += u64::from(delta as u16);
                    self.last_seq = Some(sequence_number);
                }
                // delta <= 0: reordered or duplicate packet — already counted
                // as expected by the forward jump that skipped it.
            }
        }
    }

    pub fn packets_received(&self) -> u64 {
        self.received
    }

    pub fn packets_lost(&self) -> u64 {
        self.expected.saturating_sub(self.received)
    }
}

/// Snapshot of one track's receive statistics, published periodically by
/// its decode task.
#[derive(Debug, Clone, Copy, Default)]
pub struct TrackSnapshot {
    pub jitter_ms: f64,
    pub packets_lost: u64,
    pub packets_received: u64,
}

/// Aggregate across all active inbound audio tracks.
#[derive(Debug, Clone, Copy)]
pub struct AggregateStats {
    /// Worst (maximum) jitter across tracks, mirroring the browser adapter
    /// which takes the max over inbound-rtp reports.
    pub max_jitter_ms: f64,
    /// Cumulative totals, summed across tracks. The frontend computes
    /// interval loss from deltas, exactly like the browser adapter does
    /// with WebRTC's cumulative inbound-rtp counters.
    pub packets_lost: u64,
    pub packets_received: u64,
}

/// Shared registry of per-track receive stats, written by audio decode
/// tasks and read by the `voice_connection_stats` Tauri command.
///
/// Uses a sync `Mutex`: decode tasks publish roughly once per second (not
/// per packet) and the reader polls at 3 s intervals, so contention is
/// negligible and no `.await` happens while locked.
#[derive(Clone, Default)]
pub struct ConnectionStatsRegistry {
    tracks: Arc<Mutex<HashMap<String, TrackSnapshot>>>,
}

impl ConnectionStatsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(&self, track_id: &str, snapshot: TrackSnapshot) {
        if let Ok(mut map) = self.tracks.lock() {
            map.insert(track_id.to_string(), snapshot);
        }
    }

    pub fn remove_track(&self, track_id: &str) {
        if let Ok(mut map) = self.tracks.lock() {
            map.remove(track_id);
        }
    }

    /// `None` when no track has published yet.
    pub fn aggregate(&self) -> Option<AggregateStats> {
        let map = self.tracks.lock().ok()?;
        if map.is_empty() {
            return None;
        }
        let mut agg = AggregateStats {
            max_jitter_ms: 0.0,
            packets_lost: 0,
            packets_received: 0,
        };
        for snap in map.values() {
            agg.max_jitter_ms = agg.max_jitter_ms.max(snap.jitter_ms);
            agg.packets_lost += snap.packets_lost;
            agg.packets_received += snap.packets_received;
        }
        Some(agg)
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.tracks.lock() {
            map.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Packets arriving exactly on the 20 ms Opus frame cadence have zero
    /// transit variation → jitter must converge to (stay at) zero.
    #[test]
    fn perfectly_paced_stream_has_zero_jitter() {
        let mut est = JitterEstimator::new();
        let start = Instant::now();
        for i in 0..50u32 {
            let ts = i.wrapping_mul(960); // 20 ms @ 48 kHz
            let arrival = start + Duration::from_millis(u64::from(i) * 20);
            est.on_packet(ts, arrival);
        }
        assert!(est.jitter_ms() < 0.01, "jitter was {}", est.jitter_ms());
    }

    /// Alternating 0/+5 ms arrival error makes every consecutive transit
    /// delta exactly 5 ms; RFC 3550's 1/16 smoothing converges to that.
    #[test]
    fn alternating_delay_converges_to_expected_jitter() {
        let mut est = JitterEstimator::new();
        let start = Instant::now();
        for i in 0..500u32 {
            let ts = i * 960;
            let wobble = if i % 2 == 0 { 0 } else { 5 };
            let arrival = start + Duration::from_millis(u64::from(i) * 20 + wobble);
            est.on_packet(ts, arrival);
        }
        let j = est.jitter_ms();
        assert!((4.5..=5.5).contains(&j), "expected ~5ms, got {j}");
    }

    /// A single late packet bumps jitter, then it decays back toward zero.
    #[test]
    fn jitter_decays_after_transient_spike() {
        let mut est = JitterEstimator::new();
        let start = Instant::now();
        let mut arrival_ms = 0u64;
        for i in 0..10u32 {
            est.on_packet(i * 960, start + Duration::from_millis(arrival_ms));
            arrival_ms += 20;
        }
        // one packet 100 ms late
        est.on_packet(10 * 960, start + Duration::from_millis(arrival_ms + 100));
        let spiked = est.jitter_ms();
        assert!(spiked > 2.0, "spike not registered: {spiked}");
        arrival_ms += 20;
        for i in 11..200u32 {
            est.on_packet(i * 960, start + Duration::from_millis(arrival_ms));
            arrival_ms += 20;
        }
        assert!(
            est.jitter_ms() < spiked / 2.0,
            "jitter did not decay: {} vs spike {spiked}",
            est.jitter_ms()
        );
    }

    /// RTP timestamps start at a random offset and wrap u32 mid-call; a
    /// perfectly paced stream crossing the wrap must NOT spike.
    #[test]
    fn timestamp_wraparound_does_not_spike_jitter() {
        let mut est = JitterEstimator::new();
        let start = Instant::now();
        // Start 5 packets before the wrap point.
        let base = u32::MAX - 5 * 960;
        for i in 0..50u32 {
            let ts = base.wrapping_add(i.wrapping_mul(960));
            let arrival = start + Duration::from_millis(u64::from(i) * 20);
            est.on_packet(ts, arrival);
        }
        assert!(
            est.jitter_ms() < 0.01,
            "wraparound spiked jitter to {}",
            est.jitter_ms()
        );
    }

    #[test]
    fn loss_tracker_counts_gaps_as_lost() {
        let mut loss = LossTracker::new();
        for seq in [0u16, 1, 2, 5, 6] {
            loss.on_packet(seq); // 3 and 4 missing
        }
        assert_eq!(loss.packets_received(), 5);
        assert_eq!(loss.packets_lost(), 2);
    }

    #[test]
    fn loss_tracker_handles_reordering_without_double_count() {
        let mut loss = LossTracker::new();
        for seq in [0u16, 1, 3, 2, 4] {
            loss.on_packet(seq); // 2 arrives late, nothing actually lost
        }
        assert_eq!(loss.packets_received(), 5);
        assert_eq!(loss.packets_lost(), 0);
    }

    #[test]
    fn loss_tracker_survives_sequence_wraparound() {
        let mut loss = LossTracker::new();
        for i in 0..10u16 {
            loss.on_packet((u16::MAX - 4).wrapping_add(i)); // crosses the u16 wrap
        }
        assert_eq!(loss.packets_received(), 10);
        assert_eq!(loss.packets_lost(), 0);
    }

    #[test]
    fn registry_aggregates_max_jitter_and_summed_counters() {
        let reg = ConnectionStatsRegistry::new();
        assert!(reg.aggregate().is_none());
        reg.publish(
            "a:mic",
            TrackSnapshot {
                jitter_ms: 3.5,
                packets_lost: 1,
                packets_received: 100,
            },
        );
        reg.publish(
            "b:mic",
            TrackSnapshot {
                jitter_ms: 7.25,
                packets_lost: 4,
                packets_received: 200,
            },
        );
        let agg = reg.aggregate().unwrap();
        assert_eq!(agg.max_jitter_ms, 7.25);
        assert_eq!(agg.packets_lost, 5);
        assert_eq!(agg.packets_received, 300);
        reg.remove_track("b:mic");
        assert_eq!(reg.aggregate().unwrap().packets_lost, 1);
        reg.clear();
        assert!(reg.aggregate().is_none());
    }
}
