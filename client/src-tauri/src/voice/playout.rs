//! Per-track playout: one jitter buffer and one Opus decoder per remote
//! audio track, delivering exactly one 20 ms stereo frame per tick.
//!
//! The decoder is reached through [`FrameDecoder`] so the concealment logic
//! (FEC vs. PLC, drift drops on decoded audio, carry-over of long packets)
//! is testable without libopus. [`OpusFrameDecoder`] is the production
//! implementation.
//!
//! Every decoder call adds exactly the audio its timeline step stands for
//! (a packet's duration, a lost packet's duration, or one 20 ms concealment
//! frame); a failed call is replaced by concealment or silence of the same
//! length, so the one-frame-per-tick timeline never shifts.

use super::jitter_buffer::{
    BufferStats, JitterBuffer, Playout, PushOutcome, MAX_PACKET_TS, MIN_PACKET_TS,
};
use crate::audio::{CHANNELS, FRAME_SIZE, SAMPLE_RATE};

/// One 20 ms interleaved stereo frame (960 samples × 2 channels).
pub const FRAME_SAMPLES: usize = FRAME_SIZE * CHANNELS as usize;

/// Longest Opus packet: 120 ms per channel.
const MAX_PACKET_SAMPLES_PER_CHANNEL: usize = 5760;

/// Decoded audio held: below two frames (gathering for a drift drop) plus
/// one longest packet.
const CARRY_CAPACITY: usize =
    MAX_PACKET_SAMPLES_PER_CHANNEL * CHANNELS as usize + 2 * FRAME_SAMPLES;

/// Decoder steps per tick at most: 10 ms packets need two per frame, four
/// when gathering two frames for a drift drop; each step adds at least
/// 10 ms, so this bound is never the limit for valid streams.
const MAX_POPS_PER_TICK: usize = 6;

/// The decoder of one track. All calls happen in play order, once per
/// frame, so the decoder state stays consistent across PLC and FEC.
pub trait FrameDecoder {
    /// Decode `packet` into interleaved stereo `out`. Returns samples per
    /// channel.
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String>;
    /// Rebuild the frame *before* `next` from `next`'s in-band FEC;
    /// `out.len()` sets the lost duration. `next` is decoded normally later.
    fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String>;
    /// One concealment (PLC) frame of `out.len() / 2` samples per channel.
    fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String>;
    /// Whether rebuilding `lost_per_channel` samples before `next` from
    /// `next`'s in-band FEC will really use FEC (otherwise the decoder would
    /// conceal anyway).
    fn can_recover(&self, next: &[u8], lost_per_channel: usize) -> bool;
    /// Audio duration of `packet` in samples per channel at 48 kHz.
    fn duration(&self, packet: &[u8]) -> Option<u32>;
    /// Forget all state (new stream).
    fn reset(&mut self);
}

/// libopus decoder, 48 kHz stereo.
pub struct OpusFrameDecoder {
    decoder: opus::Decoder,
    /// The last decoded packet was CELT-only (libopus then conceals
    /// instead of using FEC).
    last_celt: bool,
}

impl OpusFrameDecoder {
    pub fn new() -> Result<Self, opus::Error> {
        Ok(Self {
            decoder: opus::Decoder::new(SAMPLE_RATE, opus::Channels::Stereo)?,
            last_celt: false,
        })
    }
}

impl FrameDecoder for OpusFrameDecoder {
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
        let decoded = self
            .decoder
            .decode_float(packet, out, false)
            .map_err(|e| e.to_string())?;
        self.last_celt = packet.first().is_some_and(|toc| toc >> 3 >= 16);
        Ok(decoded)
    }

    fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String> {
        self.decoder
            .decode_float(next, out, true)
            .map_err(|e| e.to_string())
    }

    fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
        // An empty packet is libopus' "lost frame": PLC.
        self.decoder
            .decode_float(&[], out, false)
            .map_err(|e| e.to_string())
    }

    fn can_recover(&self, next: &[u8], lost_per_channel: usize) -> bool {
        // libopus falls back to PLC when the previous frame was CELT-only
        // or the gap is shorter than one frame of `next`.
        let frame = opus::packet::get_samples_per_frame(next, SAMPLE_RATE).unwrap_or(usize::MAX);
        !self.last_celt && lost_per_channel >= frame && opus_packet_has_fec(next)
    }

    fn duration(&self, packet: &[u8]) -> Option<u32> {
        opus::packet::get_nb_samples(packet, SAMPLE_RATE)
            .ok()
            .map(|n| n as u32)
    }

    fn reset(&mut self) {
        let _ = self.decoder.reset_state();
        self.last_celt = false;
    }
}

/// Whether an Opus packet carries in-band FEC (LBRR) data.
///
/// Same approach as libwebrtc's `WebRtcOpus_PacketHasFec`: the SILK layer
/// starts with one VAD bit per SILK frame and then the LBRR flag, per
/// channel. They are the first symbols of the range coder, coded with
/// uniform probability, so they are the top bits of the first frame's
/// first byte. CELT-only packets never carry FEC.
pub fn opus_packet_has_fec(packet: &[u8]) -> bool {
    let Some(&toc) = packet.first() else {
        return false;
    };
    let config = toc >> 3;
    let silk_frames: u32 = match config {
        // SILK-only: 10, 20, 40, 60 ms → 1, 1, 2, 3 SILK frames.
        0..=11 => match config & 3 {
            0 | 1 => 1,
            2 => 2,
            _ => 3,
        },
        // Hybrid: 10 or 20 ms → one SILK frame.
        12..=15 => 1,
        // CELT-only.
        _ => return false,
    };
    let channels: u32 = if toc & 0x04 != 0 { 2 } else { 1 };
    let Ok(parsed) = opus::packet::parse(packet) else {
        return false;
    };
    // libopus treats a frame of one byte or less as lost (PLC/DTX).
    let Some(&first_byte) = parsed
        .frames
        .first()
        .filter(|frame| frame.len() > 1)
        .and_then(|frame| frame.first())
    else {
        return false;
    };
    (0..channels).any(|n| first_byte & (0x80 >> ((n + 1) * (silk_frames + 1) - 1)) != 0)
}

