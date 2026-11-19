//! Per-track jitter buffer for received Opus RTP audio.
//!
//! Pure logic: no I/O, no clock, no decoder. The caller feeds packets with
//! their arrival time ([`JitterBuffer::push`]) and asks for exactly one
//! 20 ms frame per playout tick ([`JitterBuffer::pop`]). The answer says what
//! the decoder has to do for that tick (decode a packet, recover a lost one,
//! conceal, merge two frames, or stay silent). See `TrackPlayout` in
//! `voice::playout` for the decoder side.
//!
//! Playout position is driven by the RTP timestamp, ordering by the RTP
//! sequence number. Both are extended to `i64` relative to the highest value
//! seen so far, so 16-bit sequence and 32-bit timestamp wrap-around are
//! invisible to the rest of the logic.

/// RTP timestamp units in one 20 ms Opus frame (48 kHz clock).
pub const FRAME_TS: u32 = 960;

/// RTP timestamp units per millisecond (48 kHz clock).
const TS_PER_MS: i64 = 48;

/// Length of one playout tick.
pub const FRAME_MS: f64 = 20.0;

/// Lowest (and initial) target buffer level: the frame being played plus one
/// frame in reserve.
pub const MIN_TARGET_MS: f64 = 40.0;

/// Highest target buffer level. Wi-Fi power-save stalls reach ~100 ms; above
/// this, concealing a late packet is preferred over more delay.
pub const MAX_TARGET_MS: f64 = 160.0;

/// Fixed number of packet slots (500 ms of 20 ms packets). Memory per track
/// is constant; a packet further ahead than this resynchronises the buffer.
pub const CAPACITY: usize = 25;

/// Packets in the window whose minimum relative transit time is the
/// reference for the jitter estimate (2 s at 20 ms).
const JITTER_WINDOW: usize = 100;

/// How fast the jitter peak (and with it the target) shrinks: 2 ms per
/// second of playout, so 160 ms → 40 ms takes about a minute.
const PEAK_DECAY_MS_PER_TICK: f64 = 0.04;

/// Smoothing of the buffer level (exponential moving average per tick).
const LEVEL_EMA_ALPHA: f64 = 1.0 / 16.0;

/// Insert a frame when the smoothed level is this far below the target.
const INSERT_BELOW_MS: f64 = 15.0;

/// Drop a frame when the smoothed level is this far above the target.
const DROP_ABOVE_MS: f64 = 15.0;

/// Above this margin over the target, drop every [`FAST_COOLDOWN_TICKS`].
const FAST_DROP_ABOVE_MS: f64 = 60.0;

/// Ticks between two level adjustments (0.5 s).
const ADJUST_COOLDOWN_TICKS: u32 = 25;

/// Ticks between two adjustments while far above the target.
const FAST_COOLDOWN_TICKS: u32 = 2;

/// Ticks after an inserted frame before the next one (100 ms): growing is
/// cheap and losses while the buffer is too small are not.
const INSERT_COOLDOWN_TICKS: u32 = 5;

/// Concealment frames played when the buffer runs dry or a DTX gap starts;
/// after that the track is silent without calling the decoder (100 ms; Opus
/// PLC has faded out by then).
pub const PLC_MAX_FRAMES: u32 = 5;

/// A starvation that ends within this time with the very packet that was
/// due (same sequence number and timestamp) was an underrun (the network
/// was late). Longer, or with a timestamp jump, it was a pause of the
/// sender (VAD gate or DTX) and does not count.
const SILENCE_GAP_MS: u64 = 200;

/// An arrival gap this long starts a new talk spurt: the jitter reference
/// is dropped, timing from before the pause says nothing about now.
const LONG_GAP_MS: u64 = 500;

/// A packet this far behind the highest sequence number restarts the stream.
const MAX_SEQ_BACK: i64 = 500;

/// A packet this far ahead of the highest sequence number restarts the stream.
const MAX_SEQ_AHEAD: i64 = 1000;

/// Longest Opus packet (120 ms) in timestamp units.
const MAX_PACKET_TS: i64 = 5760;

/// A timestamp that advances this much more than the sequence numbers and
/// the wall clock explain is a discontinuity of the sender (1 s).
const TS_JUMP_SLACK: i64 = 48_000;

/// What the caller has to do for one playout tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playout<P> {
    /// Nothing to play (not started, or silent for longer than the
    /// concealment budget). Do not touch the decoder.
    Silence,
    /// Decode this packet.
    Decode(P),
    /// The packet for this tick never arrived in time. `next` is the
    /// following packet when it is already buffered: if it carries in-band
    /// FEC, decode the lost frame from it; otherwise conceal (PLC).
    /// `next` itself is played on a later tick as usual.
    Lost { next: Option<P> },
    /// Play one concealment frame (PLC): the buffer ran dry, a DTX gap is
    /// being bridged, or a frame is inserted to grow the buffer.
    Conceal,
    /// Drop one frame to shrink the buffer: decode both packets in order
    /// and cross-fade them into one frame.
    Merge(P, P),
    /// The caller still holds at least one frame of decoded audio (from a
    /// packet longer than 20 ms): play that, nothing to decode.
    Carry,
}

/// Result of [`JitterBuffer::push`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// The packet was buffered.
    Stored,
    /// Duplicate, late or out of range: the packet was discarded.
    Dropped,
    /// The stream restarted (new SSRC or a sequence/timestamp jump). The
    /// buffer now holds only this packet; the decoder state must be reset.
    Reset,
}

