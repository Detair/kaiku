//! Per-track playout: one jitter buffer and one Opus decoder per remote
//! audio track, delivering exactly one 20 ms stereo frame per tick.
//!
//! The decoder is reached through [`FrameDecoder`] so the concealment logic
//! (FEC vs. PLC, merge, carry-over of long packets) is testable without
//! libopus. [`OpusFrameDecoder`] is the production implementation.

use super::jitter_buffer::{BufferStats, JitterBuffer, Playout, PushOutcome, FRAME_TS};
use crate::audio::{CHANNELS, FRAME_SIZE, SAMPLE_RATE};

/// One 20 ms interleaved stereo frame (960 samples × 2 channels).
pub const FRAME_SAMPLES: usize = FRAME_SIZE * CHANNELS as usize;

/// Longest Opus packet: 120 ms per channel.
const MAX_PACKET_SAMPLES_PER_CHANNEL: usize = 5760;

/// Decoded audio not yet played: at most one long packet plus a frame.
const CARRY_CAPACITY: usize = MAX_PACKET_SAMPLES_PER_CHANNEL * CHANNELS as usize + FRAME_SAMPLES;

/// Pops per tick at most (packets shorter than 20 ms need several).
const MAX_POPS_PER_TICK: usize = 4;

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
    /// Whether `packet` carries in-band FEC for the frame before it.
    fn has_fec(&self, packet: &[u8]) -> bool;
    /// Audio duration of `packet` in samples per channel at 48 kHz.
    fn duration(&self, packet: &[u8]) -> Option<u32>;
    /// Forget all state (new stream).
    fn reset(&mut self);
}

/// libopus decoder, 48 kHz stereo.
pub struct OpusFrameDecoder(opus::Decoder);

impl OpusFrameDecoder {
    pub fn new() -> Result<Self, opus::Error> {
        Ok(Self(opus::Decoder::new(
            SAMPLE_RATE,
            opus::Channels::Stereo,
        )?))
    }
}

