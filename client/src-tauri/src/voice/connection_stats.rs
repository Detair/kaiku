//! Connection quality statistics for the native voice client.
//!
//! webrtc-rs 0.17 does not expose receive-side jitter in its stats API
//! (upstream TODO), so we compute RFC 3550 §6.4.1 interarrival jitter
//! ourselves in the audio decode loop, where every inbound RTP packet
//! already passes through. Latency and packet loss come from the publisher
//! peer connection's native stats (`RemoteInboundRTPStats` from the SFU's
//! RTCP receiver reports) — see `WebRtcClient::publisher_stats`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Opus RTP clock rate (Hz). All Kaiku audio tracks are 48 kHz.
const RTP_CLOCK_RATE: f64 = 48_000.0;

/// RFC 3550 §6.4.1 interarrival jitter estimator for a single RTP stream.
///
/// `J(i) = J(i-1) + (|D(i-1,i)| - J(i-1)) / 16` where `D` is the difference
/// in relative transit time between consecutive packets, measured in RTP
/// timestamp units and converted to milliseconds for reporting.
#[derive(Debug)]
pub struct JitterEstimator {
    /// Reference point for arrival clock, fixed at first packet.
    epoch: Option<Instant>,
    /// Relative transit time of the previous packet (RTP timestamp units).
    last_transit: Option<f64>,
    /// Smoothed jitter estimate (RTP timestamp units).
    jitter_units: f64,
}

impl JitterEstimator {
    pub fn new() -> Self {
        Self {
            epoch: None,
            last_transit: None,
            jitter_units: 0.0,
        }
    }

    /// Feed one RTP packet: its 48 kHz RTP timestamp and arrival instant.
    pub fn on_packet(&mut self, rtp_timestamp: u32, arrival: Instant) {
        let epoch = *self.epoch.get_or_insert(arrival);
        let arrival_units = arrival.duration_since(epoch).as_secs_f64() * RTP_CLOCK_RATE;
        // Wrapping-aware: RTP timestamps are u32 and may wrap mid-stream.
        // Relative transit only ever appears as a delta, so absolute offset
        // cancels out; cast to f64 after wrapping-extend from the first seen.
        let transit = arrival_units - f64::from(rtp_timestamp);
        if let Some(last) = self.last_transit {
            let d = (transit - last).abs();
            self.jitter_units += (d - self.jitter_units) / 16.0;
        }
        self.last_transit = Some(transit);
    }

    /// Current jitter estimate in milliseconds.
    pub fn jitter_ms(&self) -> f64 {
        self.jitter_units / RTP_CLOCK_RATE * 1000.0
    }
}

impl Default for JitterEstimator {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared registry of per-track jitter estimates, written by audio decode
/// tasks and read by the `voice_connection_stats` Tauri command.
///
/// Uses a sync `Mutex`: writers hold it for a single `HashMap` insert per
/// RTP packet (50/s per track) and the reader copies out at 3 s intervals,
/// so contention is negligible and no `.await` happens while locked.
#[derive(Clone, Default)]
pub struct ConnectionStatsRegistry {
    jitter_by_track: Arc<Mutex<HashMap<String, f64>>>,
}

impl ConnectionStatsRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_jitter(&self, track_id: &str, jitter_ms: f64) {
        if let Ok(mut map) = self.jitter_by_track.lock() {
            map.insert(track_id.to_string(), jitter_ms);
        }
    }

    pub fn remove_track(&self, track_id: &str) {
        if let Ok(mut map) = self.jitter_by_track.lock() {
            map.remove(track_id);
        }
    }

    /// Worst (maximum) jitter across all active inbound audio tracks, in ms.
    /// Mirrors the browser adapter, which takes the max over inbound-rtp
    /// reports. `None` when no track is reporting yet.
    pub fn max_jitter_ms(&self) -> Option<f64> {
        self.jitter_by_track
            .lock()
            .ok()?
            .values()
            .copied()
            .fold(None, |acc, v| Some(acc.map_or(v, |a: f64| a.max(v))))
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.jitter_by_track.lock() {
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
            let ts = i * 960; // 20 ms @ 48 kHz
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

    #[test]
    fn registry_tracks_max_across_tracks_and_clears() {
        let reg = ConnectionStatsRegistry::new();
        assert_eq!(reg.max_jitter_ms(), None);
        reg.update_jitter("a:mic", 3.5);
        reg.update_jitter("b:mic", 7.25);
        assert_eq!(reg.max_jitter_ms(), Some(7.25));
        reg.remove_track("b:mic");
        assert_eq!(reg.max_jitter_ms(), Some(3.5));
        reg.clear();
        assert_eq!(reg.max_jitter_ms(), None);
    }
}