/// Counters of one buffer. They survive resets (per track lifetime).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BufferStats {
    pub packets_received: u64,
    /// Arrived after a packet with a higher sequence number (still played).
    pub reordered: u64,
    pub duplicates: u64,
    /// Arrived after its frame was already concealed: discarded.
    pub late_dropped: u64,
    /// Frames whose packet did not arrive in time (concealed or skipped).
    pub lost: u64,
    /// The buffer ran dry while the network, not the sender, was late.
    pub underruns: u64,
    /// Frames dropped to shrink the buffer (drift or reduced jitter).
    pub drift_drops: u64,
    /// Frames inserted to grow the buffer (drift or increased jitter).
    pub drift_inserts: u64,
    /// Stream restarts and resynchronisations.
    pub resets: u64,
}

#[derive(Debug, Clone)]
struct Slot<P> {
    seq: i64,
    ts: i64,
    dur: i64,
    arrival_ms: u64,
    payload: P,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// No packet yet.
    Idle,
    /// Waiting until enough audio is buffered. `fresh`: nothing was played
    /// since the stream (re)started, the decoder has no state to conceal
    /// from. `since_ms`: when the buffering started. `concealed`: PLC
    /// frames played so far. `resolved`: the first packet after a
    /// starvation was classified (underrun vs. pause).
    Buffering {
        fresh: bool,
        since_ms: u64,
        concealed: u32,
        resolved: bool,
    },
    /// Playing; `gap_frames` counts frames bridged in a DTX gap.
    Playing { gap_frames: u32 },
}

/// The highest packet seen: extended and raw sequence number and timestamp.
#[derive(Debug, Clone, Copy)]
struct Highest {
    seq: i64,
    ts: i64,
    dur: i64,
    raw_seq: u16,
    raw_ts: u32,
}

/// Jitter buffer of one remote audio track. `P` is the packet payload.
#[derive(Debug)]
pub struct JitterBuffer<P> {
    slots: Vec<Option<Slot<P>>>,
    ssrc: Option<u32>,
    highest: Option<Highest>,
    last_arrival_ms: Option<u64>,
    phase: Phase,
    /// Next sequence number / timestamp to play.
    cursor_seq: i64,
    cursor_ts: i64,
    /// Bit `i` set: sequence `cursor_seq - 1 - i` was played from a packet.
    played_mask: u64,
    /// Relative transit times (arrival − timestamp, in timestamp units).
    transit: [i64; JITTER_WINDOW],
    transit_len: usize,
    transit_pos: usize,
    peak_ms: f64,
    level_ema: f64,
    cooldown: u32,
    stats: BufferStats,
}

impl<P: Clone> Default for JitterBuffer<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Clone> JitterBuffer<P> {
    pub fn new() -> Self {
        Self {
            slots: (0..CAPACITY).map(|_| None).collect(),
            ssrc: None,
            highest: None,
            last_arrival_ms: None,
            phase: Phase::Idle,
            cursor_seq: 0,
            cursor_ts: 0,
            played_mask: 0,
            transit: [0; JITTER_WINDOW],
            transit_len: 0,
            transit_pos: 0,
            peak_ms: 0.0,
            level_ema: 0.0,
            cooldown: 0,
            stats: BufferStats::default(),
        }
    }

