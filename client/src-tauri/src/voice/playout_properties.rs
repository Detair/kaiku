//! Property test of the receive path: generated remote streams through one
//! `TrackPlayout`, rendered by the mixer's `Pacer` for a simulated output
//! device whose block size may change and which may stall.
//!
//! The contract, for any generated stream (loss of single packets and of
//! runs of 1–6, with and without in-band FEC on the successor; jitter and
//! reordering; duplicates; packet durations of 10–120 ms changing
//! mid-stream; pauses with and without timestamp advance and marker;
//! sender clock drift; a source change with late packets of the old source
//! afterwards; timestamp outliers; inconsistent packets; sequence wrap) and
//! any device (blocks of 10–400 ms, constant, in a repeating pattern or
//! wandering; stalls of seconds):
//!
//! - P1 one frame per tick, rendered in real time; the device never finds the queue short while the
//!   stream is intact (after warm-up, a stall or a block larger than any of the last two seconds).
//! - P2 no audio is played twice or out of order, and a confirmed source is never displaced by an
//!   older one.
//! - P3 concealment only where a packet is truly absent: on a network without jitter, every
//!   delivered packet is played, and between two played packets exactly the missing stream time is
//!   replaced (no double concealment); FEC is used only for the stretch right before its successor.
//! - P4 latency within bounds: no audio older than [`MAX_AGE_MS`] (from sending to the device) is
//!   ever played, stalls included.
//! - P5 no reset unless two consecutive packets agree on a new timeline (or the device stalled).
//! - P6 memory bounded: never more than [`SLOT_BOUND`] packets held.
//!
//! Generator: a seeded LCG (no extra dependency); every failure message
//! names the seed (`PROP_SEED=n` runs one).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use super::audio_mixer::Pacer;
use super::playout::{FrameDecoder, TrackPlayout, FRAME_SAMPLES};
use crate::audio::OutputLevel;

/// The documented bound on packets held per track.
const SLOT_BOUND: usize = 25;

/// Oldest audio (from sending to the device) that may ever be played:
/// jitter target (160 ms) + long-packet reserve (≤ 200 ms) + output queue
/// (≤ 420 ms for a device with one block size) + network jitter (≤ 60 ms)
/// + one 120 ms packet + margin.
const MAX_AGE_MS: u64 = 1_100;

/// The same for a device whose block size varies: its output queue also
/// holds the spread between its smallest and largest block (≤ 390 ms).
const MAX_AGE_VARYING_MS: u64 = 1_500;

/// Timeline marker for output padding (silence the pacer adds to fill the
/// device queue; not part of any stream).
const PADDING: u32 = u32::MAX - 1;

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

/// What the decoder was asked to do, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Decode {
        epoch: u8,
        seq: u16,
        ts: u32,
        len: u32,
    },
    Conceal {
        len: u32,
    },
    Fec {
        len: u32,
        epoch: u8,
        seq: u16,
    },
}

/// Payload `[epoch, seq_hi, seq_lo, duration in 2.5 ms units, fec, ts…4]`.
/// Decoding fills the samples with `epoch * 100_000 + seq` and logs the
/// call; concealment and FEC fill -1.
struct TagDecoder {
    log: Rc<RefCell<Vec<Event>>>,
}

fn parse(packet: &[u8]) -> (u8, u16, u32, bool, u32) {
    let seq = u16::from_be_bytes([packet[1], packet[2]]);
    let len = u32::from(packet[3]) * 120;
    let ts = u32::from_be_bytes([packet[5], packet[6], packet[7], packet[8]]);
    (packet[0], seq, len, packet[4] == 1, ts)
}

impl FrameDecoder for TagDecoder {
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
        let (epoch, seq, len, _, ts) = parse(packet);
        self.log.borrow_mut().push(Event::Decode {
            epoch,
            seq,
            ts,
            len,
        });
        let tag = u32::from(epoch) * 100_000 + u32::from(seq);
        out[..len as usize * 2].fill(tag as f32);
        Ok(len as usize)
    }
    fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String> {
        let (epoch, seq, ..) = parse(next);
        self.log.borrow_mut().push(Event::Fec {
            len: (out.len() / 2) as u32,
            epoch,
            seq,
        });
        out.fill(-1.0);
        Ok(out.len() / 2)
    }
    fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
        self.log.borrow_mut().push(Event::Conceal {
            len: (out.len() / 2) as u32,
        });
        out.fill(-1.0);
        Ok(out.len() / 2)
    }
    fn can_recover(&self, next: &[u8], lost_per_channel: usize) -> bool {
        let (_, _, len, fec, _) = parse(next);
        fec && lost_per_channel >= len as usize
    }
    fn duration(&self, packet: &[u8]) -> Option<u32> {
        Some(parse(packet).2)
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
    fec: bool,
    /// A copy, an outlier, an inconsistent packet or a late packet of a
    /// retired source: not expected to be played.
    odd: bool,
}