/// Cross-fade `a` into `b` over one frame (interleaved stereo): the first
/// sample is `a`, the last is (almost) `b`. Used to drop a frame without a
/// click.
pub fn crossfade(a: &[f32], b: &[f32], out: &mut [f32]) {
    let channels = CHANNELS as usize;
    let frames = (out.len() / channels).max(1);
    for (i, ((o, x), y)) in out.iter_mut().zip(a).zip(b).enumerate() {
        let w = (i / channels) as f32 / frames as f32;
        *o = x.mul_add(1.0 - w, y * w);
    }
}

/// Counters of one track for listening tests.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PlayoutStats {
    pub buffer: BufferStats,
    /// Lost frames concealed with PLC (no FEC available).
    pub concealed_plc: u64,
    /// Lost frames rebuilt from the next packet's in-band FEC.
    pub recovered_fec: u64,
    pub decode_errors: u64,
    /// Packets rejected before buffering: not decodable as Opus, or
    /// shorter than 10 ms (not supported).
    pub rejected: u64,
    /// Packets that waited longer than the receive budget before they
    /// reached the buffer (the mixer had stalled): dropped.
    pub expired: u64,
    /// Packets the reader dropped because the queue to the mixer was full.
    pub queue_full: u64,
    pub target_delay_ms: f64,
    pub level_ms: f64,
}

/// Jitter buffer + decoder of one remote audio track.
pub struct TrackPlayout<D> {
    buffer: JitterBuffer<Vec<u8>>,
    decoder: D,
    carry: Box<[f32]>,
    carry_len: usize,
    scratch: Box<[f32]>,
    concealed_plc: u64,
    recovered_fec: u64,
    decode_errors: u64,
    rejected: u64,
    expired: u64,
}

impl<D: FrameDecoder> TrackPlayout<D> {
    pub fn new(decoder: D) -> Self {
        Self {
            buffer: JitterBuffer::new(),
            decoder,
            carry: vec![0.0; CARRY_CAPACITY].into_boxed_slice(),
            carry_len: 0,
            scratch: vec![0.0; MAX_PACKET_SAMPLES_PER_CHANNEL * CHANNELS as usize]
                .into_boxed_slice(),
            concealed_plc: 0,
            recovered_fec: 0,
            decode_errors: 0,
            rejected: 0,
            expired: 0,
        }
    }

    /// Buffer one received RTP packet (`marker`: RTP marker bit).
    pub fn push(
        &mut self,
        ssrc: u32,
        seq: u16,
        ts: u32,
        marker: bool,
        arrival_ms: u64,
        payload: Vec<u8>,
    ) {
        if payload.is_empty() {
            return;
        }
        // Remote input: only durations the timeline handles are buffered.
        let duration = self
            .decoder
            .duration(&payload)
            .filter(|d| (MIN_PACKET_TS..=MAX_PACKET_TS).contains(d));
        let Some(duration) = duration else {
            self.rejected += 1;
            return;
        };
        let outcome = self
            .buffer
            .push(ssrc, seq, ts, duration, marker, arrival_ms, payload);
        if outcome == PushOutcome::Reset {
            self.decoder.reset();
            self.carry_len = 0;
        }
    }

    /// The output was absent: drop held packets, decoded audio and decoder
    /// state; playback starts afresh with the next packet.
    pub fn flush(&mut self) {
        self.buffer.flush();
        self.carry_len = 0;
        self.decoder.reset();
    }

    /// A packet that waited too long to be worth buffering: count it.
    pub const fn expire(&mut self) {
        self.expired += 1;
    }

    /// Fill `out` (one interleaved stereo frame) with this tick's audio.
    /// Returns `false` when the track is silent (`out` is zeroed).
    pub fn next_frame(&mut self, now_ms: u64, out: &mut [f32]) -> bool {
        let drop = self.buffer.tick(self.carry_ts());
        let want = if drop {
            2 * FRAME_SAMPLES
        } else {
            FRAME_SAMPLES
        };
        for _ in 0..MAX_POPS_PER_TICK {
            if self.carry_len >= want {
                break;
            }
            let want_ts = (want / CHANNELS as usize) as u32;
            match self.buffer.pop(now_ms, self.carry_ts(), want_ts) {
                Playout::Carry | Playout::Silence => break,
                Playout::Decode(packet) => self.decode(&packet),
                Playout::Lost { next, dur_ts } => self.recover(next.as_deref(), dur_ts as usize),
                Playout::Conceal { dur_ts } => self.conceal(dur_ts as usize),
                Playout::Quiet { dur_ts } => self.silence(dur_ts as usize),
            }
        }

        if self.buffer.drop_pending() && self.carry_len >= 2 * FRAME_SAMPLES {
            // Drift: two frames of decoded audio become one, cross-faded.
            let n = FRAME_SAMPLES.min(out.len());
            crossfade(
                &self.carry[..n],
                &self.carry[FRAME_SAMPLES..FRAME_SAMPLES + n],
                &mut out[..n],
            );
            out[n..].fill(0.0);
            self.consume(2 * FRAME_SAMPLES);
            self.buffer.dropped();
            return true;
        }

        if self.carry_len == 0 {
            out.fill(0.0);
            return false;
        }
        let n = self.carry_len.min(FRAME_SAMPLES).min(out.len());
        out[..n].copy_from_slice(&self.carry[..n]);
        out[n..].fill(0.0);
        self.consume(n);
        true
    }