    /// Buffer one received packet. `dur_ts` is its audio duration in RTP
    /// timestamp units (960 for 20 ms), `arrival_ms` the local arrival time.
    pub fn push(
        &mut self,
        ssrc: u32,
        seq: u16,
        ts: u32,
        dur_ts: u32,
        arrival_ms: u64,
        payload: P,
    ) -> PushOutcome {
        self.stats.packets_received += 1;
        let dur = i64::from(dur_ts).clamp(1, MAX_PACKET_TS);
        let packet = RawPacket {
            seq,
            ts,
            dur,
            arrival_ms,
        };

        if self.ssrc.is_some_and(|known| known != ssrc) {
            self.ssrc = Some(ssrc);
            self.restart(packet, payload);
            return PushOutcome::Reset;
        }
        self.ssrc = Some(ssrc);
        let Some(high) = self.highest else {
            self.anchor(packet, payload);
            return PushOutcome::Stored;
        };

        // Extend to i64 relative to the highest packet: wrap-safe.
        let eseq = high.seq + i64::from(seq.wrapping_sub(high.raw_seq) as i16);
        let ets = high.ts + i64::from(ts.wrapping_sub(high.raw_ts) as i32);
        let since_last = arrival_ms.saturating_sub(self.last_arrival_ms.unwrap_or(arrival_ms));
        let seq_gap = eseq - high.seq;

        let discontinuity = if !(-MAX_SEQ_BACK..=MAX_SEQ_AHEAD).contains(&seq_gap) {
            true
        } else if seq_gap > 0 {
            let ts_gap = ets - high.ts;
            ts_gap < 0
                || ts_gap > seq_gap * MAX_PACKET_TS + since_last as i64 * TS_PER_MS + TS_JUMP_SLACK
        } else {
            false
        };
        if discontinuity {
            self.restart(packet, payload);
            return PushOutcome::Reset;
        }

        // After a pause of the sender the old timing reference says
        // nothing about the network now.
        if seq_gap > 0 && since_last >= LONG_GAP_MS {
            self.clear_transit();
        }
        if let Phase::Buffering {
            fresh: false,
            resolved: false,
            since_ms,
            concealed,
        } = self.phase
        {
            let paused = arrival_ms.saturating_sub(since_ms) >= SILENCE_GAP_MS
                || (eseq == self.cursor_seq && ets > self.cursor_ts);
            if paused {
                self.clear_transit();
            }
            self.phase = Phase::Buffering {
                fresh: false,
                since_ms,
                concealed,
                resolved: true,
            };
        }

        if eseq < self.cursor_seq {
            let fresh = matches!(self.phase, Phase::Buffering { fresh: true, .. });
            if fresh && high.seq - eseq < CAPACITY as i64 {
                // Nothing played yet: an earlier packet of the first burst
                // arrived late; start from it instead.
                self.cursor_seq = eseq;
                self.cursor_ts = ets;
            } else {
                let back = self.cursor_seq - 1 - eseq;
                if back < 64 && self.played_mask & (1 << back) != 0 {
                    self.stats.duplicates += 1;
                } else {
                    self.stats.late_dropped += 1;
                }
                return PushOutcome::Dropped;
            }
        }

        if eseq - self.cursor_seq >= CAPACITY as i64 {
            // Far ahead of the play position (output stalled, or packets
            // resumed after a long loss burst): resynchronise on this
            // packet. What never arrived in between counts as lost.
            let skipped = (eseq - self.cursor_seq) as u64;
            self.stats.lost += skipped.saturating_sub(self.buffered() as u64);
            self.stats.resets += 1;
            self.anchor(packet, payload);
            return PushOutcome::Stored;
        }

        let index = slot_index(eseq);
        if self.slots[index].as_ref().is_some_and(|s| s.seq == eseq) {
            self.stats.duplicates += 1;
            return PushOutcome::Dropped;
        }
        if eseq < high.seq {
            self.stats.reordered += 1;
        }
        self.slots[index] = Some(Slot {
            seq: eseq,
            ts: ets,
            dur,
            arrival_ms,
            payload,
        });
        if eseq > high.seq {
            self.highest = Some(Highest {
                seq: eseq,
                ts: ets,
                dur,
                raw_seq: seq,
                raw_ts: ts,
            });
        }
        self.last_arrival_ms = Some(
            self.last_arrival_ms
                .map_or(arrival_ms, |l| l.max(arrival_ms)),
        );
        self.add_transit(arrival_ms, ets);
        PushOutcome::Stored
    }

    /// One playout tick. `carry_ts` is decoded audio the caller still holds
    /// (timestamp units); with at least one frame of it, nothing is consumed.
    pub fn pop(&mut self, now_ms: u64, carry_ts: u32) -> Playout<P> {
        self.peak_ms = (self.peak_ms - PEAK_DECAY_MS_PER_TICK).max(0.0);
        self.cooldown = self.cooldown.saturating_sub(1);
        if carry_ts >= FRAME_TS {
            let level = self.level_ms() + f64::from(carry_ts) / TS_PER_MS as f64;
            self.level_ema += (level - self.level_ema) * LEVEL_EMA_ALPHA;
            return Playout::Carry;
        }
        match self.phase {
            Phase::Idle => Playout::Silence,
            Phase::Buffering { .. } => self.pop_buffering(now_ms),
            Phase::Playing { .. } => self.pop_playing(now_ms),
        }
    }

    /// Current target buffer level in milliseconds.
    pub fn target_ms(&self) -> f64 {
        (self.peak_ms + FRAME_MS).clamp(MIN_TARGET_MS, MAX_TARGET_MS)
    }

    /// Audio currently buffered from the play position on, in milliseconds.
    pub fn level_ms(&self) -> f64 {
        match self.highest {
            Some(h) if h.seq >= self.cursor_seq => {
                (h.ts + h.dur - self.cursor_ts).max(0) as f64 / TS_PER_MS as f64
            }
            _ => 0.0,
        }
    }