#[derive(Debug, Clone, Copy)]
enum Blocks {
    Constant(u64),
    /// 20, 20, 20, then `n` ms, repeated.
    Pattern(u64),
    /// A new random block size every two seconds.
    Wander,
}

/// One case in this many is steady (see [`generate`]).
const STEADY_EVERY: u64 = 4;

#[derive(Debug)]
struct Case {
    seed: u64,
    blocks: Blocks,
    stall: Option<(u64, u64)>,
    jitter_ms: u64,
    loss_pct: u64,
    drift: f64,
    arrivals: Vec<Arrival>,
    send_ms: HashMap<(u8, u16), u64>,
    restarts: usize,
    pauses: usize,
    odd_packets: usize,
    /// Packet-duration changes (the long-packet reserve then moves).
    dur_changes: usize,
    /// The packet duration at the end (the only one in steady cases).
    last_dur_ms: u64,
    end_ms: u64,
}

impl Case {
    /// No jitter, no drift, no pauses, no odd packets, no restart, no
    /// stall: every gap between two played packets is a known loss.
    fn clean(&self) -> bool {
        self.jitter_ms == 0
            && self.drift == 0.0
            && self.pauses == 0
            && self.odd_packets == 0
            && self.restarts == 0
            && self.stall.is_none()
    }
}