    /// Packets held in the jitter buffer.
    #[cfg(test)]
    pub fn buffered(&self) -> usize {
        self.buffer.buffered()
    }

    pub fn stats(&self) -> PlayoutStats {
        PlayoutStats {
            buffer: self.buffer.stats(),
            concealed_plc: self.concealed_plc,
            recovered_fec: self.recovered_fec,
            decode_errors: self.decode_errors,
            rejected: self.rejected,
            expired: self.expired,
            queue_full: 0,
            target_delay_ms: self.buffer.playout_target_ms(),
            level_ms: self.buffer.level_ms() + f64::from(self.carry_ts()) / 48.0,
        }
    }

    const fn carry_ts(&self) -> u32 {
        (self.carry_len / CHANNELS as usize) as u32
    }

    fn consume(&mut self, samples: usize) {
        let samples = samples.min(self.carry_len);
        self.carry.copy_within(samples..self.carry_len, 0);
        self.carry_len -= samples;
    }

    fn decode(&mut self, packet: &[u8]) {
        let expected = self
            .decoder
            .duration(packet)
            .map_or(FRAME_SIZE, |d| d as usize);
        match self.decoder.decode(packet, &mut self.scratch) {
            Ok(per_channel) if per_channel == expected => self.append(per_channel),
            Ok(_) | Err(_) => {
                // Keep the timeline: conceal what the packet stood for.
                self.decode_errors += 1;
                self.conceal(expected);
            }
        }
    }

    /// A lost packet of `per_channel` samples: FEC from `next` if it has
    /// it, otherwise PLC.
    fn recover(&mut self, next: Option<&[u8]>, per_channel: usize) {
        let per_channel = per_channel.min(MAX_PACKET_SAMPLES_PER_CHANNEL);
        if let Some(next) = next.filter(|n| self.decoder.can_recover(n, per_channel)) {
            let samples = per_channel * CHANNELS as usize;
            match self.decoder.decode_fec(next, &mut self.scratch[..samples]) {
                Ok(n) if n == per_channel => {
                    self.recovered_fec += 1;
                    self.append(per_channel);
                    return;
                }
                Ok(_) | Err(_) => self.decode_errors += 1,
            }
        }
        self.concealed_plc += 1;
        self.conceal(per_channel);
    }

    /// `per_channel` samples of PLC; silence if the decoder fails.
    fn conceal(&mut self, per_channel: usize) {
        let samples = per_channel.min(MAX_PACKET_SAMPLES_PER_CHANNEL) * CHANNELS as usize;
        match self.decoder.conceal(&mut self.scratch[..samples]) {
            Ok(n) if n * CHANNELS as usize == samples => {}
            Ok(_) | Err(_) => {
                self.decode_errors += 1;
                self.scratch[..samples].fill(0.0);
            }
        }
        self.append(samples / CHANNELS as usize);
    }

    /// `per_channel` samples of silence (decoder untouched).
    fn silence(&mut self, per_channel: usize) {
        let samples = per_channel.min(MAX_PACKET_SAMPLES_PER_CHANNEL) * CHANNELS as usize;
        self.scratch[..samples].fill(0.0);
        self.append(samples / CHANNELS as usize);
    }

    /// Append `per_channel` samples from `scratch` to the carry.
    fn append(&mut self, per_channel: usize) {
        let samples = (per_channel * CHANNELS as usize).min(self.scratch.len());
        let end = (self.carry_len + samples).min(CARRY_CAPACITY);
        let take = end - self.carry_len;
        self.carry[self.carry_len..end].copy_from_slice(&self.scratch[..take]);
        self.carry_len = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::jitter_buffer::FRAME_TS;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Decode(u8),
        /// FEC from packet `id`, samples per channel requested.
        Fec(u8, usize),
        /// PLC, samples per channel requested.
        Plc(usize),
        Reset,
    }

    /// Mock packet: `[id, flags, duration in 2.5 ms units]`; flag 1 = has
    /// FEC, flag 2 = FEC data is corrupt. Decoding writes `id / 100` into
    /// every sample; FEC writes `-(id / 100)` (recovered from packet `id`);
    /// PLC writes 0.01.
    #[derive(Default)]
    struct MockDecoder {
        calls: Vec<Call>,
    }

    fn mock_packet(id: u8, fec: bool) -> Vec<u8> {
        vec![id, u8::from(fec), 8]
    }

    impl FrameDecoder for MockDecoder {
        fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
            self.calls.push(Call::Decode(packet[0]));
            let per_channel = usize::from(packet[2]) * 120;
            out[..per_channel * 2].fill(f32::from(packet[0]) / 100.0);
            Ok(per_channel)
        }

        fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String> {
            self.calls.push(Call::Fec(next[0], out.len() / 2));
            if next[1] & 2 != 0 {
                return Err("corrupt".into());
            }
            out.fill(-f32::from(next[0]) / 100.0);
            Ok(out.len() / 2)
        }

        fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
            self.calls.push(Call::Plc(out.len() / 2));
            out.fill(0.01);
            Ok(out.len() / 2)
        }

        fn can_recover(&self, next: &[u8], _lost: usize) -> bool {
            next[1] & 1 == 1
        }

        fn duration(&self, packet: &[u8]) -> Option<u32> {
            Some(u32::from(packet[2]) * 120)
        }

        fn reset(&mut self) {
            self.calls.push(Call::Reset);
        }
    }

    /// Feed packets `0..n` (minus `drop`) at 20 ms, one tick per 20 ms.
    fn run(
        playout: &mut TrackPlayout<MockDecoder>,
        n: u8,
        drop: &[u8],
        fec: bool,
    ) -> Vec<Vec<f32>> {
        let mut frames = Vec::new();
        for i in 0..n {
            let now = u64::from(i) * 20;
            if !drop.contains(&i) {
                playout.push(
                    9,
                    u16::from(i),
                    u32::from(i) * FRAME_TS,
                    false,
                    now,
                    mock_packet(i, fec),
                );
            }
            let mut out = vec![1.0; FRAME_SAMPLES];
            playout.next_frame(now, &mut out);
            frames.push(out);
        }
        frames
    }

    #[test]
    fn fec_path_taken_when_next_packet_has_fec() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        let frames = run(&mut playout, 30, &[10], true);
        let calls = &playout.decoder.calls;
        let at = calls
            .iter()
            .position(|c| *c == Call::Fec(11, FRAME_SIZE))
            .expect("FEC");
        assert_eq!(calls[at - 1], Call::Decode(9));
        assert_eq!(calls[at + 1], Call::Decode(11));
        assert!(!calls.iter().any(|c| matches!(c, Call::Plc(_))));
        let stats = playout.stats();
        assert_eq!(stats.recovered_fec, 1);
        assert_eq!(stats.concealed_plc, 0);
        assert_eq!(stats.buffer.lost, 1);
        // The recovered frame is played in packet 10's place.
        assert!(frames
            .iter()
            .any(|f| f.iter().all(|s| (*s + 0.11).abs() < 1e-6)));
    }

    #[test]
    fn plc_used_for_single_loss_without_fec() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run(&mut playout, 30, &[10], false);
        let calls = &playout.decoder.calls;
        let at = calls
            .iter()
            .position(|c| matches!(c, Call::Plc(_)))
            .expect("PLC");
        assert_eq!(calls[at - 1], Call::Decode(9));
        assert_eq!(calls[at + 1], Call::Decode(11));
        assert!(!calls.iter().any(|c| matches!(c, Call::Fec(..))));
        let stats = playout.stats();
        assert_eq!(stats.concealed_plc, 1);
        assert_eq!(stats.recovered_fec, 0);
    }

    #[test]
    fn decoder_is_called_once_per_frame_in_order() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run(&mut playout, 60, &[20, 30, 31], true);
        let decoded: Vec<u8> = playout
            .decoder
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::Decode(i) => Some(*i),
                _ => None,
            })
            .collect();
        assert!(decoded.windows(2).all(|w| w[1] > w[0]), "{decoded:?}");
        // One call per tick once playing (decode, FEC or PLC).
        let total = playout.decoder.calls.len();
        assert!((57..=60).contains(&total), "{total}");
    }

    #[test]
    fn long_packets_still_give_one_frame_per_tick() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        let mut audible = 0;
        // 40 ms packets every 40 ms, ticks every 20 ms.
        for tick in 0..100u32 {
            let now = u64::from(tick) * 20;
            if tick % 2 == 0 {
                let id = (tick / 2) as u8;
                playout.push(
                    3,
                    u16::from(id),
                    u32::from(id) * 1920,
                    false,
                    now,
                    vec![id, 0, 16],
                );
            }
            let mut out = vec![0.0; FRAME_SAMPLES];
            if playout.next_frame(now, &mut out) {
                audible += 1;
                assert!(out.iter().all(|s| *s > 0.0 || tick < 6), "tick {tick}");
            }
        }
        let decodes = playout
            .decoder
            .calls
            .iter()
            .filter(|c| matches!(c, Call::Decode(_)))
            .count();
        assert!(audible >= 95, "{audible}");
        assert!((47..=50).contains(&decodes), "{decodes}");
        assert_eq!(playout.stats().buffer.lost, 0);
    }

    #[test]
    fn new_ssrc_resets_the_decoder() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run(&mut playout, 10, &[], false);
        // A new source is trusted once a second packet confirms it.
        playout.push(77, 5000, 1, false, 220, mock_packet(50, false));
        assert!(!playout.decoder.calls.contains(&Call::Reset));
        playout.push(77, 5001, 961, false, 240, mock_packet(51, false));
        assert!(playout.decoder.calls.contains(&Call::Reset));
    }

    #[test]
    fn silent_track_reports_silence() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        let mut out = vec![1.0; FRAME_SAMPLES];
        assert!(!playout.next_frame(0, &mut out));
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn crossfade_moves_from_first_to_second_frame() {
        let a = vec![1.0; FRAME_SAMPLES];
        let b = vec![0.0; FRAME_SAMPLES];
        let mut out = vec![9.0; FRAME_SAMPLES];
        crossfade(&a, &b, &mut out);
        assert!((out[0] - 1.0).abs() < 1e-6 && (out[1] - 1.0).abs() < 1e-6);
        assert!(out[FRAME_SAMPLES - 1] < 0.01);
        // Monotonic, and both channels of a sample pair move together.
        assert!(out.chunks(2).all(|p| (p[0] - p[1]).abs() < 1e-6));
        assert!(out.windows(2).all(|w| w[1] <= w[0] + 1e-6));
    }

    /// Feed `n` packets of `units` × 2.5 ms each at real time, ticking every
    /// 20 ms, skipping `drop`. `rate` scales the sender's clock (1.0 =
    /// exact). Returns the output frames.
    fn run_stream(
        playout: &mut TrackPlayout<MockDecoder>,
        units: u8,
        ticks: u64,
        drop: &[u32],
        rate: f64,
    ) -> Vec<Vec<f32>> {
        let dur_us = f64::from(units) * 2_500.0 / rate;
        let mut frames = Vec::new();
        let mut i = 0u32;
        for tick in 0..ticks {
            let now_us = tick * 20_000;
            while (f64::from(i) * dur_us) as u64 <= now_us {
                if !drop.contains(&i) {
                    playout.push(
                        5,
                        i as u16,
                        i.wrapping_mul(u32::from(units) * 120),
                        false,
                        (f64::from(i) * dur_us / 1000.0) as u64,
                        vec![(i % 200) as u8 + 1, 0, units],
                    );
                }
                i += 1;
            }
            let mut out = vec![0.0; FRAME_SAMPLES];
            playout.next_frame(now_us / 1000, &mut out);
            frames.push(out);
        }
        frames
    }

    #[test]
    fn lost_frame_is_concealed_for_the_packet_duration() {
        for (units, per_channel) in [(4u8, 480usize), (8, 960), (16, 1920), (24, 2880)] {
            let mut playout = TrackPlayout::new(MockDecoder::default());
            run_stream(&mut playout, units, 100, &[30], 1.0);
            let plc: Vec<_> = playout
                .decoder
                .calls
                .iter()
                .filter_map(|c| match c {
                    Call::Plc(n) => Some(*n),
                    _ => None,
                })
                .collect();
            assert_eq!(plc, vec![per_channel], "{units} units");
            let stats = playout.stats();
            assert_eq!(stats.concealed_plc, 1, "{units} units");
            assert_eq!(stats.buffer.resets, 0, "{units} units");
        }
    }

    #[test]
    fn every_supported_duration_plays_one_full_frame_per_tick() {
        for units in [4u8, 8, 16, 24, 48] {
            let mut playout = TrackPlayout::new(MockDecoder::default());
            let frames = run_stream(&mut playout, units, 500, &[], 1.0);
            let start = frames.iter().position(|f| f[0] != 0.0).expect("started");
            assert!(start <= 8, "{units} units started at {start}");
            for (tick, f) in frames.iter().enumerate().skip(start) {
                assert!(
                    f.iter().all(|s| *s > 0.0),
                    "{units} units: gap at tick {tick}"
                );
            }
            let st = playout.stats().buffer;
            assert_eq!(
                st.lost + st.resets + st.underruns,
                0,
                "{units} units: {st:?}"
            );
        }
    }

    #[test]
    fn packets_shorter_than_10_ms_are_dropped_and_counted() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run_stream(&mut playout, 1, 50, &[], 1.0);
        assert!(
            playout.decoder.calls.is_empty(),
            "2.5 ms packets were decoded"
        );
    }

    #[test]
    fn long_packet_streams_shrink_for_a_fast_sender() {
        // 60 ms packets, sender clock 0.5 % fast, five minutes.
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run_stream(&mut playout, 24, 15_000, &[], 1.005);
        let st = playout.stats();
        assert_eq!(st.buffer.resets, 0, "{st:?}");
        assert!(st.buffer.drift_drops >= 60, "{st:?}");
        assert!(st.level_ms <= 160.0, "{st:?}");
    }

    #[test]
    fn target_shrinks_at_the_same_pace_for_long_packets() {
        // A stall raises the target; afterwards it must decay by 2 ms per
        // second of playout whatever the packet duration.
        let mut decay = Vec::new();
        for units in [8u8, 24] {
            let mut playout = TrackPlayout::new(MockDecoder::default());
            let per_tick = 8.0 / f64::from(units);
            let mut i = 0u32;
            let mut at_200 = 0.0;
            for tick in 0..1500u64 {
                let now = tick * 20;
                loop {
                    let due = (f64::from(i) / per_tick * 20.0) as u64;
                    // A Wi-Fi stall: everything due from 1.0 to 1.1 s
                    // arrives in one burst at 1.1 s.
                    let arrival = if (1000..1100).contains(&due) {
                        1100
                    } else {
                        due
                    };
                    if arrival > now {
                        break;
                    }
                    playout.push(
                        5,
                        i as u16,
                        i * u32::from(units) * 120,
                        false,
                        arrival,
                        vec![1, 0, units],
                    );
                    i += 1;
                }
                let mut out = vec![0.0; FRAME_SAMPLES];
                playout.next_frame(now, &mut out);
                if tick == 200 {
                    at_200 = playout.stats().target_delay_ms;
                }
            }
            let end = playout.stats().target_delay_ms;
            assert!(
                at_200 > end,
                "{units} units: no stall effect ({at_200} → {end})"
            );
            decay.push(at_200 - end);
        }
        // 1300 ticks at 0.04 ms each (unless the floor was reached).
        assert!(
            (decay[0] - decay[1]).abs() <= 3.0,
            "20 ms stream decayed {} ms, 60 ms stream {} ms",
            decay[0],
            decay[1]
        );
    }

    #[test]
    fn failed_fec_decode_is_concealed_without_skipping_ahead() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        let mut frames = Vec::new();
        for i in 0..30u8 {
            let now = u64::from(i) * 20;
            if i != 10 {
                let flags = if i == 11 { 3 } else { 1 }; // 11: FEC flagged but corrupt
                playout.push(
                    9,
                    u16::from(i),
                    u32::from(i) * FRAME_TS,
                    false,
                    now,
                    vec![i, flags, 8],
                );
            }
            let mut out = vec![0.0; FRAME_SAMPLES];
            playout.next_frame(now, &mut out);
            frames.push(out[0]);
        }
        let stats = playout.stats();
        assert_eq!(stats.recovered_fec, 0, "{stats:?}");
        assert_eq!(stats.concealed_plc, 1, "{stats:?}");
        assert_eq!(stats.decode_errors, 1, "{stats:?}");
        // The lost slot plays concealment, packet 11 the tick after.
        let at = frames
            .iter()
            .position(|v| (*v - 0.09).abs() < 1e-6)
            .unwrap()
            + 1;
        assert!((frames[at] - 0.01).abs() < 1e-6, "{frames:?}");
        assert!((frames[at + 1] - 0.11).abs() < 1e-6, "{frames:?}");
    }

    #[test]
    fn resynchronisation_resets_decoder_and_carry() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run_stream(&mut playout, 24, 20, &[], 1.0);
        // The output stalls: 2 s of packets arrive without playout.
        for i in 20u32..120 {
            playout.push(
                5,
                i as u16,
                i * 2880,
                false,
                u64::from(i) * 60,
                vec![1, 0, 24],
            );
        }
        assert!(playout.decoder.calls.contains(&Call::Reset));
    }

    /// Push `(seq, ts in 2.5 ms units, duration units, arrival ms)` packets
    /// at their arrival and tick every 20 ms until `end_ms`.
    fn run_packets(
        playout: &mut TrackPlayout<MockDecoder>,
        packets: &[(u16, u32, u8, u64)],
        end_ms: u64,
    ) {
        let mut next = 0;
        let mut out = vec![0.0; FRAME_SAMPLES];
        for tick in 0..end_ms / 20 {
            let now = tick * 20;
            while next < packets.len() && packets[next].3 <= now {
                let (seq, ts_units, units, at) = packets[next];
                playout.push(
                    7,
                    seq,
                    ts_units * 120,
                    false,
                    at,
                    vec![(seq % 200) as u8 + 1, 0, units],
                );
                next += 1;
            }
            playout.next_frame(now, &mut out);
        }
    }

    /// A 60 ms packet, then two lost 10 ms packets, then 10 ms packets: the
    /// gap is concealed for exactly its 20 ms and nothing after it is lost.
    #[test]
    fn burst_loss_after_a_duration_change_conceals_only_the_gap() {
        let mut packets = Vec::new();
        let mut ts = 0u32; // 2.5 ms units
        let mut at = 0u64;
        for seq in 0..20u16 {
            packets.push((seq, ts, 24, at));
            ts += 24;
            at += 60;
        }
        // 20 and 21 (10 ms each) are lost.
        ts += 8;
        at += 20;
        for seq in 22..120u16 {
            packets.push((seq, ts, 4, at));
            ts += 4;
            at += 10;
        }
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run_packets(&mut playout, &packets, at + 400);
        let st = playout.stats();
        assert_eq!(st.buffer.late_dropped, 0, "{st:?}");
        // Concealment while the stream runs (not after its end).
        let calls = &playout.decoder.calls;
        let last = calls
            .iter()
            .rposition(|c| matches!(c, Call::Decode(_)))
            .unwrap();
        let plc: usize = calls[..last]
            .iter()
            .map(|c| match c {
                Call::Plc(n) => *n,
                _ => 0,
            })
            .sum();
        assert_eq!(plc, 960, "concealed {plc} samples per channel: {st:?}");
        let decoded = playout
            .decoder
            .calls
            .iter()
            .filter(|c| matches!(c, Call::Decode(_)))
            .count();
        assert_eq!(decoded, packets.len(), "{st:?}");
    }

    /// 10 ms packets with a 10 ms DTX gap between them, both sides already
    /// buffered: the gap is bridged with 10 ms, not a 20 ms frame.
    #[test]
    fn a_short_dtx_gap_is_bridged_for_its_own_length() {
        let mut packets = Vec::new();
        let (mut ts, mut at) = (0u32, 0u64);
        for seq in 0..200u16 {
            if seq == 100 {
                ts += 4; // 10 ms of silence, timestamp runs on
                at += 10;
            }
            packets.push((seq, ts, 4, at));
            ts += 4;
            at += 10;
        }
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run_packets(&mut playout, &packets, at);
        let plc: Vec<usize> = playout
            .decoder
            .calls
            .iter()
            .filter_map(|c| match c {
                Call::Plc(n) => Some(*n),
                _ => None,
            })
            .collect();
        assert_eq!(plc, vec![480], "{plc:?}");
        let st = playout.stats().buffer;
        assert_eq!(st.drift_drops + st.drift_inserts, 0, "{st:?}");
    }

    /// The reported target and level are what the track really holds back,
    /// including the extra reserve for long packets and decoded audio.
    #[test]
    fn long_packets_report_their_effective_target_and_held_audio() {
        let mut playout = TrackPlayout::new(MockDecoder::default());
        run_stream(&mut playout, 24, 500, &[], 1.0); // 60 ms packets
        let st = playout.stats();
        // 40 ms floor + 1.5 × (60 − 20) ms reserve for long packets.
        assert!((st.target_delay_ms - 100.0).abs() < 1e-9, "{st:?}");
        assert!(st.level_ms >= 40.0, "{st:?}");
    }

    // ---- real libopus ----

    /// Speech-like test signal: a voice whose pitch glides between 100 and
    /// 200 Hz, six harmonics, 3 Hz syllable envelope (10–100 %). A steady
    /// tone would be learnt as background noise by the SILK VAD, which then
    /// stops sending FEC.
    fn speech_frame(k: usize, pcm: &mut [i16; FRAME_SIZE]) {
        use std::f32::consts::PI;
        for (n, s) in pcm.iter_mut().enumerate() {
            let t = (k * FRAME_SIZE + n) as f32 / SAMPLE_RATE as f32;
            let env = 0.45f32.mul_add((2.0 * PI * 3.0 * t).sin(), 0.55);
            // Phase of 150 Hz + 50 Hz · sin(2π · 0.7 Hz · t).
            let phase = 2.0
                * PI
                * (50.0 / (2.0 * PI * 0.7)).mul_add(-(2.0 * PI * 0.7 * t).cos(), 150.0 * t);
            let v: f32 = (1..=6).map(|h| (phase * h as f32).sin() / h as f32).sum();
            *s = (v * env * 0.15 * 32767.0) as i16;
        }
    }

    fn rms(frame: &[f32]) -> f32 {
        (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
    }

    #[test]
    fn has_fec_detects_lbrr_only_when_encoded() {
        let mut pcm = [0i16; FRAME_SIZE];
        let mut buf = [0u8; 4000];
        let mut with_fec = crate::audio::voice_encoder().unwrap();
        let mut without =
            opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let (mut on, mut off) = (0, 0);
        for k in 0..100 {
            speech_frame(k, &mut pcm);
            let n = with_fec.encode(&pcm, &mut buf).unwrap();
            on += usize::from(k >= 5 && opus_packet_has_fec(&buf[..n]));
            let n = without.encode(&pcm, &mut buf).unwrap();
            off += usize::from(opus_packet_has_fec(&buf[..n]));
        }
        // LBRR follows SILK's speech activity: most, not all, packets.
        assert!(on >= 60, "LBRR in {on} of 95 packets");
        assert_eq!(off, 0);
        assert!(!opus_packet_has_fec(&[]));
        // CELT-only TOC (config 16+): never FEC.
        assert!(!opus_packet_has_fec(&[16 << 3, 0xFF, 0xFF]));
    }

    #[test]
    fn real_opus_fec_recovers_every_tenth_packet() {
        let mut encoder = crate::audio::voice_encoder().unwrap();
        let mut playout = TrackPlayout::new(OpusFrameDecoder::new().unwrap());
        let mut pcm = [0i16; FRAME_SIZE];
        let mut buf = [0u8; 4000];
        let mut out = vec![0.0f32; FRAME_SIZE * 2];
        let mut gap_rms = Vec::new();
        let mut source_rms = Vec::new();
        let mut dropped = 0u64;
        for k in 0..250usize {
            speech_frame(k, &mut pcm);
            let n = encoder.encode(&pcm, &mut buf).unwrap();
            let now = k as u64 * 20;
            let source: Vec<f32> = pcm.iter().map(|s| f32::from(*s) / 32768.0).collect();
            source_rms.push(rms(&source));
            if k >= 20 && k % 10 == 9 && k < 240 {
                dropped += 1;
            } else {
                playout.push(
                    0x51,
                    k as u16,
                    (k as u32).wrapping_mul(FRAME_TS),
                    false,
                    now,
                    buf[..n].to_vec(),
                );
            }
            let before = playout.stats();
            playout.next_frame(now, &mut out);
            let after = playout.stats();
            if after.recovered_fec + after.concealed_plc
                > before.recovered_fec + before.concealed_plc
            {
                gap_rms.push(rms(&out));
            }
        }
        let stats = playout.stats();
        assert_eq!(stats.buffer.lost, dropped, "{stats:?}");
        assert_eq!(gap_rms.len() as u64, dropped);
        assert_eq!(stats.recovered_fec + stats.concealed_plc, dropped);
        // FEC is only present while SILK hears speech: most gaps, not all.
        assert!(
            stats.recovered_fec * 2 >= dropped,
            "FEC recovered {} of {dropped}: {stats:?}",
            stats.recovered_fec
        );
        // The gaps are filled with signal, not silence.
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(gap_rms.iter().all(|r| *r > 0.002), "{gap_rms:?}");
        assert!(
            mean(&gap_rms) > 0.3 * mean(&source_rms),
            "gaps {} vs source {}",
            mean(&gap_rms),
            mean(&source_rms)
        );
        assert_eq!(stats.decode_errors, 0);
    }

    #[test]
    fn hostile_payloads_never_claim_fec_or_panic() {
        let silk = 1 << 3; // SILK NB 20 ms, mono, code 0
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![silk],
            vec![silk | 1],                       // code 1 with no frames
            vec![silk | 1, 0xFF],                 // code 1, odd length
            vec![silk | 2, 0xFC],                 // code 2, length runs past the end
            vec![silk | 2, 0xFF, 0xFF, 0xFF],     // code 2, length overflow
            vec![silk | 3],                       // code 3 without count byte
            vec![silk | 3, 0],                    // code 3, zero frames
            vec![silk | 3, 49, 0xFF],             // code 3, 49 frames (over 120 ms)
            vec![silk | 3, 0x40 | 2, 0xFF],       // code 3, padding past the end
            vec![silk | 3, 0x80 | 3, 0xFF, 0xFF], // code 3 VBR, lengths missing
        ];
        for packet in &cases {
            assert!(!opus_packet_has_fec(packet), "{packet:?}");
        }
        // A first SILK frame of one byte is DTX/PLC to libopus, whatever
        // its bits say.
        assert!(!opus_packet_has_fec(&[silk, 0xFF]));
        // Largest legal frame count: 48 CELT frames of 2.5 ms (CBR).
        let mut max = vec![(16 << 3) | 3, 48];
        max.extend(std::iter::repeat_n(0x55, 48));
        assert!(!opus_packet_has_fec(&max));
        // A real CELT-only packet from an encoder with FEC on: never FEC.
        let mut celt = opus::Encoder::new(
            SAMPLE_RATE,
            opus::Channels::Mono,
            opus::Application::LowDelay,
        )
        .unwrap();
        celt.set_inband_fec(true).unwrap();
        celt.set_packet_loss_perc(10).unwrap();
        let mut pcm = [0i16; FRAME_SIZE];
        let mut buf = [0u8; 4000];
        for k in 0..20 {
            speech_frame(k, &mut pcm);
            let n = celt.encode(&pcm, &mut buf).unwrap();
            assert!(buf[0] >> 3 >= 16, "not CELT");
            assert!(!opus_packet_has_fec(&buf[..n]));
        }
    }

    /// libopus only rebuilds from FEC when the previous frame was not CELT
    /// and the gap is at least one frame of the next packet; otherwise it
    /// conceals. That must be counted as concealment, not as FEC.
    #[test]
    fn fec_after_a_celt_frame_counts_as_concealment() {
        let mut celt = opus::Encoder::new(
            SAMPLE_RATE,
            opus::Channels::Mono,
            opus::Application::LowDelay,
        )
        .unwrap();
        let mut silk = crate::audio::voice_encoder().unwrap();
        let mut pcm = [0i16; FRAME_SIZE];
        let mut buf = [0u8; 4000];
        let mut silk_packets = Vec::new();
        for k in 0..40 {
            speech_frame(k, &mut pcm);
            let n = silk.encode(&pcm, &mut buf).unwrap();
            silk_packets.push(buf[..n].to_vec());
        }
        let with_lbrr = silk_packets
            .iter()
            .rposition(|p| opus_packet_has_fec(p))
            .expect("an LBRR packet");
        let mut playout = TrackPlayout::new(OpusFrameDecoder::new().unwrap());
        let mut out = vec![0.0f32; FRAME_SAMPLES];
        for k in 0..30u32 {
            let now = u64::from(k) * 20;
            let packet = match k.cmp(&20) {
                std::cmp::Ordering::Less => {
                    speech_frame(k as usize, &mut pcm);
                    let n = celt.encode(&pcm, &mut buf).unwrap();
                    Some(buf[..n].to_vec())
                }
                // Lost; the next one carries LBRR, the decoder is in CELT.
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(silk_packets[with_lbrr].clone()),
            };
            if let Some(packet) = packet {
                playout.push(3, k as u16, k * FRAME_TS, false, now, packet);
            }
            playout.next_frame(now, &mut out);
        }
        let st = playout.stats();
        assert_eq!(st.buffer.lost, 1, "{st:?}");
        assert_eq!((st.recovered_fec, st.concealed_plc), (0, 1), "{st:?}");
    }

    /// Real Opus packets of every duration a peer may send, through the
    /// real decoder at real time with one loss: one full frame per tick,
    /// nothing rejected, the loss concealed or rebuilt.
    #[test]
    fn real_opus_every_packet_duration_end_to_end() {
        for per_packet in [480usize, 960, 1920, 2880, 5760] {
            let mut encoder = crate::audio::voice_encoder().unwrap();
            let frame_len = per_packet.min(2880);
            if per_packet > frame_len {
                // 120 ms = two 60 ms frames in one packet; both must share
                // one mode for the repacketizer, so fix the bandwidth.
                encoder.set_bandwidth(opus::Bandwidth::Wideband).unwrap();
            }
            let mut repack = opus::Repacketizer::new().unwrap();
            let mut playout = TrackPlayout::new(OpusFrameDecoder::new().unwrap());
            let mut pcm = vec![0i16; frame_len];
            let mut buf = [0u8; 4000];
            let mut packets = Vec::new();
            let mut k = 0usize;
            // 3 s of speech.
            while packets.len() * per_packet < 3 * SAMPLE_RATE as usize {
                let mut parts = Vec::new();
                for _ in 0..per_packet / frame_len {
                    for (n, s) in pcm.iter_mut().enumerate() {
                        let t = (k * frame_len + n) as f32 / SAMPLE_RATE as f32;
                        *s = ((2.0 * std::f32::consts::PI * 180.0 * t).sin() * 8000.0) as i16;
                    }
                    k += 1;
                    let n = encoder.encode(&pcm, &mut buf).unwrap();
                    parts.push(buf[..n].to_vec());
                }
                if parts.len() == 1 {
                    packets.push(parts.remove(0));
                } else {
                    let refs: Vec<&[u8]> = parts.iter().map(Vec::as_slice).collect();
                    let n = repack.combine(&refs, &mut buf).unwrap();
                    packets.push(buf[..n].to_vec());
                }
            }
            let lost = packets.len() / 2;
            let dur_ms = per_packet / 48;
            let mut out = vec![0.0f32; FRAME_SAMPLES];
            let mut started = None;
            let mut next = 0usize;
            let ticks = packets.len() * dur_ms / 20;
            for tick in 0..ticks {
                let now = tick as u64 * 20;
                while next < packets.len() && (next * dur_ms) as u64 <= now {
                    if next != lost {
                        playout.push(
                            0x99,
                            next as u16,
                            (next * per_packet) as u32,
                            false,
                            (next * dur_ms) as u64,
                            packets[next].clone(),
                        );
                    }
                    next += 1;
                }
                let audible = playout.next_frame(now, &mut out);
                if started.is_none() && audible {
                    started = Some(tick);
                }
                if started.is_some() && tick + 8 < ticks {
                    assert!(audible, "{dur_ms} ms: silent tick {tick}");
                    assert!(rms(&out) > 0.01, "{dur_ms} ms: quiet tick {tick}");
                }
            }
            let st = playout.stats();
            assert_eq!(st.rejected, 0, "{dur_ms} ms: {st:?}");
            assert_eq!(st.decode_errors, 0, "{dur_ms} ms: {st:?}");
            assert_eq!(st.buffer.lost, 1, "{dur_ms} ms: {st:?}");
            assert_eq!(
                st.recovered_fec + st.concealed_plc,
                1,
                "{dur_ms} ms: {st:?}"
            );
            assert_eq!(
                st.buffer.resets + st.buffer.underruns,
                0,
                "{dur_ms} ms: {st:?}"
            );
        }
    }
}
