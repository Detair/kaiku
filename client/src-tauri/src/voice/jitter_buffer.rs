//! Per-track jitter buffer for received Opus RTP audio.
//!
//! Pure logic: no I/O, no clock, no decoder. The caller feeds packets with
//! their arrival time ([`JitterBuffer::push`]). Once per 20 ms playout tick
//! it calls [`JitterBuffer::tick`] and then [`JitterBuffer::pop`] until it
//! holds enough decoded audio for the tick; each answer says what the
//! decoder has to do (decode a packet, rebuild or conceal a lost one, bridge
//! a gap, or nothing). The timeline works in decoded audio, so packets of
//! any Opus duration from 10 ms to 120 ms are handled alike. See
//! `TrackPlayout` in `voice::playout` for the decoder side.
//!
//! Playout position is driven by the RTP timestamp, ordering by the RTP
//! sequence number. Both are extended to `i64` relative to the highest value
//! seen so far, so 16-bit sequence and 32-bit timestamp wrap-around are
//! invisible to the rest of the logic. Packets come from remote peers and
//! are untrusted: a packet whose sequence number and timestamp do not agree
//! with the stream is discarded, and only a second packet that agrees with
//! it restarts the stream.

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

/// Fixed number of packet slots: at most this many packets are held per
/// track (the documented bound). Memory per track is constant.
pub const CAPACITY: usize = 25;

/// Audio held ahead of the play position at most (600 ms); a packet further
/// ahead resynchronises the buffer.
const MAX_SPAN_TS: i64 = 600 * TS_PER_MS;

/// Shortest packet accepted (10 ms); shorter Opus packets are rejected by
/// the caller.
pub const MIN_PACKET_TS: u32 = 480;

/// Longest Opus packet (120 ms) in timestamp units.
pub const MAX_PACKET_TS: u32 = 5760;

/// Packets in the window whose minimum relative transit time is the
/// reference for the jitter estimate (about 2 s).
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

/// Ticks between two drops (0.5 s).
const ADJUST_COOLDOWN_TICKS: u32 = 25;

/// Ticks between two drops while far above the target.
const FAST_COOLDOWN_TICKS: u32 = 2;

/// Ticks after an inserted frame before the next one (100 ms): growing is
/// cheap and losses while the buffer is too small are not.
const INSERT_COOLDOWN_TICKS: u32 = 5;

/// Concealment frames played when the buffer runs dry or a DTX gap starts;
/// after that the track is silent without calling the decoder (100 ms; Opus
/// PLC has faded out by then).
pub const PLC_MAX_FRAMES: u32 = 5;

/// An arrival gap this long starts a new talk spurt: the jitter reference
/// is dropped, timing from before the pause says nothing about now.
const LONG_GAP_MS: u64 = 500;

/// A gap in the timeline longer than this while playing is skipped rather
/// than played out as silence.
const LONG_GAP_TS: i64 = LONG_GAP_MS as i64 * TS_PER_MS;

/// A packet that arrives at least this much later than its predecessor
/// (beyond the packet spacing) with a contiguous timestamp may follow a
/// pause of a sender that does not advance its timestamp over pauses.
const STEP_GAP_MS: u64 = 40;

/// Packets after the gap that decide between a pause and network delay.
const STEP_CONFIRM: usize = 5;

/// A pause shifts the arrival/timestamp relation for good: every packet
/// after the gap, even the earliest, stays at least this much above the
/// old best. A packet back below that shows the network catching up.
const STEP_MIN_TS: i64 = 40 * TS_PER_MS;

/// After a pause the packets keep a steady relation: their spread stays
/// within ordinary network jitter. A wider spread (delay still growing or
/// shrinking) is network behaviour, not a pause.
const STEP_SPREAD_TS: i64 = 40 * TS_PER_MS;

/// A packet this far behind the highest sequence number does not belong to
/// the stream.
const MAX_SEQ_BACK: i64 = 500;

/// A packet this far ahead of the highest sequence number does not belong to
/// the stream.
const MAX_SEQ_AHEAD: i64 = 1000;

/// A timestamp may advance this much more than the sequence numbers and the
/// wall clock explain (1 s); beyond that it does not belong to the stream.
const TS_JUMP_SLACK: i64 = 48_000;

/// A forward timestamp jump may exceed the arrival gap by this much (network
/// jitter after a pause) before it needs confirmation.
const JUMP_TOLERANCE_MS: i64 = 120;

/// Slots short by up to this many while playing: the oldest packets are
/// dropped to make room; further ahead, the buffer resynchronises.
const OVERFLOW_DROP_MAX: i64 = 4;

/// Replaced sources remembered (their late packets are refused).
const RETIRED_SOURCES: usize = 4;

/// How long packets of a replaced source are refused (well beyond any
/// reordering).
const RETIRED_MS: u64 = 10_000;

/// Packets that agree with a discarded one restart the stream when they
/// follow it within this many sequence numbers.
const RESTART_CONFIRM_SEQ: i64 = 3;

/// What the caller has to do to extend its decoded audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playout<P> {
    /// Nothing to play now. Do not touch the decoder; play what is held,
    /// then silence.
    Silence,
    /// Decode this packet.
    Decode(P),
    /// `dur_ts` of audio never arrived in time. `next` is the following
    /// packet when it is already buffered and starts right after the gap:
    /// if it carries in-band FEC, rebuild the gap from it; otherwise
    /// conceal `dur_ts` (PLC). `next` itself is decoded later as usual.
    Lost { next: Option<P>, dur_ts: u32 },
    /// Play `dur_ts` of concealment (PLC; one 20 ms frame, or the rest of
    /// a shorter DTX gap): the buffer ran dry, a DTX gap is being bridged,
    /// or a frame is inserted to grow the buffer.
    Conceal { dur_ts: u32 },
    /// Play `dur_ts` of silence without touching the decoder: the rest of a
    /// DTX gap after the concealment budget.
    Quiet { dur_ts: u32 },
    /// The caller holds enough decoded audio for this tick.
    Carry,
}

/// Result of [`JitterBuffer::push`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// The packet was buffered.
    Stored,
    /// Duplicate, late or not matching the stream: discarded.
    Dropped,
    /// The stream restarted or was resynchronised. The buffer now holds
    /// only this packet; decoder state and held audio must be reset.
    Reset,
}