impl FrameDecoder for OpusFrameDecoder {
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
        self.0
            .decode_float(packet, out, false)
            .map_err(|e| e.to_string())
    }

    fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String> {
        self.0
            .decode_float(next, out, true)
            .map_err(|e| e.to_string())
    }

    fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
        // An empty packet is libopus' "lost frame": PLC.
        self.0
            .decode_float(&[], out, false)
            .map_err(|e| e.to_string())
    }

    fn has_fec(&self, packet: &[u8]) -> bool {
        opus_packet_has_fec(packet)
    }

    fn duration(&self, packet: &[u8]) -> Option<u32> {
        opus::packet::get_nb_samples(packet, SAMPLE_RATE)
            .ok()
            .map(|n| n as u32)
    }

    fn reset(&mut self) {
        let _ = self.0.reset_state();
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
    let Some(&first_byte) = parsed.frames.first().and_then(|frame| frame.first()) else {
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
    scratch_b: Box<[f32]>,
    concealed_plc: u64,
    recovered_fec: u64,
    decode_errors: u64,
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
            scratch_b: vec![0.0; MAX_PACKET_SAMPLES_PER_CHANNEL * CHANNELS as usize]
                .into_boxed_slice(),
            concealed_plc: 0,
            recovered_fec: 0,
            decode_errors: 0,
        }
    }

    /// Buffer one received RTP packet.
    pub fn push(&mut self, ssrc: u32, seq: u16, ts: u32, arrival_ms: u64, payload: Vec<u8>) {
        if payload.is_empty() {
            return;
        }
        let duration = self
            .decoder
            .duration(&payload)
            .filter(|d| (120..=MAX_PACKET_SAMPLES_PER_CHANNEL as u32).contains(d))
            .unwrap_or(FRAME_TS);
        if self
            .buffer
            .push(ssrc, seq, ts, duration, arrival_ms, payload)
            == PushOutcome::Reset
        {
            self.decoder.reset();
            self.carry_len = 0;
        }
    }

    /// Fill `out` (one interleaved stereo frame) with this tick's audio.
    /// Returns `false` when the track is silent (`out` is zeroed).
    pub fn next_frame(&mut self, now_ms: u64, out: &mut [f32]) -> bool {
        for _ in 0..MAX_POPS_PER_TICK {
            if self.carry_len >= FRAME_SAMPLES {
                break;
            }
            let carry_ts = (self.carry_len / CHANNELS as usize) as u32;
            match self.buffer.pop(now_ms, carry_ts) {
                Playout::Carry | Playout::Silence => break,
                Playout::Decode(packet) => self.decode(&packet),
                Playout::Lost { next } => match next {
                    Some(next) if self.decoder.has_fec(&next) => {
                        self.recovered_fec += 1;
                        let result = self
                            .decoder
                            .decode_fec(&next, &mut self.scratch[..FRAME_SAMPLES]);
                        self.append_decoded(result);
                    }
                    _ => {
                        self.concealed_plc += 1;
                        self.conceal();
                    }
                },
                Playout::Conceal => self.conceal(),
                Playout::Merge(first, second) => self.merge(&first, &second),
            }
        }

        if self.carry_len == 0 {
            out.fill(0.0);
            return false;
        }
        let n = self.carry_len.min(FRAME_SAMPLES).min(out.len());
        out[..n].copy_from_slice(&self.carry[..n]);
        out[n..].fill(0.0);
        self.carry.copy_within(n..self.carry_len, 0);
        self.carry_len -= n;
        true
    }

    pub fn stats(&self) -> PlayoutStats {
        PlayoutStats {
            buffer: self.buffer.stats(),
            concealed_plc: self.concealed_plc,
            recovered_fec: self.recovered_fec,
            decode_errors: self.decode_errors,
            target_delay_ms: self.buffer.target_ms(),
            level_ms: self.buffer.level_ms(),
        }
    }

    fn decode(&mut self, packet: &[u8]) {
        let result = self.decoder.decode(packet, &mut self.scratch);
        if result.is_err() {
            // Keep the decoder moving in step: conceal this frame instead.
            self.decode_errors += 1;
            self.conceal();
            return;
        }
        self.append_decoded(result);
    }

    fn conceal(&mut self) {
        let result = self.decoder.conceal(&mut self.scratch[..FRAME_SAMPLES]);
        self.append_decoded(result);
    }

    /// Decode two consecutive frames and cross-fade them into one.
    fn merge(&mut self, first: &[u8], second: &[u8]) {
        let a = self.decoder.decode(first, &mut self.scratch);
        let b = self.decoder.decode(second, &mut self.scratch_b);
        match (a, b) {
            (Ok(FRAME_SIZE), Ok(FRAME_SIZE)) => {
                let start = self.carry_len;
                let end = (start + FRAME_SAMPLES).min(CARRY_CAPACITY);
                crossfade(
                    &self.scratch[..FRAME_SAMPLES],
                    &self.scratch_b[..FRAME_SAMPLES],
                    &mut self.carry[start..end],
                );
                self.carry_len = end;
            }
            (a, b) => {
                // Unexpected sizes or an error: play both instead.
                self.append_decoded(a);
                if let Ok(n) = b {
                    let samples = (n * CHANNELS as usize).min(self.scratch_b.len());
                    let end = (self.carry_len + samples).min(CARRY_CAPACITY);
                    let take = end - self.carry_len;
                    self.carry[self.carry_len..end].copy_from_slice(&self.scratch_b[..take]);
                    self.carry_len = end;
                } else {
                    self.decode_errors += 1;
                }
            }
        }
    }

    /// Append `per_channel` decoded samples from `scratch` to the carry.
    fn append_decoded(&mut self, result: Result<usize, String>) {
        match result {
            Ok(per_channel) => {
                let samples = (per_channel * CHANNELS as usize).min(self.scratch.len());
                let end = (self.carry_len + samples).min(CARRY_CAPACITY);
                let take = end - self.carry_len;
                self.carry[self.carry_len..end].copy_from_slice(&self.scratch[..take]);
                self.carry_len = end;
            }
            Err(_) => self.decode_errors += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Decode(u8),
        Fec(u8),
        Plc,
        Reset,
    }

    /// Mock packet: `[id, has_fec, duration in 10 ms units]`. Decoding
    /// writes `id / 100` into every sample; FEC writes `-(id / 100)`
    /// (recovered from packet `id`); PLC writes 0.01.
    #[derive(Default)]
    struct MockDecoder {
        calls: Vec<Call>,
    }

    fn mock_packet(id: u8, fec: bool) -> Vec<u8> {
        vec![id, u8::from(fec), 2]
    }

    impl FrameDecoder for MockDecoder {
        fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
            self.calls.push(Call::Decode(packet[0]));
            let per_channel = usize::from(packet[2]) * 480;
            out[..per_channel * 2].fill(f32::from(packet[0]) / 100.0);
            Ok(per_channel)
        }

        fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String> {
            self.calls.push(Call::Fec(next[0]));
            out.fill(-f32::from(next[0]) / 100.0);
            Ok(out.len() / 2)
        }

        fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
            self.calls.push(Call::Plc);
            out.fill(0.01);
            Ok(out.len() / 2)
        }

        fn has_fec(&self, packet: &[u8]) -> bool {
            packet[1] == 1
        }

        fn duration(&self, packet: &[u8]) -> Option<u32> {
            Some(u32::from(packet[2]) * 480)
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
        let at = calls.iter().position(|c| *c == Call::Fec(11)).expect("FEC");
        assert_eq!(calls[at - 1], Call::Decode(9));
        assert_eq!(calls[at + 1], Call::Decode(11));
        assert!(!calls.contains(&Call::Plc));
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
        let at = calls.iter().position(|c| *c == Call::Plc).expect("PLC");
        assert_eq!(calls[at - 1], Call::Decode(9));
        assert_eq!(calls[at + 1], Call::Decode(11));
        assert!(!calls.iter().any(|c| matches!(c, Call::Fec(_))));
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
                playout.push(3, u16::from(id), u32::from(id) * 1920, now, vec![id, 0, 4]);
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
        playout.push(77, 5000, 1, 220, mock_packet(50, false));
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
            let phase =
                2.0 * PI * (150.0 * t - 50.0 / (2.0 * PI * 0.7) * (2.0 * PI * 0.7 * t).cos());
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

    /// Measures what FEC costs in bitrate (printed with `--nocapture`).
    #[test]
    fn encoder_fec_bitrate_overhead() {
        let mut pcm = [0i16; FRAME_SIZE];
        let mut buf = [0u8; 4000];
        let mut kbps = Vec::new();
        for loss in [None, Some(5), Some(10), Some(15)] {
            let mut enc =
                opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip)
                    .unwrap();
            if let Some(loss) = loss {
                enc.set_inband_fec(true).unwrap();
                enc.set_packet_loss_perc(loss).unwrap();
            }
            let mut bytes = 0usize;
            for k in 0..500 {
                speech_frame(k, &mut pcm);
                bytes += enc.encode(&pcm, &mut buf).unwrap();
            }
            // 500 frames = 10 s.
            kbps.push(bytes as f64 * 8.0 / 10.0 / 1000.0);
        }
        eprintln!(
            "Opus VoIP mono payload kbit/s: no FEC {:.1}, FEC@5% {:.1}, FEC@10% {:.1}, FEC@15% {:.1}",
            kbps[0], kbps[1], kbps[2], kbps[3]
        );
        // Total bitrate is set by the encoder; FEC takes a share of it.
        assert!(kbps.iter().all(|k| *k > 8.0 && *k < 80.0), "{kbps:?}");
    }
}