    /// Number of packets held.
    pub fn buffered(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn stats(&self) -> BufferStats {
        self.stats
    }

    fn pop_buffering(&mut self, now_ms: u64) -> Playout<P> {
        let Phase::Buffering {
            fresh,
            since_ms,
            concealed,
            resolved,
        } = self.phase
        else {
            return Playout::Silence;
        };
        let target = self.target_ms();
        let oldest_arrival = self.slots.iter().flatten().map(|s| s.arrival_ms).min();
        let ready = oldest_arrival.is_some_and(|arrival| {
            self.level_ms() >= target || now_ms.saturating_sub(arrival) as f64 >= target
        });
        if !ready {
            if !fresh && concealed < PLC_MAX_FRAMES {
                self.phase = Phase::Buffering {
                    fresh,
                    since_ms,
                    concealed: concealed + 1,
                    resolved,
                };
                return Playout::Conceal;
            }
            return Playout::Silence;
        }

        if !fresh {
            // The very packet that was due came back soon: the network was
            // late, not the sender quiet. Grow the target by a frame.
            let underrun = self.slot(self.cursor_seq).is_some_and(|s| {
                s.ts == self.cursor_ts && s.arrival_ms.saturating_sub(since_ms) < SILENCE_GAP_MS
            });
            if underrun {
                self.stats.underruns += 1;
                self.peak_ms = self.target_ms().min(MAX_TARGET_MS - FRAME_MS);
            }
        }
        if let Some((seq, ts)) = self
            .slots
            .iter()
            .flatten()
            .min_by_key(|s| s.seq)
            .map(|s| (s.seq, s.ts))
        {
            let gap = seq - self.cursor_seq;
            if gap > 2 {
                // More missing than a short loss: skip to what arrived.
                self.stats.lost += gap as u64;
                self.cursor_seq = seq;
                self.cursor_ts = ts;
                self.played_mask = 0;
            } else if gap == 0 && ts > self.cursor_ts {
                // The sender paused (DTX or VAD gate): we were silent anyway.
                self.cursor_ts = ts;
            }
        }
        self.phase = Phase::Playing { gap_frames: 0 };
        self.level_ema = self.level_ms();
        self.cooldown = ADJUST_COOLDOWN_TICKS;
        self.pop_playing(now_ms)
    }

    fn pop_playing(&mut self, now_ms: u64) -> Playout<P> {
        let Phase::Playing { gap_frames } = self.phase else {
            return Playout::Silence;
        };
        let target = self.target_ms();
        self.level_ema += (self.level_ms() - self.level_ema) * LEVEL_EMA_ALPHA;
        let can_adjust = self.cooldown == 0;
        let fast = self.level_ema > target + FAST_DROP_ABOVE_MS;
        let wants_insert = can_adjust && self.level_ema < target - INSERT_BELOW_MS;
        let wants_drop = can_adjust && self.level_ema > target + DROP_ABOVE_MS;

        match self.slot(self.cursor_seq).map(|s| s.ts) {
            Some(ts) if ts > self.cursor_ts => {
                // A DTX gap before the next packet: bridge it. Drift is
                // corrected here for free by shortening or stretching it.
                if wants_drop && ts - self.cursor_ts >= 2 * i64::from(FRAME_TS) {
                    self.cursor_ts += i64::from(FRAME_TS);
                    self.stats.drift_drops += 1;
                    self.adjusted(-1.0, fast);
                } else if wants_insert {
                    self.stats.drift_inserts += 1;
                    self.adjusted(1.0, fast);
                }
                if !wants_insert {
                    self.cursor_ts = (self.cursor_ts + i64::from(FRAME_TS)).min(ts);
                }
                let gap_frames = gap_frames + 1;
                self.phase = Phase::Playing { gap_frames };
                if gap_frames <= PLC_MAX_FRAMES {
                    Playout::Conceal
                } else {
                    Playout::Silence
                }
            }
            Some(_) => {
                self.phase = Phase::Playing { gap_frames: 0 };
                if wants_insert {
                    self.stats.drift_inserts += 1;
                    self.adjusted(1.0, fast);
                    return Playout::Conceal;
                }
                if wants_drop {
                    if let Some(merged) = self.try_merge() {
                        self.stats.drift_drops += 1;
                        self.adjusted(-1.0, fast);
                        return merged;
                    }
                }
                match self.take(self.cursor_seq) {
                    Some(slot) => {
                        self.advance_played(slot.ts + slot.dur);
                        Playout::Decode(slot.payload)
                    }
                    None => Playout::Silence,
                }
            }
            None if self.highest.is_some_and(|h| h.seq > self.cursor_seq) => {
                // Later packets are here, this one is not: lost (or so late
                // that it is useless now).
                self.stats.lost += 1;
                let next = self
                    .slot(self.cursor_seq + 1)
                    .map(|n| (n.ts, n.payload.clone()));
                let dur = next
                    .as_ref()
                    .map(|(ts, _)| ts - self.cursor_ts)
                    .filter(|d| (1..=MAX_PACKET_TS).contains(d))
                    .unwrap_or_else(|| i64::from(FRAME_TS));
                self.cursor_seq += 1;
                self.cursor_ts += dur;
                self.played_mask <<= 1;
                self.phase = Phase::Playing { gap_frames: 0 };
                Playout::Lost {
                    next: next.map(|(_, payload)| payload),
                }
            }
            None => {
                // Ran dry: conceal and wait for packets without moving on.
                self.phase = Phase::Buffering {
                    fresh: false,
                    since_ms: now_ms,
                    concealed: 1,
                    resolved: false,
                };
                Playout::Conceal
            }
        }
    }

    /// Drop one frame: take the packet at the cursor and the next one when
    /// both are plain consecutive 20 ms frames.
    fn try_merge(&mut self) -> Option<Playout<P>> {
        let first = self.slot(self.cursor_seq)?;
        let second = self.slot(self.cursor_seq + 1)?;
        let frame = i64::from(FRAME_TS);
        if first.dur != frame || second.dur != frame || second.ts != first.ts + frame {
            return None;
        }
        let first = self.take(self.cursor_seq)?;
        self.advance_played(first.ts + first.dur);
        let second = self.take(self.cursor_seq)?;
        self.advance_played(second.ts + second.dur);
        Some(Playout::Merge(first.payload, second.payload))
    }

    /// Bookkeeping after inserting (+1) or dropping (−1) a frame.
    fn adjusted(&mut self, direction: f64, fast: bool) {
        self.level_ema += direction * FRAME_MS;
        self.cooldown = if direction > 0.0 {
            INSERT_COOLDOWN_TICKS
        } else if fast {
            FAST_COOLDOWN_TICKS
        } else {
            ADJUST_COOLDOWN_TICKS
        };
    }

    fn advance_played(&mut self, next_ts: i64) {
        self.cursor_seq += 1;
        self.cursor_ts = next_ts;
        self.played_mask = (self.played_mask << 1) | 1;
    }

    fn slot(&self, seq: i64) -> Option<&Slot<P>> {
        self.slots[slot_index(seq)]
            .as_ref()
            .filter(|s| s.seq == seq)
    }

    fn take(&mut self, seq: i64) -> Option<Slot<P>> {
        let index = slot_index(seq);
        if self.slots[index].as_ref().is_some_and(|s| s.seq == seq) {
            self.slots[index].take()
        } else {
            None
        }
    }

    /// Stream restart: count it and begin again from `packet`.
    fn restart(&mut self, packet: RawPacket, payload: P) {
        self.stats.resets += 1;
        self.peak_ms = 0.0;
        self.anchor(packet, payload);
    }

    /// Drop everything and wait for enough audio from `packet` on.
    fn anchor(&mut self, packet: RawPacket, payload: P) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.clear_transit();
        self.played_mask = 0;
        let seq = i64::from(packet.seq);
        let ts = i64::from(packet.ts);
        self.highest = Some(Highest {
            seq,
            ts,
            dur: packet.dur,
            raw_seq: packet.seq,
            raw_ts: packet.ts,
        });
        self.cursor_seq = seq;
        self.cursor_ts = ts;
        self.phase = Phase::Buffering {
            fresh: true,
            since_ms: packet.arrival_ms,
            concealed: 0,
            resolved: true,
        };
        self.slots[slot_index(seq)] = Some(Slot {
            seq,
            ts,
            dur: packet.dur,
            arrival_ms: packet.arrival_ms,
            payload,
        });
        self.last_arrival_ms = Some(packet.arrival_ms);
        self.add_transit(packet.arrival_ms, ts);
    }