/// Counters of one buffer. They survive resets (per track lifetime).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BufferStats {
    pub packets_received: u64,
    /// Arrived after a packet with a higher sequence number (still played).
    pub reordered: u64,
    pub duplicates: u64,
    /// Arrived after its audio was already concealed or played: discarded.
    pub late_dropped: u64,
    /// Sequence number and timestamp did not agree with the stream.
    pub discarded: u64,
    /// Packets whose audio did not arrive in time (concealed or skipped).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No packet yet.
    Idle,
    /// Waiting until enough audio is buffered. `fresh`: nothing was played
    /// since the stream (re)started, the decoder has no state to conceal
    /// from. `concealed`: PLC frames played so far. `starved_ts`: audio
    /// played (concealment or silence) since the buffer ran dry; if the
    /// missing packets turn out lost, it already covered part of the gap.
    Buffering {
        fresh: bool,
        concealed: u32,
        starved_ts: i64,
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

/// A discarded packet that a following one may confirm as a restart.
#[derive(Debug, Clone, Copy)]
struct RestartCandidate {
    ssrc: u32,
    seq: u16,
    ts: u32,
    arrival_ms: u64,
}

/// A packet arrived late after a gap with a contiguous timestamp: either
/// the network was late (it catches up) or the sender paused without
/// advancing its timestamp (the new relation holds). Transit samples are
/// held back until that is clear.
#[derive(Debug, Clone, Copy)]
struct StepCandidate {
    /// Best relative transit before the gap.
    old_best: i64,
    samples: [i64; STEP_CONFIRM],
    len: usize,
}

/// Drop decision for the current tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropState {
    None,
    Pending { fast: bool },
    Done,
}