fn generate(seed: u64) -> Case {
    let mut rng = Rng(seed);
    let blocks = match rng.below(3) {
        0 => Blocks::Constant(rng.pick(&[10, 20, 21, 43, 50, 100, 160, 200, 300, 400])),
        1 => Blocks::Pattern(rng.pick(&[60, 100, 200])),
        _ => Blocks::Wander,
    };
    let stall = rng
        .chance(20)
        .then(|| (3_000 + rng.below(8_000), rng.pick(&[1_500u64, 3_000])));
    let jitter_ms = rng.pick(&[0, 0, 0, 0, 10, 30, 60]);
    let loss_pct = rng.pick(&[0, 0, 2, 5, 15]);
    let burst_pct = rng.pick(&[0, 1, 2]);
    let fec_pct = rng.pick(&[0, 50, 100]);
    let dup_pct = rng.pick(&[0, 0, 3]);
    let drift = rng.pick(&[0.0, 0.0, 0.001, -0.001]);
    let honest = rng.chance(50);
    // No pauses, outliers, restarts, odd packets.
    let calm = rng.chance(40);
    // Every fourth case is steady: a clean stream of one packet duration
    // (losses and any device blocks allowed), where no drift adjustment
    // beyond what block changes, a settling target and losses explain may
    // happen (see `Observed::block_budget`). Drawn
    // after the other choices, so the other cases stay as they were.
    let steady = seed.is_multiple_of(STEADY_EVERY);
    let (stall, jitter_ms, drift) = if steady {
        (None, 0, 0.0)
    } else {
        (stall, jitter_ms, drift)
    };
    let calm = calm || steady;
    let durations = [10u64, 20, 20, 20, 40, 60, 120];
    let mut dur_ms = rng.pick(&durations);
    let (mut ssrc, mut epoch) = (rng.next() as u32, 0u8);
    let mut seq = if rng.chance(30) {
        65_000 + rng.below(500) as u16
    } else {
        rng.below(30_000) as u16
    };
    let mut ts = rng.next() as u32;
    let mut t_send = 0.0f64;
    let mut marker = true;
    let (mut restarts, mut pauses, mut odd_packets, mut dur_changes) = (0, 0, 0, 0);
    let mut arrivals = Vec::new();
    let mut send_ms = HashMap::new();
    let mut burst_left = 0u64;
    let mut last_was_odd = false;
    let mut retired: Option<(u32, u16, u32, u8)> = None;
    let end_ms = 20_000u64;
    while (t_send as u64) < end_ms - 1_000 {
        let mut after_pause = false;
        if !calm && rng.chance(1) {
            after_pause = true;
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
            let new = if steady { dur_ms } else { new };
            if new != dur_ms {
                dur_changes += 1;
            }
            dur_ms = new;
        }
        if !calm && restarts == 0 && rng.below(1_500) == 0 {
            retired = Some((ssrc, seq, ts, epoch));
            ssrc = rng.next() as u32;
            epoch = 1;
            seq = rng.below(30_000) as u16;
            ts = rng.next() as u32;
            marker = true;
            restarts += 1;
        }
        if burst_left == 0 && rng.chance(burst_pct) {
            burst_left = 1 + rng.below(6);
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
        send_ms.insert((epoch, seq), t_send as u64);
        let fec = rng.chance(fec_pct);
        if !lost {
            let arrival = Arrival {
                at_ms: at,
                ssrc,
                seq,
                ts,
                marker,
                epoch,
                dur_ms,
                fec,
                odd: false,
            };
            if rng.chance(dup_pct) {
                let mut copy = arrival.clone();
                copy.at_ms += rng.below(80);
                copy.odd = true;
                arrivals.push(copy);
            }
            arrivals.push(arrival);
        }
        // Odd packets now and then (never two in a row, never right after a
        // pause, where an outlier looks like the pause's honest jump).
        if !calm && !after_pause && !last_was_odd && rng.below(300) == 0 {
            let kind = rng.below(3);
            let (oseq, ots) = match kind {
                // Timestamp 0.7–0.9 s ahead.
                0 => (
                    seq.wrapping_add(1),
                    ts.wrapping_add(rng.pick(&[700u32, 800, 900]) * 48),
                ),
                // Sequence far ahead, timestamp near.
                1 => (seq.wrapping_add(60), ts.wrapping_add((dur_ms * 48) as u32)),
                // Sequence near, timestamp far behind.
                _ => (seq.wrapping_add(1), ts.wrapping_sub(5 * 48_000)),
            };
            arrivals.push(Arrival {
                at_ms: at + 1,
                ssrc,
                seq: oseq,
                ts: ots,
                marker: false,
                epoch,
                dur_ms,
                fec: false,
                odd: true,
            });
            odd_packets += 1;
            last_was_odd = true;
        } else {
            last_was_odd = false;
        }
        marker = false;
        seq = seq.wrapping_add(1);
        ts = ts.wrapping_add((dur_ms * 48) as u32);
        t_send = (dur_ms as f64).mul_add(1.0 + drift, t_send);
        // Two late packets of the retired source, 150 ms after the switch.
        if let Some((rssrc, rseq, rts, repoch)) = retired.take() {
            for k in 0..2u16 {
                arrivals.push(Arrival {
                    at_ms: t_send as u64 + 150 + u64::from(k),
                    ssrc: rssrc,
                    seq: rseq.wrapping_sub(2 - k),
                    ts: rts.wrapping_sub(u32::from(2 - k) * 960),
                    marker: false,
                    epoch: repoch,
                    dur_ms: 20,
                    fec: false,
                    odd: true,
                });
                odd_packets += 1;
            }
        }
    }
    arrivals.sort_by_key(|a| a.at_ms);
    Case {
        seed,
        blocks,
        stall,
        jitter_ms,
        loss_pct,
        drift,
        arrivals,
        send_ms,
        restarts,
        pauses,
        odd_packets,
        dur_changes,
        last_dur_ms: dur_ms,
        end_ms,
    }
}

/// What a run of a case observed; `violations` collects broken properties.
#[derive(Debug, Default)]
struct Observed {
    violations: Vec<String>,
    delivered: HashSet<(u8, u16)>,
    flushed: usize,
    first_of_new_source: usize,
    resets: u64,
    /// Resynchronisations while the device stalled (packets kept coming).
    stall_resets: u64,
    /// Drift adjustments a clean stream of one packet duration may make
    /// after the warm-up: per change of the device's block (draining a
    /// reserve that shrank by `d` ms takes `d / 20` skips, plus one), per
    /// 20 ms the target still moves, and per ten lost packets.
    block_budget: u64,
}

impl Observed {
    fn fail(&mut self, what: String) {
        if self.violations.len() < 3 {
            self.violations.push(what);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run(case: &Case) -> Observed {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut playout = TrackPlayout::new(TagDecoder {
        log: Rc::clone(&log),
    });
    let level = OutputLevel::new();
    let stream = level.begin_stream();
    let mut pacer = Pacer::default();
    // Output queue content: (samples, send time of its audio if exact).
    let mut queue: VecDeque<(usize, Option<u64>)> = VecDeque::new();
    let mut queued = 0usize;
    let mut frame = vec![0.0f32; FRAME_SAMPLES];
    let mut obs = Observed::default();
    let mut next = 0usize;
    let mut last_tag: Option<f32> = None;
    let mut last_ssrc = None;
    let mut rng = Rng(case.seed ^ 0x5eed);
    let mut block_ms = match case.blocks {
        Blocks::Constant(b) => b,
        Blocks::Pattern(_) | Blocks::Wander => 20,
    };
    let mut pattern_pos = 0usize;
    let mut next_callback = block_ms;
    let mut blocks_seen: VecDeque<(u64, u64)> = VecDeque::new();
    let mut quiet_until = 3_000u64; // warm-up
    let mut rendered = 0u64;
    // The output timeline in 2.5 ms units: a packet tag, or None for
    // replacement audio (concealment, FEC, silence); cross-faded units are
    // marked `Some(u32::MAX)`.
    let mut timeline: Vec<Option<u32>> = Vec::new();
    // Where the warm-up ends: timeline position and counters.
    let mut warm: Option<(usize, super::jitter_buffer::BufferStats)> = None;
    let mut warm_target = 0.0f64;
    // The mixer's 5 ms wakes sit at any phase relative to the device.
    let wake_phase = case.seed % 5;
    for t in 0..case.end_ms {
        // Warm-up: the level average spans six 120 ms packets, so a stream
        // of long packets settles its reserve within about six seconds.
        if t == 6_000 {
            warm = Some((timeline.len(), playout.stats().buffer));
            warm_target = playout.stats().target_delay_ms;
        }
        while next < case.arrivals.len() && case.arrivals[next].at_ms <= t {
            let a = &case.arrivals[next];
            let during_stall = case
                .stall
                .is_some_and(|(at, len)| a.at_ms >= at && a.at_ms < at + len + 500);
            if !a.odd && !during_stall {
                obs.delivered.insert((a.epoch, a.seq));
                if last_ssrc.is_some_and(|s| s != a.ssrc) {
                    obs.first_of_new_source += 1;
                }
                last_ssrc = Some(a.ssrc);
            }
            let before = playout.stats().buffer.resets;
            let held = playout.buffered();
            let in_stall = case
                .stall
                .is_some_and(|(at, len)| t >= at && t < at + len + 1_000);
            let units = (a.dur_ms * 2 / 5) as u8;
            let [hi, lo] = a.seq.to_be_bytes();
            let [t0, t1, t2, t3] = a.ts.to_be_bytes();
            playout.push(
                a.ssrc,
                a.seq,
                a.ts,
                a.marker,
                a.at_ms,
                vec![a.epoch, hi, lo, units, u8::from(a.fec), t0, t1, t2, t3],
            );
            if playout.stats().buffer.resets > before {
                if in_stall {
                    obs.stall_resets += 1;
                } else if std::env::var("PROP_DEBUG").is_ok() {
                    let st = playout.stats();
                    eprintln!(
                        "RESET t {t}: seq {} ts {} dur {} held {held} level {:.0} target {:.0} queued {}",
                        a.seq, a.ts, a.dur_ms, st.level_ms, st.target_delay_ms, queued / 96
                    );
                }
                obs.flushed += held;
            }
            if playout.buffered() > SLOT_BOUND {
                obs.fail(format!("P6 t {t}: {} packets held", playout.buffered()));
            }
            next += 1;
        }
        if t % 5 == wake_phase {
            let n = pacer.frames(
                5_000,
                level.queued(),
                level.request_range(),
                level.callbacks(),
            );
            if pacer.take_went_absent() {
                level.request_flush();
                playout.flush();
            }
            let quiet = pacer.silence();
            for i in 0..n {
                let before = playout.stats();
                if i < quiet {
                    frame.fill(0.0);
                } else {
                    playout.next_frame(t, &mut frame);
                }
                if std::env::var("PROP_TRACE").is_ok() {
                    let after = playout.stats();
                    let lg = log.borrow();
                    eprintln!(
                        "TR t {t} lvl {:.0} tgt {:.0} ins {} drp {} und {} lost {} last {:?}",
                        after.level_ms,
                        after.target_delay_ms,
                        after.buffer.drift_inserts - before.buffer.drift_inserts,
                        after.buffer.drift_drops - before.buffer.drift_drops,
                        after.buffer.underruns - before.buffer.underruns,
                        after.buffer.lost - before.buffer.lost,
                        lg.last()
                    );
                }
                // A drift drop cross-fades two frames: its samples carry no
                // exact tag (even when one happens to be an integer).
                let blended = playout.stats().buffer.drift_drops > before.buffer.drift_drops;
                rendered += 1;
                level.produced(FRAME_SAMPLES);
                queued += FRAME_SAMPLES;
                let exact = |s: f32| (!blended && s > 0.0 && s.fract() == 0.0).then_some(s as u32);
                let sent = exact(frame[0]).and_then(|tag| {
                    case.send_ms
                        .get(&((tag / 100_000) as u8, (tag % 100_000) as u16))
                        .copied()
                });
                queue.push_back((FRAME_SAMPLES, sent));
                for unit in frame.chunks(240) {
                    let v = unit[0];
                    timeline.push(if i < quiet {
                        Some(PADDING)
                    } else if blended && v > 0.0 {
                        Some(u32::MAX)
                    } else if v > 0.0 && v.fract() == 0.0 {
                        Some(v as u32)
                    } else if v > 0.0 {
                        Some(u32::MAX)
                    } else {
                        None
                    });
                }
                for sample in [frame[0], frame[FRAME_SAMPLES - 1]] {
                    if blended || sample <= 0.0 || sample.fract() != 0.0 {
                        continue;
                    }
                    if let Some(last) = last_tag {
                        let (le, te) = ((last as u32) / 100_000, (sample as u32) / 100_000);
                        // Order within a source (sequence numbers may wrap).
                        let back = te == le && {
                            let d = (sample as u32 % 100_000) as u16;
                            let l = (last as u32 % 100_000) as u16;
                            (d.wrapping_sub(l) as i16) < 0
                        };
                        if back || te < le {
                            obs.fail(format!("P2 t {t}: {sample} after {last}"));
                        }
                    }
                    last_tag = Some(sample);
                }
            }
        }
        let stalled = case.stall.is_some_and(|(at, len)| t >= at && t < at + len);
        if t >= next_callback && !stalled {
            let block = block_ms as usize * 96;
            // As the output callback does: stale audio is dropped.
            if level.take_flush() {
                let stale = queued;
                queue.clear();
                queued = 0;
                level.consumed(stale);
            }
            if std::env::var("PROP_DEBUG").is_ok() && (5_000..6_300).contains(&t) {
                eprintln!(
                    "CB t {t} block {block_ms} queued {} req {}",
                    queued / 96,
                    level.request() / 96
                );
            }
            level.note_request(stream, block);
            // The device takes its block from the queue's front.
            let mut need = block;
            while need > 0 {
                let Some(front) = queue.front_mut() else {
                    break;
                };
                let take = front.0.min(need);
                if let Some(sent) = front.1 {
                    let age = t.saturating_sub(sent);
                    let bound = match case.blocks {
                        Blocks::Constant(_) => MAX_AGE_MS,
                        Blocks::Pattern(_) | Blocks::Wander => MAX_AGE_VARYING_MS,
                    };
                    if age > bound {
                        obs.fail(format!("P4 t {t}: played audio {age} ms old"));
                    }
                }
                front.0 -= take;
                need -= take;
                if front.0 == 0 {
                    queue.pop_front();
                }
            }
            let taken = block - need;
            queued -= taken;
            level.consumed(taken);
            level.callback_done(stream);
            // A block larger than any of the last two seconds may find the
            // queue short once; so may the first blocks after a stall.
            blocks_seen.retain(|(at, _)| t - *at <= 2_000);
            let larger = blocks_seen.iter().all(|(_, b)| *b < block_ms);
            blocks_seen.push_back((t, block_ms));
            let intact = case.jitter_ms == 0 && case.loss_pct == 0 && case.pauses == 0;
            if need > 0 && t > quiet_until && !larger && intact && block_ms <= 400 {
                obs.fail(format!(
                    "P1 t {t}: device short by {need} samples at {block_ms} ms blocks"
                ));
            }
            // The device plays the block it just took, then asks again.
            next_callback = t + block_ms;
            block_ms = match case.blocks {
                Blocks::Constant(b) => b,
                Blocks::Pattern(n) => {
                    pattern_pos = (pattern_pos + 1) % 4;
                    if pattern_pos == 3 {
                        n
                    } else {
                        20
                    }
                }
                Blocks::Wander => {
                    let new = if t % 2_000 < block_ms {
                        rng.pick(&[10, 20, 40, 100, 200, 300, 400])
                    } else {
                        block_ms
                    };
                    if new != block_ms && t >= 6_000 {
                        obs.block_budget += new.abs_diff(block_ms) / 20 + 1;
                    }
                    new
                }
            };
        }
        if stalled {
            next_callback = t + 1;
            quiet_until = t + 3_000;
        }
        if t % 1000 == 999 {
            level.roll_window();
        }
    }
    // P1: real time.
    let expected = case.end_ms / 20;
    let stall_ms = case.stall.map_or(0, |(_, len)| len);
    if rendered + stall_ms / 20 + 30 < expected || rendered > expected + 30 {
        obs.fail(format!("P1: {rendered} frames for {expected}"));
    }
    obs.resets = playout.stats().buffer.resets;
    if std::env::var("PROP_DEBUG").is_ok() {
        eprintln!("STATS {:?}", playout.stats());
        let l = log.borrow();
        let first_conceal = l.iter().position(|e| matches!(e, Event::Conceal { .. }));
        if let Some(i) = first_conceal {
            eprintln!("LOG {:?}", &l[i.saturating_sub(4)..(i + 8).min(l.len())]);
        }
    }
    let mut stats = playout.stats().buffer;
    let (warm_at, warm_stats) = warm.unwrap_or_default();
    stats.drift_inserts -= warm_stats.drift_inserts;
    stats.drift_drops -= warm_stats.drift_drops;
    // A target still settling after the warm-up (a start-up underrun
    // decays at 2 ms/s) moves the level by a frame per 20 ms it moves.
    let settle = (playout.stats().target_delay_ms - warm_target).abs() / 20.0;
    obs.block_budget += settle.ceil() as u64;
    // A lost packet drains the level by up to a packet before its successor
    // shows the loss; frequent losses pull the average down by that much.
    // One adjustment per ten losses (loss-free cases allow none).
    obs.block_budget += (stats.lost - warm_stats.lost) / 10;
    check_replacement(case, &log.borrow(), &timeline[warm_at..], stats, &mut obs);
    obs
}

/// P3 on clean cases: between two played packets of one source exactly the
/// missing stream time is replaced (measured on the output), FEC only
/// right before its successor.
fn check_replacement(
    case: &Case,
    log: &[Event],
    timeline: &[Option<u32>],
    stats: super::jitter_buffer::BufferStats,
    obs: &mut Observed,
) {
    let mut decoded: HashMap<(u8, u16), (u32, u32)> = HashMap::new();
    for e in log {
        if let Event::Decode {
            epoch,
            seq,
            ts,
            len,
        } = *e
        {
            decoded.insert((epoch, seq), (ts, len));
        }
    }
    if case.jitter_ms == 0 {
        let missing = obs
            .delivered
            .iter()
            .filter(|k| !decoded.contains_key(k))
            .count();
        let allowance = obs.first_of_new_source + obs.flushed + 12;
        if missing > allowance {
            obs.fail(format!(
                "P3: {missing} delivered packets never played (allowed {allowance})"
            ));
        }
    }
    if !case.clean() {
        return;
    }
    // A clean stream needs no drift adjustment, unless the packet duration
    // changes (the long-packet reserve moves) or the device's blocks change
    // after the warm-up (the pacer drains a shrinking reserve by skips).
    if case.dur_changes == 0 && stats.drift_inserts + stats.drift_drops > obs.block_budget {
        obs.fail(format!(
            "P3: {} inserts, {} drops on a clean stream (block changes, target and losses explain {})",
            stats.drift_inserts, stats.drift_drops, obs.block_budget
        ));
    }
    // Replacement between two packets' audio on the output equals the gap,
    // give or take deliberate drift adjustments: an insert adds 20 ms, a
    // drop inside a gap removes 20 ms. Nothing else (no double
    // concealment, no gap left unfilled).
    let (mut excess, mut deficit) = (0i64, 0i64);
    let key = |tag: u32| ((tag / 100_000) as u8, (tag % 100_000) as u16);
    let mut last: Option<u32> = None;
    let mut replaced = 0u32;
    let mut blurred = false;
    for unit in timeline {
        match *unit {
            None => replaced += 120,
            Some(u32::MAX) => blurred = true,
            Some(PADDING) => {}
            Some(tag) if Some(tag) == last => {}
            Some(tag) => {
                if let (Some(prev), false) = (last, blurred) {
                    if let (Some(&(pts, plen)), Some(&(ts, _))) =
                        (decoded.get(&key(prev)), decoded.get(&key(tag)))
                    {
                        let gap = i64::from(ts.wrapping_sub(pts.wrapping_add(plen)) as i32);
                        let diff = i64::from(replaced) - gap;
                        if diff != 0 && std::env::var("PROP_DEBUG").is_ok() {
                            eprintln!(
                                "PAIR {prev}->{tag}: gap {} replaced {}",
                                gap / 48,
                                replaced / 48
                            );
                        }
                        if gap >= 0 && diff > 0 {
                            excess += diff;
                        }
                        if gap >= 0 && diff < 0 {
                            deficit -= diff;
                        }
                        if gap >= 0 && diff % 960 != 0 {
                            obs.fail(format!(
                                "P3: {} ms replaced for a {} ms gap before seq {}",
                                f64::from(replaced) / 48.0,
                                gap / 48,
                                tag % 100_000
                            ));
                        }
                    }
                }
                last = Some(tag);
                replaced = 0;
                blurred = false;
            }
        }
    }
    let inserts = i64::from(stats.drift_inserts as u32) * 960;
    let drops = i64::from(stats.drift_drops as u32) * 960;
    if excess > inserts || deficit > drops {
        obs.fail(format!(
            "P3: {} ms replaced beyond the gaps ({} inserts), {} ms short ({} drops)",
            excess / 48,
            stats.drift_inserts,
            deficit / 48,
            stats.drift_drops
        ));
    }
    // FEC only right before its successor.
    let mut fec_at: Option<(usize, u16)> = None;
    for (i, event) in log.iter().enumerate() {
        match *event {
            Event::Decode { seq, .. } => {
                if let Some((at, fseq)) = fec_at.take() {
                    if at + 1 != i || fseq != seq {
                        obs.fail(format!(
                            "P3: FEC from {fseq} not right before it (seq {seq})"
                        ));
                    }
                }
            }
            Event::Fec { seq, .. } => fec_at = Some((i, seq)),
            Event::Conceal { .. } => {}
        }
    }
}

#[test]
fn receive_path_keeps_its_contract_for_generated_streams() {
    let seeds = std::env::var("PROP_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(1..=400u64, |s: u64| s..=s);
    let mut failures = Vec::new();
    let (mut cases, mut steady) = (0u64, 0u64);
    for seed in seeds {
        let case = generate(seed);
        cases += 1;
        if case.clean() && case.dur_changes == 0 {
            steady += 1;
        }
        let obs = run(&case);
        let tag = format!(
            "seed {seed}: {:?}, stall {:?}, jitter {} ms, loss {} %, {} pauses, {} restarts, {} odd, {} ms packets",
            case.blocks, case.stall, case.jitter_ms, case.loss_pct, case.pauses, case.restarts, case.odd_packets, case.last_dur_ms
        );
        let mut violations = obs.violations;
        let allowed = case.restarts as u64 + obs.stall_resets;
        if obs.resets > allowed {
            violations.push(format!("P5: {} resets (allowed {allowed})", obs.resets));
        }
        for v in violations {
            failures.push(format!("{v} [{tag}]"));
        }
    }
    // The no-adjustment property must keep applying to a real share of the
    // cases, never only to a lucky few.
    if std::env::var("PROP_DEBUG").is_ok() {
        eprintln!("{steady} of {cases} cases clean with one packet duration");
    }
    if cases >= STEADY_EVERY {
        assert!(
            steady * STEADY_EVERY >= cases,
            "only {steady} of {cases} cases are clean with one packet duration"
        );
    }
    assert!(
        failures.is_empty(),
        "{} violations:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