    const fn clear_transit(&mut self) {
        self.transit_len = 0;
        self.transit_pos = 0;
    }

    /// Track the relative transit time and raise the jitter peak at once
    /// when a packet arrives later than the best of the window.
    fn add_transit(&mut self, arrival_ms: u64, ts: i64) {
        let relative = arrival_ms as i64 * TS_PER_MS - ts;
        self.transit[self.transit_pos] = relative;
        self.transit_pos = (self.transit_pos + 1) % JITTER_WINDOW;
        self.transit_len = (self.transit_len + 1).min(JITTER_WINDOW);
        let best = self.transit[..self.transit_len]
            .iter()
            .copied()
            .min()
            .unwrap_or(relative);
        let spread_ms = (relative - best) as f64 / TS_PER_MS as f64;
        self.peak_ms = self.peak_ms.max(spread_ms.min(MAX_TARGET_MS - FRAME_MS));
    }
}

/// Header fields of a packet being pushed.
#[derive(Debug, Clone, Copy)]
struct RawPacket {
    seq: u16,
    ts: u32,
    dur: i64,
    arrival_ms: u64,
}

fn slot_index(seq: i64) -> usize {
    seq.rem_euclid(CAPACITY as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test payload: (stream tag, packet index).
    type Pkt = (u8, u32);

    /// A simulated sender: packet `i` has `seq0 + i`, `ts0 + i * 960`.
    #[derive(Clone, Copy)]
    struct Stream {
        ssrc: u32,
        seq0: u16,
        ts0: u32,
        tag: u8,
    }

    impl Stream {
        const fn new(seq0: u16, ts0: u32) -> Self {
            Self {
                ssrc: 0xA1,
                seq0,
                ts0,
                tag: 1,
            }
        }

        fn push(&self, jb: &mut JitterBuffer<Pkt>, i: u32, arrival_ms: u64) -> PushOutcome {
            jb.push(
                self.ssrc,
                self.seq0.wrapping_add(i as u16),
                self.ts0.wrapping_add(i.wrapping_mul(FRAME_TS)),
                FRAME_TS,
                arrival_ms,
                (self.tag, i),
            )
        }
    }

    /// Run ticks every 20 ms from `start_ms` until `end_ms`, pushing every
    /// scheduled arrival (sorted `(arrival_ms, index)`) that is due before
    /// each tick. Returns what each tick played.
    fn simulate(
        jb: &mut JitterBuffer<Pkt>,
        stream: Stream,
        arrivals: &[(u64, u32)],
        ticks: std::ops::Range<u64>,
        mut each_tick: impl FnMut(&JitterBuffer<Pkt>),
    ) -> Vec<Playout<Pkt>> {
        let mut next = 0;
        let mut out = Vec::new();
        for tick in ticks {
            let now = tick * 20;
            while next < arrivals.len() && arrivals[next].0 <= now {
                stream.push(jb, arrivals[next].1, arrivals[next].0);
                next += 1;
            }
            out.push(jb.pop(now, 0));
            each_tick(jb);
        }
        out
    }

    fn regular(n: u32) -> Vec<(u64, u32)> {
        (0..n).map(|i| (u64::from(i) * 20, i)).collect()
    }

    /// Indices of all decoded packets in play order (merges count both).
    fn decoded(out: &[Playout<Pkt>]) -> Vec<u32> {
        let mut v = Vec::new();
        for p in out {
            match p {
                Playout::Decode((_, i)) => v.push(*i),
                Playout::Merge((_, a), (_, b)) => {
                    v.push(*a);
                    v.push(*b);
                }
                _ => {}
            }
        }
        v
    }

    fn count(out: &[Playout<Pkt>], pred: impl Fn(&Playout<Pkt>) -> bool) -> usize {
        out.iter().filter(|p| pred(p)).count()
    }

    fn assert_target_in_bounds(jb: &JitterBuffer<Pkt>) {
        let t = jb.target_ms();
        assert!(
            (MIN_TARGET_MS..=MAX_TARGET_MS).contains(&t),
            "target {t} ms out of bounds"
        );
    }

    #[test]
    fn in_order_stream_plays_one_frame_per_tick_after_initial_delay() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(1000, 5000);
        let out = simulate(&mut jb, s, &regular(500), 0..500, assert_target_in_bounds);

        let first = out
            .iter()
            .position(|p| matches!(p, Playout::Decode(_)))
            .expect("playback started");
        // Initial delay: at most the 40 ms target.
        assert!(first <= 2, "started after {first} ticks");
        assert!(out[..first].iter().all(|p| *p == Playout::Silence));
        // From then on exactly one decoded packet per tick, in order.
        assert!(out[first..].iter().all(|p| matches!(p, Playout::Decode(_))));
        assert_eq!(decoded(&out), (0..(500 - first) as u32).collect::<Vec<_>>());
        let st = jb.stats();
        assert_eq!(st.lost, 0);
        assert_eq!(st.underruns, 0);
        assert_eq!(st.drift_drops + st.drift_inserts, 0);
        assert!((jb.target_ms() - MIN_TARGET_MS).abs() < 1e-9);
    }

    #[test]
    fn swapped_neighbours_play_in_order() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(7, 77);
        // Pairs arrive swapped: 1,0,3,2,… each in its neighbour's slot.
        let arrivals: Vec<_> = (0..400u32).map(|k| (u64::from(k) * 20, k ^ 1)).collect();
        let out = simulate(&mut jb, s, &arrivals, 0..400, assert_target_in_bounds);
        let d = decoded(&out);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1), "{d:?}");
        assert!(d.len() >= 390);
        let st = jb.stats();
        assert_eq!(st.lost, 0);
        assert_eq!(st.late_dropped, 0);
        assert!(st.reordered >= 190, "{st:?}");
    }

    #[test]
    fn shuffle_within_window_loses_nothing() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(300, 1);
        // Blocks of four arrive as 2,0,3,1 (up to 40 ms late).
        let order = [2u32, 0, 3, 1];
        let arrivals: Vec<_> = (0..600u32)
            .map(|k| (u64::from(k) * 20, k / 4 * 4 + order[(k % 4) as usize]))
            .collect();
        let out = simulate(&mut jb, s, &arrivals, 0..620, assert_target_in_bounds);
        let d = decoded(&out);
        assert_eq!(d, (0..600).collect::<Vec<_>>());
        let st = jb.stats();
        assert_eq!(st.lost, 0);
        assert_eq!(st.late_dropped, 0);
    }

    #[test]
    fn duplicates_play_once() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(65_000, 0);
        let mut arrivals = Vec::new();
        for i in 0..300u32 {
            let t = u64::from(i) * 20;
            arrivals.push((t, i));
            arrivals.push((t, i)); // immediate duplicate
            if i >= 5 {
                arrivals.push((t, i - 5)); // duplicate of an already played one
            }
        }
        let out = simulate(&mut jb, s, &arrivals, 0..300, |_| {});
        let d = decoded(&out);
        let mut sorted = d.clone();
        sorted.dedup();
        assert_eq!(sorted, d, "a packet was played twice");
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
        let st = jb.stats();
        assert_eq!(st.duplicates, 300 + 295);
        assert_eq!(st.lost, 0);
        assert_eq!(st.late_dropped, 0);
    }

    #[test]
    fn sequence_wraps_at_65535() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(65_500, 123_456);
        let out = simulate(&mut jb, s, &regular(200), 0..200, |_| {});
        let d = decoded(&out);
        assert!(d.len() >= 197);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
        assert_eq!(jb.stats().lost, 0);
        assert_eq!(jb.stats().resets, 0);
    }

    #[test]
    fn timestamp_wraps_at_u32_max() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(10, u32::MAX - 20 * FRAME_TS + 7);
        let out = simulate(&mut jb, s, &regular(200), 0..200, |_| {});
        let d = decoded(&out);
        assert!(d.len() >= 197);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
        let st = jb.stats();
        assert_eq!(st.lost, 0);
        assert_eq!(st.resets, 0);
        assert_eq!(st.drift_drops + st.drift_inserts, 0);
    }

    #[test]
    fn single_loss_reports_lost_with_next_packet() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        let arrivals: Vec<_> = regular(200).into_iter().filter(|a| a.1 != 50).collect();
        let out = simulate(&mut jb, s, &arrivals, 0..200, |_| {});
        let lost: Vec<_> = out
            .iter()
            .filter(|p| matches!(p, Playout::Lost { .. }))
            .collect();
        assert_eq!(
            lost,
            vec![&Playout::Lost {
                next: Some((1, 51))
            }]
        );
        let d = decoded(&out);
        assert!(!d.contains(&50));
        assert!(d.contains(&49) && d.contains(&51));
        assert_eq!(jb.stats().lost, 1);
        assert_eq!(count(&out, |p| *p == Playout::Conceal), 0);
    }

    #[test]
    fn late_packet_after_concealment_is_dropped() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        let mut arrivals: Vec<_> = regular(200)
            .into_iter()
            .map(|(t, i)| if i == 50 { (t + 200, i) } else { (t, i) })
            .collect();
        arrivals.sort_unstable();
        let out = simulate(&mut jb, s, &arrivals, 0..200, |_| {});
        let d = decoded(&out);
        assert!(!d.contains(&50), "late packet was played");
        assert!(d.windows(2).all(|w| w[1] > w[0]), "played audio reordered");
        let st = jb.stats();
        assert_eq!(st.late_dropped, 1);
        assert_eq!(st.lost, 1);
    }

    #[test]
    fn burst_loss_longer_than_buffer_conceals_bounded_then_resumes() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(40_000, 9);
        // Packets 100..160 (1.2 s, more than the 500 ms capacity) are lost.
        let arrivals: Vec<_> = regular(400)
            .into_iter()
            .filter(|a| !(100..160).contains(&a.1))
            .collect();
        let mut max_buffered = 0;
        let out = simulate(&mut jb, s, &arrivals, 0..400, |jb| {
            assert_target_in_bounds(jb);
            max_buffered = max_buffered.max(jb.buffered());
        });
        assert!(max_buffered <= CAPACITY);
        // Concealment during the burst is bounded, then silence.
        let concealing = count(&out, |p| {
            matches!(p, Playout::Conceal | Playout::Lost { .. })
        });
        assert!(concealing as u32 <= PLC_MAX_FRAMES, "{concealing}");
        // Clean resume: everything from 160 on plays in order.
        let d = decoded(&out);
        let resumed: Vec<_> = d.iter().copied().filter(|i| *i >= 160).collect();
        assert_eq!(resumed[0], 160);
        assert!(resumed.windows(2).all(|w| w[1] == w[0] + 1));
        assert!(resumed.len() >= 236);
        let st = jb.stats();
        assert_eq!(st.lost, 60);
        assert_eq!(st.underruns, 0);
        assert!(jb.target_ms() <= MIN_TARGET_MS + 1e-9);
    }

    #[test]
    fn ssrc_change_resets_without_stale_audio() {
        let mut jb = JitterBuffer::new();
        let a = Stream::new(100, 1000);
        let b = Stream {
            ssrc: 0xB2,
            seq0: 9,
            ts0: 4_000_000,
            tag: 2,
        };
        let mut out = Vec::new();
        for tick in 0..100u64 {
            a.push(&mut jb, tick as u32, tick * 20);
            out.push(jb.pop(tick * 20, 0));
        }
        assert_eq!(
            b.push(&mut jb, 0, 2000),
            PushOutcome::Reset,
            "new SSRC must reset"
        );
        let mut after = Vec::new();
        for tick in 100..200u64 {
            if tick > 100 {
                b.push(&mut jb, (tick - 100) as u32, tick * 20);
            }
            after.push(jb.pop(tick * 20, 0));
        }
        // Nothing from stream A is played after the switch.
        assert!(after.iter().all(|p| match p {
            Playout::Decode((tag, _)) | Playout::Merge((tag, _), _) => *tag == 2,
            Playout::Lost {
                next: Some((tag, _)),
            } => *tag == 2,
            _ => true,
        }));
        let d = decoded(&after);
        assert_eq!(d[0], 0);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
        assert_eq!(jb.stats().resets, 1);
    }

    #[test]
    fn long_pause_reanchors_without_underrun_or_stale_audio() {
        // A sender that stops sending in silence (VAD gate) and resumes with
        // contiguous sequence numbers and timestamps.
        let mut jb = JitterBuffer::new();
        let s = Stream::new(5, 5);
        let mut arrivals = regular(100);
        // 2 s pause, then packets 100.. continue.
        arrivals.extend((100..300u32).map(|i| (u64::from(i) * 20 + 2000, i)));
        let out = simulate(&mut jb, s, &arrivals, 0..300, assert_target_in_bounds);
        let d = decoded(&out);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
        // The pause is bounded concealment then silence.
        assert!(count(&out, |p| *p == Playout::Conceal) as u32 <= PLC_MAX_FRAMES);
        let st = jb.stats();
        assert_eq!(st.underruns, 0, "{st:?}");
        assert_eq!(st.lost, 0);
        assert!(jb.target_ms() <= MIN_TARGET_MS + 1e-9, "{}", jb.target_ms());
    }

    #[test]
    fn timestamp_jump_resets() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        for i in 0..50u32 {
            s.push(&mut jb, i, u64::from(i) * 20);
            jb.pop(u64::from(i) * 20, 0);
        }
        // Next sequence number, but the timestamp is 10 s ahead with no
        // matching wall-clock gap.
        let outcome = jb.push(0xA1, 50, 50 * FRAME_TS + 480_000, FRAME_TS, 1000, (1, 50));
        assert_eq!(outcome, PushOutcome::Reset);
        assert_eq!(jb.stats().resets, 1);
        assert_eq!(jb.buffered(), 1);
    }

    #[test]
    fn sequence_jump_resets() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        for i in 0..50u32 {
            s.push(&mut jb, i, u64::from(i) * 20);
            jb.pop(u64::from(i) * 20, 0);
        }
        assert_eq!(s.push(&mut jb, 5000, 1000), PushOutcome::Reset);
        assert_eq!(jb.stats().resets, 1);
    }

    #[test]
    fn dtx_gap_is_not_loss() {
        // DTX: sequence numbers stay contiguous, the timestamp jumps over
        // the silent stretch and the packet arrives at its time.
        let mut jb = JitterBuffer::new();
        let mut out = Vec::new();
        let gap_frames = 15u32; // 300 ms of silence
        for tick in 0..300u32 {
            let now = u64::from(tick) * 20;
            let i = if tick < 100 {
                Some(tick)
            } else if tick >= 100 + gap_frames {
                Some(tick - gap_frames)
            } else {
                None
            };
            if let Some(i) = i {
                let frame = if i < 100 { i } else { i + gap_frames };
                jb.push(7, i as u16, frame * FRAME_TS, FRAME_TS, now, (1, i));
            }
            out.push(jb.pop(now, 0));
        }
        let d = decoded(&out);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
        assert!(d.len() >= 280);
        let st = jb.stats();
        assert_eq!(st.lost, 0, "{st:?}");
        assert_eq!(st.underruns, 0, "{st:?}");
        assert_eq!(count(&out, |p| matches!(p, Playout::Lost { .. })), 0);
    }

    #[test]
    fn underrun_grows_target_quickly() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        // A Wi-Fi stall: the packets due in the 200 ms from 4.0 s are held
        // back and delivered in order in one burst at 4.2 s.
        let mut arrivals: Vec<_> = regular(400)
            .into_iter()
            .map(|(t, i)| {
                if (200..210).contains(&i) {
                    (210 * 20, i)
                } else {
                    (t, i)
                }
            })
            .collect();
        arrivals.sort_unstable();
        let mut targets = Vec::new();
        let out = simulate(&mut jb, s, &arrivals, 0..400, |jb| {
            assert_target_in_bounds(jb);
            targets.push(jb.target_ms());
        });
        let st = jb.stats();
        assert!(st.underruns >= 1, "{st:?}");
        assert_eq!(st.lost, 0, "{st:?}");
        // Within 0.5 s after the stall the target is at least one frame up.
        assert!(targets[215] >= MIN_TARGET_MS + FRAME_MS, "{}", targets[215]);
        let d = decoded(&out);
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
    }

    /// Deterministic pseudo random numbers (LCG).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }
    }

    #[test]
    fn jitter_increase_grows_target_then_shrinks() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(1, 1);
        let mut rng = Lcg(42);
        // 0–30 s calm, 30–60 s up to 80 ms random delay, 60–180 s calm.
        let n = 9000u32;
        let mut arrivals: Vec<_> = (0..n)
            .map(|i| {
                let t = u64::from(i) * 20;
                let extra = if (1500..3000).contains(&i) {
                    rng.next(80)
                } else {
                    0
                };
                (t + extra, i)
            })
            .collect();
        arrivals.sort_unstable();
        let mut targets = Vec::new();
        let out = simulate(&mut jb, s, &arrivals, 0..u64::from(n), |jb| {
            assert_target_in_bounds(jb);
            assert!(jb.level_ms() <= MAX_TARGET_MS + 100.0, "{}", jb.level_ms());
            targets.push(jb.target_ms());
        });
        let calm = targets[1400];
        let jittery = targets[2900];
        let end = targets[8990];
        assert!((calm - MIN_TARGET_MS).abs() < 1e-9, "calm {calm}");
        assert!(jittery >= 80.0, "jittery {jittery}");
        assert!(end <= 50.0, "end {end}");
        let d = decoded(&out);
        assert!(d.windows(2).all(|w| w[1] > w[0]));
        // With the grown target, the jittery stretch loses little.
        let st = jb.stats();
        assert!(st.lost + st.late_dropped < 40, "{st:?}");
    }

    /// One simulated hour with the sender's clock off by `ppm`.
    fn drift_hour(period_us: u64) -> (BufferStats, f64, f64) {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        let ticks = 180_000u64; // one hour of 20 ms ticks
        let mut i = 0u32;
        let mut min_level = f64::MAX;
        let mut max_level = 0.0f64;
        for tick in 0..ticks {
            let now_us = tick * 20_000;
            while u64::from(i) * period_us <= now_us {
                s.push(&mut jb, i, u64::from(i) * period_us / 1000);
                i += 1;
            }
            // Level when the tick needs a frame (before taking it).
            if tick > 100 {
                let level = jb.level_ms();
                min_level = min_level.min(level);
                max_level = max_level.max(level);
            }
            jb.pop(now_us / 1000, 0);
            assert_target_in_bounds(&jb);
        }
        (jb.stats(), min_level, max_level)
    }

    #[test]
    fn clock_drift_slow_sender_inserts_within_bounds() {
        // Sender 0.1 % slow: 180 frames short per hour → ~180 inserts.
        let (st, min_level, max_level) = drift_hour(20_020);
        assert!((170..=200).contains(&st.drift_inserts), "{st:?}");
        assert!(st.drift_drops <= 5, "{st:?}");
        assert_eq!(st.underruns, 0, "{st:?}");
        assert_eq!(st.lost, 0, "{st:?}");
        assert!(min_level >= FRAME_MS - 1e-9, "min level {min_level}");
        assert!(max_level <= MIN_TARGET_MS + 40.0, "max level {max_level}");
    }

    #[test]
    fn clock_drift_fast_sender_drops_within_bounds() {
        // Sender 0.1 % fast: 180 frames too many per hour → ~180 drops.
        let (st, min_level, max_level) = drift_hour(19_980);
        assert!((170..=200).contains(&st.drift_drops), "{st:?}");
        assert!(st.drift_inserts <= 5, "{st:?}");
        assert_eq!(st.underruns, 0, "{st:?}");
        assert_eq!(st.lost, 0, "{st:?}");
        assert!(min_level >= FRAME_MS - 1e-9, "min level {min_level}");
        assert!(max_level <= MIN_TARGET_MS + 40.0, "max level {max_level}");
    }

    #[test]
    fn memory_stays_bounded_without_playout() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        for i in 0..10_000u32 {
            s.push(&mut jb, i, u64::from(i) * 20);
            assert!(jb.buffered() <= CAPACITY);
        }
        assert!(jb.level_ms() <= CAPACITY as f64 * FRAME_MS);
    }
}
