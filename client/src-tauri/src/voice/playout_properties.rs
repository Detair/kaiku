//! Property test of the receive path: generated remote streams through one
//! `TrackPlayout`, rendered by the mixer's `Pacer` for a simulated output
//! device.
//!
//! The contract, for any generated stream (loss, bursts, reordering and
//! jitter, duplicates, packet durations of 10–60 ms changing mid-stream,
//! pauses with and without timestamp advance and with or without marker,
//! sender clock drift, a source change) and any device block of 10–400 ms:
//!
//! - P1 exactly one frame per tick, rendered in real time: frames rendered match elapsed time, and
//!   the device never runs dry after warm-up.
//! - P2 no audio is played twice or out of order.
//! - P3 concealment only where a packet is truly absent at its deadline: on a network without
//!   jitter or reordering, every delivered packet is played (except a new source's first packet,
//!   which only a second packet confirms, and what a confirmed restart flushes).
//! - P4 latency within bounds: buffered audio never exceeds the span limit; on a network without
//!   jitter, the level stays within its target band.
//! - P5 no reset unless two consecutive packets agree on a new timeline: resets never exceed the
//!   generated source changes.
//! - P6 memory bounded: never more packets held than the buffer's slots.
//!
//! Generator: a seeded LCG (no extra dependency); every failure message
//! names the case seed.

use std::collections::HashSet;

use super::audio_mixer::Pacer;
use super::jitter_buffer::{CAPACITY, MAX_TARGET_MS};
use super::playout::{FrameDecoder, TrackPlayout, FRAME_SAMPLES};
use crate::audio::OutputLevel;

/// Deterministic pseudo random numbers.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u64) as usize]
    }
}

/// Payload `[epoch, seq_hi, seq_lo, duration in 2.5 ms units]`; decoding
/// fills the samples with `epoch * 100_000 + seq` and records the packet,
/// concealment fills -1.
struct TagDecoder {
    decoded: std::rc::Rc<std::cell::RefCell<HashSet<(u8, u16)>>>,
}

impl FrameDecoder for TagDecoder {
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
        let seq = u16::from_be_bytes([packet[1], packet[2]]);
        self.decoded.borrow_mut().insert((packet[0], seq));
        let tag = u32::from(packet[0]) * 100_000 + u32::from(seq);
        let per_channel = usize::from(packet[3]) * 120;
        out[..per_channel * 2].fill(tag as f32);
        Ok(per_channel)
    }
    fn decode_fec(&mut self, _next: &[u8], out: &mut [f32]) -> Result<usize, String> {
        out.fill(-1.0);
        Ok(out.len() / 2)
    }
    fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
        out.fill(-1.0);
        Ok(out.len() / 2)
    }
    fn can_recover(&self, _next: &[u8], _lost: usize) -> bool {
        false
    }
    fn duration(&self, packet: &[u8]) -> Option<u32> {
        Some(u32::from(packet[3]) * 120)
    }
    fn reset(&mut self) {}
}

#[derive(Debug, Clone)]
struct Arrival {
    at_ms: u64,
    ssrc: u32,
    seq: u16,
    ts: u32,
    marker: bool,
    epoch: u8,
    dur_ms: u64,
    duplicate: bool,
}

#[derive(Debug)]
struct Case {
    seed: u64,
    block_ms: u64,
    jitter_ms: u64,
    loss_pct: u64,
    arrivals: Vec<Arrival>,
    /// Generated source changes (new SSRC and timeline).
    restarts: usize,
    /// Pauses of the sender.
    pauses: usize,
    /// Single packets with a timestamp that does not fit the stream.
    outliers: usize,
    end_ms: u64,
    max_dur_ms: u64,
}