/// Jitter buffer of one remote audio track. `P` is the packet payload.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // independent flags of the control state
pub struct JitterBuffer<P> {
    slots: Vec<Option<Slot<P>>>,
    ssrc: Option<u32>,
    highest: Option<Highest>,
    last_arrival_ms: Option<u64>,
    restart: Option<RestartCandidate>,
    /// Recently replaced sources and until when their packets are refused.
    retired: [Option<(u32, u64)>; RETIRED_SOURCES],
    phase: Phase,
    /// Next sequence number / timestamp to decode.
    cursor_seq: i64,
    cursor_ts: i64,
    /// Duration of the last decoded packet (assumed for lost ones).
    last_dur: i64,
    /// Bit `i` set: sequence `cursor_seq - 1 - i` was played from a packet.
    played_mask: u64,
    /// Relative transit times (arrival − timestamp, in timestamp units).
    transit: [i64; JITTER_WINDOW],
    transit_len: usize,
    transit_pos: usize,
    step: Option<StepCandidate>,
    /// This stream has set the marker bit (it marks its talk spurts).
    marker_seen: bool,
    /// The packet ending the current starvation began a talk spurt.
    pause_seen: bool,
    /// Resumed with the due packet while a step candidate was open: an
    /// underrun unless the candidate turns out to be a pause.
    pending_underrun: bool,
    peak_ms: f64,
    level_ema: f64,
    cooldown: u32,
    insert_due: bool,
    /// The last step concealed a gap; the next packet follows it directly.
    after_gap: bool,
    drop: DropState,
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
            restart: None,
            retired: [None; RETIRED_SOURCES],
            phase: Phase::Idle,
            cursor_seq: 0,
            cursor_ts: 0,
            last_dur: i64::from(FRAME_TS),
            played_mask: 0,
            transit: [0; JITTER_WINDOW],
            transit_len: 0,
            transit_pos: 0,
            step: None,
            marker_seen: false,
            pause_seen: false,
            pending_underrun: false,
            peak_ms: 0.0,
            level_ema: 0.0,
            cooldown: 0,
            insert_due: false,
            after_gap: false,
            drop: DropState::None,
            stats: BufferStats::default(),
        }
    }

    /// Buffer one received packet. `dur_ts` is its audio duration in RTP
    /// timestamp units (960 for 20 ms; the caller only passes
    /// [`MIN_PACKET_TS`]..=[`MAX_PACKET_TS`]), `marker` the RTP marker bit
    /// (start of a talk spurt), `arrival_ms` the local arrival time.
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        ssrc: u32,
        seq: u16,
        ts: u32,
        dur_ts: u32,
        marker: bool,
        arrival_ms: u64,
        payload: P,
    ) -> PushOutcome {
        self.stats.packets_received += 1;
        let dur = i64::from(dur_ts.clamp(MIN_PACKET_TS, MAX_PACKET_TS));
        let packet = RawPacket {
            seq,
            ts,
            dur,
            arrival_ms,
        };
        let Some(high) = self.highest else {
            self.ssrc = Some(ssrc);
            self.anchor(packet, marker, payload);
            return PushOutcome::Stored;
        };

        // Late packets of a source that was replaced never come back in.
        if self
            .retired
            .iter()
            .flatten()
            .any(|&(old, until)| old == ssrc && arrival_ms < until)
        {
            self.stats.discarded += 1;
            return PushOutcome::Dropped;
        }
        let since_last = arrival_ms.saturating_sub(self.last_arrival_ms.unwrap_or(arrival_ms));
        let fits = if self.ssrc == Some(ssrc) {
            relation(&high, seq, ts, since_last)
        } else {
            None
        };
        // A forward timestamp jump must be explained by the time that
        // passed (a pause); otherwise it only counts once a second packet
        // confirms the new timeline.
        let fits = fits.filter(|&(eseq, ets)| {
            let seq_gap = eseq - high.seq;
            seq_gap <= 0
                || (ets - high.ts - seq_gap * high.dur) / TS_PER_MS
                    <= since_last as i64 + JUMP_TOLERANCE_MS
        });
        let Some((eseq, ets)) = fits else {
            // Not part of this stream. A restart only counts when the next
            // packet agrees with this one.
            let confirmed = self
                .restart
                .is_some_and(|c| c.confirmed_by(ssrc, seq, ts, arrival_ms));
            if confirmed {
                self.restart = None;
                let continues = self.ssrc == Some(ssrc)
                    && (1..=RESTART_CONFIRM_SEQ + 1)
                        .contains(&i64::from(seq.wrapping_sub(high.raw_seq) as i16));
                if continues {
                    // Same source, sequence numbers running on: the sender
                    // paused while the network's queue drained (the first
                    // packets after the pause came early). Resume from
                    // here like after any pause; no restart.
                    self.anchor(packet, marker, payload);
                    self.phase = Phase::Buffering {
                        fresh: false,
                        concealed: PLC_MAX_FRAMES,
                        starved_ts: 0,
                    };
                    return PushOutcome::Stored;
                }
                if let Some(old) = self.ssrc.filter(|old| *old != ssrc) {
                    self.retired.rotate_right(1);
                    self.retired[0] = Some((old, arrival_ms + RETIRED_MS));
                }
                self.ssrc = Some(ssrc);
                self.stats.resets += 1;
                self.peak_ms = 0.0;
                self.anchor(packet, marker, payload);
                return PushOutcome::Reset;
            }
            self.restart = Some(RestartCandidate {
                ssrc,
                seq,
                ts,
                arrival_ms,
            });
            self.stats.discarded += 1;
            return PushOutcome::Dropped;
        };
        self.restart = None;

        let seq_gap = eseq - high.seq;
        let starving = matches!(self.phase, Phase::Buffering { fresh: false, .. });
        // A sender that marks its talk spurts advances its timestamps over
        // pauses: steps after a gap are then network delay, always.
        self.marker_seen |= marker;
        let mut step_start = false;
        if seq_gap > 0 {
            // More timeline than the missing packets explain: the sender
            // paused (DTX, or a voice-activity gate with honest timestamps).
            let ts_jump = ets - high.ts - seq_gap * high.dur > i64::from(FRAME_TS);
            if marker || ts_jump || since_last >= LONG_GAP_MS {
                self.clear_transit();
                self.step = None;
                self.pending_underrun = false;
                if starving {
                    self.pause_seen = true;
                }
            } else if (1..=3).contains(&seq_gap)
                && self.step.is_none()
                && !self.marker_seen
                && since_last >= high.dur as u64 / TS_PER_MS as u64 + STEP_GAP_MS
            {
                step_start = true;
            }
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

        // While starving, the play position is stale (the sender may have
        // paused); what is buffered from now on is measured from the oldest
        // new packet.
        let (base_seq, base_ts) = if starving {
            self.first_buffered()
                .map_or((eseq, ets), |(s, t)| (s.min(eseq), t.min(ets)))
        } else {
            (self.cursor_seq, self.cursor_ts)
        };
        // Slots run out by a few while playing (short packets, a high
        // target): drop the oldest packets to make room, as a drift drop,
        // instead of starting over.
        let playing = matches!(self.phase, Phase::Playing { .. });
        let over = eseq - self.cursor_seq - (CAPACITY as i64 - 1);
        if playing && (1..=OVERFLOW_DROP_MAX).contains(&over) && ets + dur - base_ts <= MAX_SPAN_TS
        {
            for _ in 0..over {
                if let Some(slot) = self.take(self.cursor_seq) {
                    self.cursor_ts = slot.ts + slot.dur;
                    self.stats.drift_drops += 1;
                } else {
                    self.cursor_ts += self.last_dur;
                    self.stats.lost += 1;
                }
                self.cursor_seq += 1;
                self.played_mask <<= 1;
            }
        }
        if eseq - self.cursor_seq.max(base_seq) >= CAPACITY as i64
            || ets + dur - base_ts > MAX_SPAN_TS
        {
            // Far ahead of the play position (output stalled, or packets
            // resumed after a long loss burst): resynchronise on this
            // packet. What never arrived in between counts as lost.
            let skipped = (eseq - self.cursor_seq).max(0) as u64;
            self.stats.lost += skipped.saturating_sub(self.buffered() as u64);
            self.stats.resets += 1;
            self.anchor(packet, marker, payload);
            return PushOutcome::Reset;
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
        self.observe_transit(arrival_ms, ets, seq_gap, step_start);
        PushOutcome::Stored
    }

    /// Start a playout tick: per-tick bookkeeping (jitter decay, level
    /// tracking, drift decision). `carry_ts` is decoded audio the caller
    /// already holds. Returns `true` when the caller should drop one frame
    /// this tick: gather two frames ([`JitterBuffer::pop`] with
    /// `want_ts` = 2 frames), cross-fade them into one and call
    /// [`JitterBuffer::dropped`].
    pub fn tick(&mut self, carry_ts: u32) -> bool {
        self.peak_ms = (self.peak_ms - PEAK_DECAY_MS_PER_TICK).max(0.0);
        self.cooldown = self.cooldown.saturating_sub(1);
        let level = self.level_ms() + f64::from(carry_ts) / TS_PER_MS as f64;
        // Long packets make the level swing by a packet each period: the
        // average spans the same number of packets whatever their length.
        let alpha =
            LEVEL_EMA_ALPHA * f64::from(FRAME_TS) / self.last_dur.max(i64::from(FRAME_TS)) as f64;
        self.level_ema = (level - self.level_ema).mul_add(alpha, self.level_ema);
        self.insert_due = false;
        self.drop = DropState::None;
        if matches!(self.phase, Phase::Playing { .. }) && self.cooldown == 0 {
            let target = self.control_target_ms();
            if self.level_ema < target - INSERT_BELOW_MS {
                self.insert_due = true;
            } else if self.level_ema > target + DROP_ABOVE_MS {
                self.drop = DropState::Pending {
                    fast: self.level_ema > target + FAST_DROP_ABOVE_MS,
                };
            }
        }
        self.drop_pending()
    }

    /// Whether this tick's drop still has to be done by the caller.
    pub const fn drop_pending(&self) -> bool {
        matches!(self.drop, DropState::Pending { .. })
    }

    /// The caller dropped a frame (cross-faded two into one).
    pub fn dropped(&mut self) {
        if let DropState::Pending { fast } = self.drop {
            self.stats.drift_drops += 1;
            self.adjusted(-1.0, fast);
            self.drop = DropState::Done;
        }
    }

    /// Next step towards `want_ts` of decoded audio; `carry_ts` is what the
    /// caller holds now. Call [`JitterBuffer::tick`] once per tick first.
    pub fn pop(&mut self, now_ms: u64, carry_ts: u32, want_ts: u32) -> Playout<P> {
        if carry_ts >= want_ts {
            return Playout::Carry;
        }
        // Gathering a second frame for a drop: only from real packets.
        let gathering = carry_ts >= FRAME_TS;
        match self.phase {
            Phase::Idle => Playout::Silence,
            Phase::Buffering { .. } if gathering => Playout::Carry,
            Phase::Buffering { .. } => self.pop_buffering(now_ms, want_ts - carry_ts),
            Phase::Playing { .. } => self.pop_playing(gathering, want_ts - carry_ts),
        }
    }

    /// The level the buffer actually aims at, in milliseconds: the target
    /// plus the reserve long packets need (see `control_target_ms`).
    pub fn playout_target_ms(&self) -> f64 {
        self.control_target_ms()
    }

    /// Drop everything held: the output was absent and it went stale. The
    /// next packet starts afresh (source, refused sources, jitter estimate
    /// and counters stay).
    pub fn flush(&mut self) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.highest = None;
        self.restart = None;
        self.step = None;
        self.pending_underrun = false;
        self.pause_seen = false;
        self.clear_transit();
        self.phase = Phase::Idle;
    }

    /// Current target buffer level in milliseconds (jitter part).
    pub fn target_ms(&self) -> f64 {
        (self.peak_ms + FRAME_MS).clamp(MIN_TARGET_MS, MAX_TARGET_MS)
    }

    /// Long packets need more in reserve: a lost packet is only recognised
    /// (and rebuilt from FEC) when the next one is already here, so the low
    /// point of the level keeps one packet beyond the first frame above the
    /// target. The level then swings by one more packet, so its average sits
    /// another half packet higher. Drift control aims at that average.
    fn control_target_ms(&self) -> f64 {
        1.5f64.mul_add(self.packet_excess_ms(), self.target_ms())
    }

    /// How much longer than one frame the stream's packets are.
    fn packet_excess_ms(&self) -> f64 {
        ((self.last_dur - i64::from(FRAME_TS)).max(0) / TS_PER_MS) as f64
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

    pub const fn stats(&self) -> BufferStats {
        self.stats
    }

    /// `need_ts`: audio still missing for this tick.
    fn pop_buffering(&mut self, now_ms: u64, need_ts: u32) -> Playout<P> {
        let Phase::Buffering {
            fresh,
            concealed,
            starved_ts,
        } = self.phase
        else {
            return Playout::Silence;
        };
        if !fresh {
            if let Some((seq, ts)) = self.first_buffered() {
                let gap = seq - self.cursor_seq;
                let room = ts - self.cursor_ts;
                if room > gap * i64::from(MAX_PACKET_TS) {
                    // More time passed than the missing packets can hold:
                    // the sender paused. Resume from what arrived.
                    self.stats.lost += gap as u64;
                    self.cursor_seq = seq;
                    self.cursor_ts = ts;
                    self.played_mask = 0;
                    self.purge_before_cursor();
                } else if gap > 0 {
                    // Packets were lost and the next one is here: the gap is
                    // known. What was played while waiting already filled
                    // its start; the rest is concealed (or rebuilt from FEC)
                    // right away, the stream goes on without new buffering.
                    let credit = starved_ts.min(room);
                    self.cursor_ts += credit;
                    if self.cursor_ts >= ts {
                        self.stats.lost += gap as u64;
                        self.cursor_seq = seq;
                        self.cursor_ts = ts;
                        self.played_mask <<= gap.min(63);
                        self.purge_before_cursor();
                    }
                    self.pause_seen = false;
                    self.phase = Phase::Playing { gap_frames: 0 };
                    self.insert_due = false;
                    return self.pop_playing(false, need_ts);
                }
            }
        }
        // Start at the high point of the level's swing (see
        // `control_target_ms`); for 20 ms packets that is the target.
        let target = 2.0f64.mul_add(self.packet_excess_ms(), self.target_ms());
        let oldest_arrival = self.slots.iter().flatten().map(|s| s.arrival_ms).min();
        let ready = oldest_arrival.is_some_and(|arrival| {
            self.level_ms() >= target || now_ms.saturating_sub(arrival) as f64 >= target
        });
        if !ready {
            if fresh {
                return Playout::Silence;
            }
            // Fill exactly what this tick still needs, and count it: if the
            // missing packets turn out lost, it covered part of the gap.
            let starved_ts = starved_ts + i64::from(need_ts);
            let concealing = concealed < PLC_MAX_FRAMES;
            self.phase = Phase::Buffering {
                fresh,
                concealed: concealed + u32::from(concealing),
                starved_ts,
            };
            return if concealing {
                Playout::Conceal { dur_ts: need_ts }
            } else {
                Playout::Quiet { dur_ts: need_ts }
            };
        }

        if !fresh {
            // The very packet that was due came back: the network was late,
            // unless the sender paused (talk spurt start, or a step that a
            // few more packets will confirm).
            let due = self
                .slot(self.cursor_seq)
                .is_some_and(|s| s.ts == self.cursor_ts);
            if due && !self.pause_seen {
                if self.step.is_some() {
                    self.pending_underrun = true;
                } else {
                    self.count_underrun();
                }
            }
        }
        self.pause_seen = false;
        self.phase = Phase::Playing { gap_frames: 0 };
        self.level_ema = self.level_ms();
        self.cooldown = ADJUST_COOLDOWN_TICKS;
        self.insert_due = false;
        self.pop_playing(false, need_ts)
    }

    fn pop_playing(&mut self, gathering: bool, need_ts: u32) -> Playout<P> {
        let Phase::Playing { gap_frames } = self.phase else {
            return Playout::Silence;
        };
        let frame = i64::from(FRAME_TS);
        loop {
            match self.slot(self.cursor_seq).map(|s| (s.ts, s.dur)) {
                Some((ts, _)) if ts - self.cursor_ts > LONG_GAP_TS => {
                    // A long pause ahead: skip it instead of playing silence.
                    self.cursor_ts = ts;
                }
                Some((ts, _)) if ts > self.cursor_ts => {
                    if gathering {
                        return Playout::Carry;
                    }
                    // A DTX gap before the next packet: bridge it. Drift is
                    // corrected here for free by shortening or stretching it.
                    if let DropState::Pending { fast } = self.drop {
                        if ts - self.cursor_ts >= 2 * frame {
                            self.cursor_ts += frame;
                            self.stats.drift_drops += 1;
                            self.adjusted(-1.0, fast);
                            self.drop = DropState::Done;
                        }
                    }
                    // The timeline and the decoder advance by the same step:
                    // a frame, or what is left of a shorter gap.
                    let step = if self.insert_due {
                        self.insert_due = false;
                        self.stats.drift_inserts += 1;
                        self.adjusted(1.0, false);
                        frame
                    } else {
                        let step = (ts - self.cursor_ts).min(frame);
                        self.cursor_ts += step;
                        step
                    };
                    let gap_frames = gap_frames + 1;
                    self.phase = Phase::Playing { gap_frames };
                    let dur_ts = step as u32;
                    return if gap_frames <= PLC_MAX_FRAMES {
                        Playout::Conceal { dur_ts }
                    } else {
                        Playout::Quiet { dur_ts }
                    };
                }
                Some((ts, dur)) if ts + dur <= self.cursor_ts => {
                    // Entirely before the play position: nothing to add.
                    let _ = self.take(self.cursor_seq);
                    self.stats.late_dropped += 1;
                    self.cursor_seq += 1;
                    self.played_mask <<= 1;
                }
                Some(_) => {
                    self.phase = Phase::Playing { gap_frames: 0 };
                    // Never between a gap's concealment and the packet after
                    // it (FEC sits right before its successor).
                    if self.insert_due && !gathering && !self.after_gap {
                        self.insert_due = false;
                        self.stats.drift_inserts += 1;
                        self.adjusted(1.0, false);
                        return Playout::Conceal { dur_ts: FRAME_TS };
                    }
                    self.after_gap = false;
                    return match self.take(self.cursor_seq) {
                        Some(slot) => {
                            self.cursor_seq += 1;
                            self.cursor_ts = slot.ts + slot.dur;
                            self.last_dur = slot.dur;
                            self.played_mask = (self.played_mask << 1) | 1;
                            Playout::Decode(slot.payload)
                        }
                        None => Playout::Silence,
                    };
                }
                None if gathering => return Playout::Carry,
                None if self.highest.is_some_and(|h| h.seq > self.cursor_seq) => {
                    // Later packets are here, this one is not: lost (or so
                    // late that it is useless now). The earliest packet that
                    // did arrive fixes the gap; the stretch right before it
                    // is left for its in-band FEC, the rest is concealed
                    // first.
                    let Some((succ_seq, succ_ts, succ_dur)) = self
                        .slots
                        .iter()
                        .flatten()
                        .filter(|s| s.seq > self.cursor_seq)
                        .min_by_key(|s| s.seq)
                        .map(|s| (s.seq, s.ts, s.dur))
                    else {
                        return Playout::Silence;
                    };
                    let missing = succ_seq - self.cursor_seq;
                    let room = succ_ts - self.cursor_ts;
                    self.phase = Phase::Playing { gap_frames: 0 };
                    if room <= 0 {
                        // No time left for them before the next packet.
                        self.stats.lost += missing as u64;
                        self.cursor_seq = succ_seq;
                        self.played_mask <<= missing.min(63);
                        continue;
                    }
                    let fec_part = succ_dur.min(room);
                    let before = room - fec_part;
                    if before > 0 {
                        let dur = before.min(i64::from(MAX_PACKET_TS));
                        self.cursor_ts += dur;
                        return Playout::Lost {
                            next: None,
                            dur_ts: dur as u32,
                        };
                    }
                    self.stats.lost += missing as u64;
                    self.cursor_seq = succ_seq;
                    self.cursor_ts = succ_ts;
                    self.played_mask <<= missing.min(63);
                    self.after_gap = true;
                    let fec = (i64::from(MIN_PACKET_TS)..=i64::from(MAX_PACKET_TS))
                        .contains(&fec_part)
                        && fec_part % 120 == 0;
                    let next = if fec {
                        self.slot(succ_seq).map(|n| n.payload.clone())
                    } else {
                        None
                    };
                    return Playout::Lost {
                        next,
                        dur_ts: fec_part as u32,
                    };
                }
                None => {
                    // Ran dry: conceal what this tick still needs and wait
                    // for packets without moving on.
                    self.phase = Phase::Buffering {
                        fresh: false,
                        concealed: 1,
                        starved_ts: i64::from(need_ts),
                    };
                    return Playout::Conceal { dur_ts: need_ts };
                }
            }
        }
    }

    fn count_underrun(&mut self) {
        self.stats.underruns += 1;
        self.peak_ms = self.target_ms().min(MAX_TARGET_MS - FRAME_MS);
    }

    /// Bookkeeping after inserting (+1) or dropping (−1) a frame.
    fn adjusted(&mut self, direction: f64, fast: bool) {
        self.level_ema = direction.mul_add(FRAME_MS, self.level_ema);
        self.cooldown = if direction > 0.0 {
            INSERT_COOLDOWN_TICKS
        } else if fast {
            FAST_COOLDOWN_TICKS
        } else {
            ADJUST_COOLDOWN_TICKS
        };
    }

    fn first_buffered(&self) -> Option<(i64, i64)> {
        self.slots
            .iter()
            .flatten()
            .min_by_key(|s| s.seq)
            .map(|s| (s.seq, s.ts))
    }

    fn purge_before_cursor(&mut self) {
        let cursor = self.cursor_seq;
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|s| s.seq < cursor) {
                *slot = None;
            }
        }
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

    /// Drop everything and wait for enough audio from `packet` on.
    fn anchor(&mut self, packet: RawPacket, marker: bool, payload: P) {
        for slot in &mut self.slots {
            *slot = None;
        }
        self.clear_transit();
        self.step = None;
        self.marker_seen = marker;
        self.pending_underrun = false;
        self.pause_seen = false;
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
        self.last_dur = packet.dur;
        self.phase = Phase::Buffering {
            fresh: true,
            concealed: 0,
            starved_ts: 0,
        };
        self.slots[slot_index(seq)] = Some(Slot {
            seq,
            ts,
            dur: packet.dur,
            arrival_ms: packet.arrival_ms,
            payload,
        });
        self.last_arrival_ms = Some(packet.arrival_ms);
        self.add_transit(packet.arrival_ms as i64 * TS_PER_MS - ts);
    }

    const fn clear_transit(&mut self) {
        self.transit_len = 0;
        self.transit_pos = 0;
    }

    /// Feed one transit sample, holding samples back while a step after a
    /// gap is unresolved (network late vs. sender paused).
    fn observe_transit(&mut self, arrival_ms: u64, ts: i64, seq_gap: i64, step_start: bool) {
        let relative = arrival_ms as i64 * TS_PER_MS - ts;
        let _ = seq_gap;
        let Some(mut step) = self.step else {
            if step_start {
                let mut samples = [0; STEP_CONFIRM];
                samples[0] = relative;
                let old_best = self.transit[..self.transit_len]
                    .iter()
                    .copied()
                    .min()
                    .unwrap_or(relative);
                self.step = Some(StepCandidate {
                    old_best,
                    samples,
                    len: 1,
                });
            } else {
                self.add_transit(relative);
            }
            return;
        };
        if step.len < STEP_CONFIRM {
            step.samples[step.len] = relative;
            step.len += 1;
        }
        let held = &step.samples[..step.len];
        let low = held.iter().copied().min().unwrap_or(relative);
        let high = held.iter().copied().max().unwrap_or(relative);
        let back_to_old = low < step.old_best + STEP_MIN_TS;
        if !back_to_old && step.len < STEP_CONFIRM {
            self.step = Some(step);
            return;
        }
        self.step = None;
        if !back_to_old && high - low <= STEP_SPREAD_TS {
            // The new relation holds steadily: the sender paused. Start the
            // jitter reference afresh from here.
            self.pending_underrun = false;
            self.clear_transit();
            for &sample in held {
                self.add_transit(sample);
            }
            return;
        }
        // Catching up, or a delay still on the move: network jitter.
        for &sample in held {
            self.add_transit(sample);
        }
        if self.pending_underrun {
            self.pending_underrun = false;
            self.count_underrun();
        }
    }

    /// Track the relative transit time and raise the jitter peak at once
    /// when a packet arrives later than the best of the window.
    fn add_transit(&mut self, relative: i64) {
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

impl RestartCandidate {
    /// `(ssrc, seq, ts)` continues this candidate: same source, a few
    /// sequence numbers on, a timestamp step those packets and the elapsed
    /// time explain.
    fn confirmed_by(&self, ssrc: u32, seq: u16, ts: u32, arrival_ms: u64) -> bool {
        let seq_gap = i64::from(seq.wrapping_sub(self.seq) as i16);
        let ts_gap = i64::from(ts.wrapping_sub(self.ts) as i32);
        let elapsed = arrival_ms.saturating_sub(self.arrival_ms) as i64;
        ssrc == self.ssrc
            && (1..=RESTART_CONFIRM_SEQ).contains(&seq_gap)
            && ts_gap >= 0
            && ts_gap <= seq_gap * i64::from(MAX_PACKET_TS) + elapsed * TS_PER_MS + TS_JUMP_SLACK
    }
}

/// Extended sequence number and timestamp of a packet, if its sequence
/// number and timestamp agree with the stream's highest packet: forward
/// packets may not step back in time nor jump further ahead than the
/// missing packets and the wall clock explain; older packets must be older
/// by a plausible amount; equal sequence numbers need equal timestamps.
/// Both directions need at least 10 ms of timestamp per sequence number.
fn relation(high: &Highest, seq: u16, ts: u32, since_last_ms: u64) -> Option<(i64, i64)> {
    let seq_gap = i64::from(seq.wrapping_sub(high.raw_seq) as i16);
    let ts_gap = i64::from(ts.wrapping_sub(high.raw_ts) as i32);
    let max_packet = i64::from(MAX_PACKET_TS);
    // Every packet holds at least 10 ms: the timestamp moves at least that
    // much per sequence number, at most a longest packet (plus pauses).
    let min_packet = i64::from(MIN_PACKET_TS);
    let agrees = if (1..=MAX_SEQ_AHEAD).contains(&seq_gap) {
        ts_gap >= seq_gap * min_packet
            && ts_gap <= seq_gap * max_packet + since_last_ms as i64 * TS_PER_MS + TS_JUMP_SLACK
    } else if seq_gap == 0 {
        ts_gap == 0
    } else if (-MAX_SEQ_BACK..0).contains(&seq_gap) {
        -ts_gap >= -seq_gap * min_packet && -ts_gap <= -seq_gap * max_packet + TS_JUMP_SLACK
    } else {
        false
    };
    agrees.then_some((high.seq + seq_gap, high.ts + ts_gap))
}

const fn slot_index(seq: i64) -> usize {
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
                false,
                arrival_ms,
                (self.tag, i),
            )
        }
    }

    /// One playout tick as `TrackPlayout` does it, without audio: `carry`
    /// is the decoded audio held, in timestamp units (all test packets are
    /// 20 ms). Returns what the decoder was asked to do.
    fn play(jb: &mut JitterBuffer<Pkt>, carry: &mut u32, now: u64) -> Vec<Playout<Pkt>> {
        let drop = jb.tick(*carry);
        let want = if drop { 2 * FRAME_TS } else { FRAME_TS };
        let mut acts = Vec::new();
        for _ in 0..8 {
            let p = jb.pop(now, *carry, want);
            match &p {
                Playout::Carry | Playout::Silence => {
                    if acts.is_empty() {
                        acts.push(p);
                    }
                    break;
                }
                Playout::Decode(_) => *carry += FRAME_TS,
                Playout::Conceal { dur_ts } | Playout::Quiet { dur_ts } => *carry += dur_ts,
                Playout::Lost { dur_ts, .. } => *carry += dur_ts,
            }
            acts.push(p);
        }
        if jb.drop_pending() && *carry >= 2 * FRAME_TS {
            *carry -= 2 * FRAME_TS;
            jb.dropped();
        } else {
            *carry = carry.saturating_sub(FRAME_TS);
        }
        acts
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
        let mut carry = 0;
        for tick in ticks {
            let now = tick * 20;
            while next < arrivals.len() && arrivals[next].0 <= now {
                stream.push(jb, arrivals[next].1, arrivals[next].0);
                next += 1;
            }
            out.extend(play(jb, &mut carry, now));
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
            if let Playout::Decode((_, i)) = p {
                v.push(*i);
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
                next: Some((1, 51)),
                dur_ts: FRAME_TS
            }]
        );
        let d = decoded(&out);
        assert!(!d.contains(&50));
        assert!(d.contains(&49) && d.contains(&51));
        assert_eq!(jb.stats().lost, 1);
        assert_eq!(count(&out, |p| matches!(p, Playout::Conceal { .. })), 0);
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
            matches!(p, Playout::Conceal { .. } | Playout::Lost { .. })
        });
        // (plus the exact rest of the gap once its end is known)
        assert!(concealing as u32 <= PLC_MAX_FRAMES + 2, "{concealing}");
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
        let mut carry = 0;
        for tick in 0..100u64 {
            a.push(&mut jb, tick as u32, tick * 20);
            play(&mut jb, &mut carry, tick * 20);
        }
        // A single packet of a new source is not trusted yet; the next one
        // confirms the new stream.
        assert_eq!(b.push(&mut jb, 0, 2000), PushOutcome::Dropped);
        let mut after = Vec::new();
        for tick in 100..200u64 {
            if tick > 100 {
                let outcome = b.push(&mut jb, (tick - 100) as u32, tick * 20);
                if tick == 101 {
                    assert_eq!(outcome, PushOutcome::Reset, "new SSRC must reset");
                }
            }
            let acts = play(&mut jb, &mut carry, tick * 20);
            // Until the reset, stream A may finish its current frame.
            if tick > 100 {
                after.extend(acts);
            }
        }
        // Nothing from stream A is played after the reset.
        assert!(after.iter().all(|p| match p {
            Playout::Decode((tag, _)) => *tag == 2,
            Playout::Lost {
                next: Some((tag, _)),
                ..
            } => *tag == 2,
            _ => true,
        }));
        let d = decoded(&after);
        assert_eq!(d[0], 1);
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
        assert!(count(&out, |p| matches!(p, Playout::Conceal { .. })) as u32 <= PLC_MAX_FRAMES);
        let st = jb.stats();
        assert_eq!(st.underruns, 0, "{st:?}");
        assert_eq!(st.lost, 0);
        assert!(jb.target_ms() <= MIN_TARGET_MS + 1e-9, "{}", jb.target_ms());
    }

    /// Same source, sequence numbers running on, the timestamp 10 s ahead
    /// without a matching gap: discarded at first; once a second packet
    /// confirms the new timeline, playback resumes from it like after a
    /// pause (no restart, the jitter estimate is kept).
    #[test]
    fn confirmed_timestamp_jump_resumes_like_a_pause() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        let mut carry = 0;
        for i in 0..50u32 {
            s.push(&mut jb, i, u64::from(i) * 20);
            play(&mut jb, &mut carry, u64::from(i) * 20);
        }
        // Next sequence number, but the timestamp is 10 s ahead with no
        // matching wall-clock gap: discarded until the next packet agrees.
        let jump = 50 * FRAME_TS + 480_000;
        let outcome = jb.push(0xA1, 50, jump, FRAME_TS, false, 1000, (1, 50));
        assert_eq!(outcome, PushOutcome::Dropped);
        assert_eq!(jb.stats().discarded, 1);
        let outcome = jb.push(0xA1, 51, jump + FRAME_TS, FRAME_TS, false, 1020, (1, 51));
        assert_eq!(outcome, PushOutcome::Stored);
        assert_eq!(jb.stats().resets, 0);
        assert_eq!(jb.buffered(), 1);
    }

    #[test]
    fn confirmed_sequence_jump_resets() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        let mut carry = 0;
        for i in 0..50u32 {
            s.push(&mut jb, i, u64::from(i) * 20);
            play(&mut jb, &mut carry, u64::from(i) * 20);
        }
        assert_eq!(s.push(&mut jb, 5000, 1000), PushOutcome::Dropped);
        assert_eq!(s.push(&mut jb, 5001, 1020), PushOutcome::Reset);
        assert_eq!(jb.stats().resets, 1);
    }

    #[test]
    fn dtx_gap_is_not_loss() {
        // DTX: sequence numbers stay contiguous, the timestamp jumps over
        // the silent stretch and the packet arrives at its time.
        let mut jb = JitterBuffer::new();
        let mut out = Vec::new();
        let mut carry = 0;
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
                jb.push(7, i as u16, frame * FRAME_TS, FRAME_TS, false, now, (1, i));
            }
            out.extend(play(&mut jb, &mut carry, now));
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
        let mut carry = 0;
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
                let level = jb.level_ms() + f64::from(carry) / 48.0;
                min_level = min_level.min(level);
                max_level = max_level.max(level);
            }
            play(&mut jb, &mut carry, now_us / 1000);
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

    // ---- pauses, resumption, hostile sequences ----

    /// Ticks every 20 ms over `arrivals` (sorted `(arrival_ms, seq, frame)`),
    /// where `frame` is the packet's timestamp in 20 ms frames.
    fn simulate_frames(
        jb: &mut JitterBuffer<Pkt>,
        arrivals: &[(u64, u32, u32)],
        ticks: std::ops::Range<u64>,
        mut each_tick: impl FnMut(u64, &JitterBuffer<Pkt>),
    ) -> Vec<Vec<Playout<Pkt>>> {
        let mut next = 0;
        let mut out = Vec::new();
        let mut carry = 0;
        for tick in ticks {
            let now = tick * 20;
            while next < arrivals.len() && arrivals[next].0 <= now {
                let (arrival, seq, frame) = arrivals[next];
                jb.push(
                    0x77,
                    seq as u16,
                    frame.wrapping_mul(FRAME_TS),
                    FRAME_TS,
                    false,
                    arrival,
                    (1, seq),
                );
                next += 1;
            }
            out.push(play(jb, &mut carry, now));
            each_tick(tick, jb);
        }
        out
    }

    /// An older desktop sender: stops sending in pauses and continues with
    /// contiguous sequence numbers *and* timestamps.
    #[test]
    fn old_sender_pauses_do_not_count_as_jitter_or_underruns() {
        let mut jb = JitterBuffer::new();
        let mut arrivals = Vec::new();
        let (mut t, mut seq) = (0u64, 0u32);
        for round in 0..10 {
            for _ in 0..50 {
                arrivals.push((t, seq, seq));
                t += 20;
                seq += 1;
            }
            t += if round % 2 == 0 { 150 } else { 350 };
        }
        let end = t / 20;
        let mut max_target = 0.0f64;
        let out = simulate_frames(&mut jb, &arrivals, 0..end, |tick, jb| {
            if tick > 60 {
                max_target = max_target.max(jb.target_ms());
            }
        });
        let st = jb.stats();
        assert_eq!(st.underruns, 0, "{st:?}");
        assert_eq!(st.lost, 0, "{st:?}");
        assert!(max_target <= 60.0, "target rose to {max_target}");
        let d = decoded(&out.concat());
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1));
    }

    /// The current desktop sender: the timestamp runs on over pauses and
    /// the first packet after a pause carries the marker bit.
    #[test]
    fn honest_sender_pauses_keep_the_target_at_its_floor() {
        let mut jb = JitterBuffer::new();
        let mut carry = 0;
        let (mut t, mut seq, mut frame) = (0u64, 0u32, 0u32);
        let mut marker = true;
        let mut max_target = 0.0f64;
        for round in 0..10 {
            for _ in 0..50 {
                jb.push(
                    9,
                    seq as u16,
                    frame * FRAME_TS,
                    FRAME_TS,
                    marker,
                    t,
                    (1, seq),
                );
                marker = false;
                play(&mut jb, &mut carry, t);
                max_target = max_target.max(jb.target_ms());
                t += 20;
                seq += 1;
                frame += 1;
            }
            let pause = if round % 2 == 0 { 8 } else { 18 }; // 160 / 360 ms
            for _ in 0..pause {
                play(&mut jb, &mut carry, t);
                t += 20;
                frame += 1;
            }
            marker = true;
        }
        let st = jb.stats();
        assert_eq!(st.underruns + st.lost + st.resets, 0, "{st:?}");
        assert!(
            max_target <= MIN_TARGET_MS + 1e-9,
            "target rose to {max_target}"
        );
    }

    /// A DTX pause with the timestamp running on: after it, playback waits
    /// for the target delay of *new* audio instead of starting with one
    /// frame because the stale play position makes the buffer look full.
    #[test]
    fn resumption_after_a_dtx_pause_rebuffers_from_what_arrived() {
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..100u32).map(|i| (u64::from(i) * 20, i, i)).collect();
        // 2 s silence: timestamps (and arrival) jump by 100 frames.
        arrivals.extend((100..200u32).map(|i| (u64::from(i + 100) * 20, i, i + 100)));
        let out = simulate_frames(&mut jb, &arrivals, 0..320, |_, _| {});
        let first = out
            .iter()
            .position(|acts| acts.contains(&Playout::Decode((1, 100))))
            .expect("resumed");
        // Packet 100 arrives at tick 200, 101 at tick 201: 40 ms buffered.
        assert_eq!(first, 201);
        assert_eq!(jb.stats().underruns, 0);
    }

    #[test]
    fn lost_first_packet_after_a_dtx_pause_does_not_delay_speech() {
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..100u32).map(|i| (u64::from(i) * 20, i, i)).collect();
        arrivals.extend((101..200u32).map(|i| (u64::from(i + 100) * 20, i, i + 100)));
        let out = simulate_frames(&mut jb, &arrivals, 0..320, |_, _| {});
        let first = out
            .iter()
            .position(|acts| acts.contains(&Playout::Decode((1, 101))))
            .expect("resumed");
        // 101 arrives at tick 201; with 102 at tick 202 it plays at once.
        assert!(first <= 203, "speech started at tick {first}");
        let d = decoded(&out.concat());
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1 || w[0] == 99));
        assert_eq!(jb.stats().resets, 0);
    }

    /// A packet with an older sequence number but a timestamp that does not
    /// fit must not poison the jitter estimate or stall playout.
    #[test]
    fn forged_timestamp_on_an_older_sequence_number_is_ignored() {
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..300u32)
            .filter(|i| *i != 99 && *i != 100)
            .map(|i| (u64::from(i) * 20, i, i))
            .collect();
        // 99 is really lost; 100 comes a little early; then a packet
        // claiming to be 99 arrives, with a timestamp 2^31 ahead, before
        // 99's slot is played.
        arrivals.push((1990, 100, 100));
        arrivals.push((1995, 99, 99 + (1u32 << 31) / FRAME_TS));
        arrivals.sort_unstable();
        let mut max_target = 0.0f64;
        let out = simulate_frames(&mut jb, &arrivals, 0..300, |_, jb| {
            max_target = max_target.max(jb.target_ms());
        });
        assert!(max_target <= MIN_TARGET_MS + 1e-9, "target {max_target}");
        let d = decoded(&out.concat());
        assert!(d.contains(&100) && d.contains(&290), "playout stalled");
        assert!(!d.contains(&99));
        assert_eq!(jb.stats().resets, 0);
    }

    #[test]
    fn a_single_outlier_packet_does_not_restart_the_stream() {
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..300u32).map(|i| (u64::from(i) * 20, i, i)).collect();
        arrivals.push((100 * 20 + 1, 5_000, 5_000));
        arrivals.sort_unstable();
        let out = simulate_frames(&mut jb, &arrivals, 0..300, |_, _| {});
        assert_eq!(jb.stats().resets, 0);
        let d = decoded(&out.concat());
        assert!(d.windows(2).all(|w| w[1] == w[0] + 1), "{d:?}");
        assert!(d.len() >= 295);
    }

    #[test]
    fn a_confirmed_restart_just_behind_the_old_sequence_plays_at_once() {
        // Same SSRC, new random start that lands 10 behind the old numbers,
        // with an unrelated timestamp.
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..100u32).map(|i| (u64::from(i) * 20, i, i)).collect();
        arrivals.extend((0..100u32).map(|k| (u64::from(100 + k) * 20, 90 + k, 70_000 + k)));
        let out = simulate_frames(&mut jb, &arrivals, 0..200, |_, _| {});
        let st = jb.stats();
        assert_eq!(st.resets, 1, "{st:?}");
        // At most the first packet of the new stream is lost to the check.
        let new_seqs: Vec<u32> = decoded(&out[100..].concat())
            .into_iter()
            .filter(|s| *s >= 90)
            .collect();
        assert!(
            new_seqs.len() >= 94,
            "{} new packets played",
            new_seqs.len()
        );
        assert!(st.late_dropped <= 2, "{st:?}");
    }

    #[test]
    fn device_stall_and_resume_play_no_stale_audio() {
        let mut jb = JitterBuffer::new();
        let s = Stream::new(0, 0);
        let mut out = Vec::new();
        let mut carry = 0;
        for tick in 0..1000u64 {
            s.push(&mut jb, tick as u32, tick * 20);
            assert!(jb.buffered() <= CAPACITY);
            // The output device stalls from 4 s to 9 s.
            if !(200..450).contains(&tick) {
                for p in play(&mut jb, &mut carry, tick * 20) {
                    out.push((tick, p));
                }
            }
        }
        let resumed = out
            .iter()
            .find_map(|(tick, p)| match p {
                Playout::Decode((_, i)) if *tick >= 450 => Some(*i),
                _ => None,
            })
            .expect("resumed");
        // During the stall the buffer kept only the newest packets: what
        // plays first is within its 25 slots of the newest one (449).
        assert!(
            resumed >= 449 - CAPACITY as u32,
            "stale packet {resumed} after resume"
        );
        assert!(jb.buffered() <= CAPACITY);
    }

    /// During normal playback, one packet with the next sequence number and
    /// a timestamp 800 ms ahead (within the generic allowance, beyond the
    /// span) must not flush the buffer before a second packet confirms it.
    #[test]
    fn an_unexplained_timestamp_jump_needs_confirmation() {
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..300u32).map(|i| (u64::from(i) * 20, i, i)).collect();
        arrivals[100].2 += 40; // 800 ms ahead, arriving on time
        let out = simulate_frames(&mut jb, &arrivals, 0..300, |_, _| {});
        let st = jb.stats();
        assert_eq!(st.resets, 0, "{st:?}");
        assert_eq!(st.discarded, 1, "{st:?}");
        let d = decoded(&out.concat());
        assert!(
            d.contains(&99) && d.contains(&101) && d.contains(&290),
            "{d:?}"
        );
    }

    /// Old senders on Wi-Fi: pauses with contiguous timestamps and 20–30 ms
    /// of network jitter must still be recognised as pauses.
    #[test]
    fn old_sender_pauses_survive_network_jitter() {
        for jitter in [20u64, 30] {
            let mut jb = JitterBuffer::new();
            let mut rng = Lcg(7);
            let mut arrivals = Vec::new();
            let (mut t, mut seq) = (0u64, 0u32);
            for round in 0..60 {
                for _ in 0..100 {
                    arrivals.push((t + rng.next(jitter), seq, seq));
                    t += 20;
                    seq += 1;
                }
                t += if round % 2 == 0 { 150 } else { 350 };
            }
            arrivals.sort_unstable();
            let (mut sum, mut n) = (0.0, 0.0);
            simulate_frames(&mut jb, &arrivals, 0..t / 20, |tick, jb| {
                if tick > 100 {
                    sum += jb.target_ms();
                    n += 1.0;
                }
            });
            let st = jb.stats();
            let mean = sum / n;
            // A real network of this jitter needs a target of about
            // 40 + jitter; pauses must not push it further.
            assert!(
                mean <= 40.0 + jitter as f64 + 15.0,
                "{jitter} ms: mean target {mean}, {st:?}"
            );
            assert!(st.underruns <= 6, "{jitter} ms: {st:?}");
        }
    }

    /// A network whose delay keeps growing is jitter, not a sender pause.
    #[test]
    fn steadily_growing_delay_is_not_taken_for_a_pause() {
        let mut jb = JitterBuffer::new();
        let mut arrivals: Vec<_> = (0..300u32).map(|i| (u64::from(i) * 20, i, i)).collect();
        // From packet 100 on, each packet 25 ms later than the one before,
        // for eight packets, then the network catches up at once.
        for k in 0..8u32 {
            arrivals[(100 + k) as usize].0 += 60 + u64::from(k) * 25;
        }
        arrivals.sort_unstable();
        simulate_frames(&mut jb, &arrivals, 0..300, |_, _| {});
        let st = jb.stats();
        assert!(st.underruns >= 1, "{st:?}");
        assert!(jb.target_ms() > MIN_TARGET_MS, "{}", jb.target_ms());
    }
}