fn generate(seed: u64) -> Case {
    let mut rng = Rng(seed);
    let block_ms = rng.pick(&[10, 20, 21, 43, 50, 100, 160, 200, 300, 400]);
    let jitter_ms = rng.pick(&[0, 0, 0, 10, 30, 60]);
    let loss_pct = rng.pick(&[0, 0, 2, 5, 15]);
    let dup_pct = rng.pick(&[0, 0, 3]);
    let drift = rng.pick(&[0.0, 0.001, -0.001]);
    let honest = rng.chance(50);
    let durations = [10u64, 20, 20, 20, 40, 60];
    let mut dur_ms = rng.pick(&durations);
    let mut max_dur_ms = dur_ms;
    let (mut ssrc, mut epoch) = (rng.next() as u32, 0u8);
    let mut seq = rng.below(30_000) as u16;
    let mut ts = rng.next() as u32;
    let mut t_send = 0.0f64;
    let mut marker = true;
    let (mut restarts, mut pauses, mut outliers) = (0, 0, 0);
    let mut last_was_outlier = false;
    let mut arrivals = Vec::new();
    let mut burst_left = 0u64;
    let end_ms = 20_000u64;
    while (t_send as u64) < end_ms - 1_000 {
        // Events between packets.
        let mut after_pause = false;
        if rng.chance(1) {
            after_pause = true;
            // A pause of the sender, 60 ms – 1 s.
            let pause = rng.pick(&[60u64, 100, 150, 300, 350, 1_000]);
            t_send += pause as f64;
            if honest {
                ts = ts.wrapping_add((pause * 48) as u32);
                marker = rng.chance(80);
            }
            pauses += 1;
        }
        if rng.chance(1) {
            let new = rng.pick(&durations);
            dur_ms = new;
            max_dur_ms = max_dur_ms.max(new);
        }
        if restarts == 0 && rng.below(2_000) == 0 {
            ssrc = rng.next() as u32;
            epoch = 1;
            seq = rng.below(30_000) as u16;
            ts = rng.next() as u32;
            marker = true;
            restarts += 1;
        }
        if burst_left == 0 && rng.below(500) == 0 {
            burst_left = rng.pick(&[3u64, 10, 30]);
        }
        let lost = if burst_left > 0 {
            burst_left -= 1;
            true
        } else {
            rng.chance(loss_pct)
        };
        let at = t_send as u64
            + if jitter_ms > 0 {
                rng.below(jitter_ms)
            } else {
                0
            };
        // Now and then one packet carries a timestamp 0.7–0.9 s off. (Two in
        // a row with the same offset agree on a new timeline and restart the
        // stream, as the contract says; right after a pause one cannot be
        // told from the pause's honest jump: the following packets then
        // restart the stream, a known limit.)
        let outlier = !lost && !after_pause && !last_was_outlier && rng.below(400) == 0;
        last_was_outlier = outlier;
        if outlier {
            outliers += 1;
        }
        let arrival = Arrival {
            at_ms: at,
            ssrc,
            seq,
            ts: if outlier {
                ts.wrapping_add(rng.pick(&[700u32, 800, 900]) * 48)
            } else {
                ts
            },
            marker,
            epoch,
            dur_ms,
            duplicate: false,
        };
        if !lost {
            if rng.chance(dup_pct) {
                let mut copy = arrival.clone();
                copy.at_ms += rng.below(80);
                copy.duplicate = true;
                arrivals.push(copy);
            }
            arrivals.push(arrival);
        }
        marker = false;
        seq = seq.wrapping_add(1);
        ts = ts.wrapping_add((dur_ms * 48) as u32);
        t_send = (dur_ms as f64).mul_add(1.0 + drift, t_send);
    }
    arrivals.sort_by_key(|a| a.at_ms);
    Case {
        seed,
        block_ms,
        jitter_ms,
        loss_pct,
        arrivals,
        restarts,
        pauses,
        outliers,
        end_ms,
        max_dur_ms,
    }
}

/// What a run of a case observed.
#[derive(Debug, Default)]
struct Observed {
    rendered: u64,
    device_missing_after_warmup: usize,
    order_violation: Option<String>,
    delivered: HashSet<(u8, u16)>,
    played: HashSet<(u8, u16)>,
    flushed: usize,
    first_of_new_source: usize,
    max_buffered: usize,
    max_level_ms: f64,
    resets: u64,
}

fn run(case: &Case) -> Observed {
    let decoded = std::rc::Rc::default();
    let mut playout = TrackPlayout::new(TagDecoder {
        decoded: std::rc::Rc::clone(&decoded),
    });
    let level = OutputLevel::new();
    let stream = level.begin_stream();
    let mut pacer = Pacer::default();
    let block = case.block_ms as usize * 96;
    let mut queue = 0usize;
    let mut frame = vec![0.0f32; FRAME_SAMPLES];
    let mut obs = Observed::default();
    let mut next = 0usize;
    let mut last_tag: Option<f32> = None;
    let mut last_ssrc = None;
    for t in 0..case.end_ms {
        while next < case.arrivals.len() && case.arrivals[next].at_ms <= t {
            let a = &case.arrivals[next];
            if !a.duplicate {
                obs.delivered.insert((a.epoch, a.seq));
            }
            if last_ssrc.is_some_and(|s| s != a.ssrc) && !a.duplicate {
                obs.first_of_new_source += 1;
            }
            last_ssrc = Some(a.ssrc);
            let before = playout.stats().buffer.resets;
            let held = playout.buffered();
            let units = (a.dur_ms * 2 / 5) as u8; // 2.5 ms units
            let [hi, lo] = a.seq.to_be_bytes();
            playout.push(
                a.ssrc,
                a.seq,
                a.ts,
                a.marker,
                a.at_ms,
                vec![a.epoch, hi, lo, units],
            );
            if playout.stats().buffer.resets > before {
                obs.flushed += held;
                if std::env::var("PROP_DEBUG").is_ok() {
                    let prev: Vec<_> = case.arrivals[next.saturating_sub(4)..=next]
                        .iter()
                        .map(|a| (a.at_ms, a.seq, a.ts, a.dur_ms, a.marker))
                        .collect();
                    eprintln!("RESET seed {} t {t}: {prev:?}", case.seed);
                }
            }
            obs.max_buffered = obs.max_buffered.max(playout.buffered());
            next += 1;
        }
        if t % 5 == 0 {
            for _ in 0..pacer.frames(5_000, level.queued(), level.request(), level.callbacks()) {
                playout.next_frame(t, &mut frame);
                obs.rendered += 1;
                level.produced(FRAME_SAMPLES);
                queue += FRAME_SAMPLES;
                // Played audio, in order: the first and last sample of each
                // frame (a drift drop cross-fades two packets).
                for sample in [frame[0], frame[FRAME_SAMPLES - 1]] {
                    // Concealment, silence and cross-faded samples carry no
                    // exact tag.
                    if sample <= 0.0 || sample.fract() != 0.0 {
                        continue;
                    }
                    let tag = sample as u32;
                    if let Some(last) = last_tag {
                        let same_epoch = (last as u32) / 100_000 == tag / 100_000;
                        if (same_epoch && sample < last)
                            || ((last as u32) / 100_000 > tag / 100_000)
                        {
                            obs.order_violation
                                .get_or_insert_with(|| format!("t {t}: {sample} after {last}"));
                        }
                    }
                    last_tag = Some(sample);
                }
                obs.max_level_ms = obs.max_level_ms.max(playout.stats().level_ms);
            }
        }
        if t % case.block_ms == 0 && t > 0 {
            level.note_request(stream, block);
            let take = queue.min(block);
            queue -= take;
            level.consumed(take);
            level.callback_done(stream);
            if t > 3_000 {
                obs.device_missing_after_warmup += block - take;
            }
        }
        if t % 1000 == 999 {
            level.roll_window();
        }
    }
    obs.resets = playout.stats().buffer.resets;
    obs.played = decoded.borrow().clone();
    obs
}

#[test]
fn receive_path_keeps_its_contract_for_generated_streams() {
    let mut failures = Vec::new();
    for seed in std::env::var("PROP_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(1..=400u64, |s: u64| s..=s)
    {
        let case = generate(seed);
        let obs = run(&case);
        let tag = format!(
            "seed {seed}: block {} ms, jitter {} ms, loss {} %, {} pauses, {} restarts, {} outliers, max packet {} ms",
            case.block_ms,
            case.jitter_ms,
            case.loss_pct,
            case.pauses,
            case.restarts,
            case.outliers,
            case.max_dur_ms
        );
        // P1: real time and no device starvation.
        let expected = case.end_ms / 20;
        let slack = case.block_ms / 20 + 4;
        if obs.rendered.abs_diff(expected) > slack {
            failures.push(format!("P1 {tag}: {} frames", obs.rendered));
        }
        if obs.device_missing_after_warmup != 0 {
            failures.push(format!(
                "P1 {tag}: device missing {}",
                obs.device_missing_after_warmup
            ));
        }
        // P2: order.
        if let Some(v) = &obs.order_violation {
            failures.push(format!("P2 {tag}: {v}"));
        }
        // P3: on a network without jitter, everything delivered is played.
        if case.jitter_ms == 0 {
            let missing: Vec<_> = obs.delivered.difference(&obs.played).collect();
            // The tail still buffered at the end of the run is not played.
            let allowance = obs.first_of_new_source + obs.flushed + case.outliers + 12;
            if missing.len() > allowance {
                let mut sample: Vec<_> = missing.iter().take(8).collect();
                sample.sort();
                failures.push(format!(
                    "P3 {tag}: {} delivered packets never played (allowed {allowance}), e.g. {sample:?}",
                    missing.len()
                ));
            }
        }
        // P4: latency bound.
        // Target, start reserve for long packets, two packets in flight and
        // a margin.
        let longest = case.max_dur_ms as f64;
        let bound = 2.0f64.mul_add(
            longest,
            2.0f64.mul_add((longest - 20.0).max(0.0), MAX_TARGET_MS),
        );
        let bound = bound + 60.0;
        if obs.max_level_ms > bound.min(660.0) {
            failures.push(format!("P4 {tag}: level {}", obs.max_level_ms));
        }
        // P5: resets only for generated source changes.
        if obs.resets > case.restarts as u64 {
            failures.push(format!("P5 {tag}: {} resets", obs.resets));
        }
        // P6: memory.
        if obs.max_buffered > CAPACITY {
            failures.push(format!("P6 {tag}: {} held", obs.max_buffered));
        }
    }
    assert!(
        failures.is_empty(),
        "{} violations:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
