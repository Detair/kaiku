//! Audio Mixer
//!
//! Multi-track mixer that turns the received RTP audio of all remote
//! subscribers into a single PCM stream for CPAL playback.
//!
//! Each track owns a [`TrackPlayout`] (jitter buffer + Opus decoder) inside
//! the mixer task. Per output frame every track delivers exactly one decoded
//! or concealed 20 ms frame and the mixer sums them: nothing is discarded
//! here. Frames are produced when the output device needs them
//! ([`OutputLevel`]), so the device clock drives playout and clock drift is
//! absorbed by the jitter buffers.
//!
//! Uses a **command channel** pattern so that `mpsc::Receiver`s stay local to the
//! mixer task and are never placed behind `Arc`/`RwLock`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::audio::OutputLevel;
use crate::voice::playout::{
    FrameDecoder, OpusFrameDecoder, PlayoutStats, TrackPlayout, FRAME_SAMPLES,
};

/// Highest per-user gain (200 %).
pub const MAX_USER_GAIN: f32 = 2.0;

/// How often the mixer wakes to take in packets and top up the output.
const WAKE_INTERVAL: Duration = Duration::from_millis(5);

/// Output queue kept above one device block by this much (one frame).
const OUTPUT_HEADROOM_SAMPLES: usize = FRAME_SAMPLES;

/// Frames produced per wake at most (catch-up after a stall).
const MAX_FRAMES_PER_WAKE: usize = 3;

/// Packets queued per track between the RTP reader and the mixer. The mixer
/// takes up to [`MAX_PACKETS_PER_WAKE`] every 5 ms; a full queue means the
/// mixer stalled (the reader then drops new packets, counted).
pub const PACKET_QUEUE: usize = 16;

/// A packet older than this when the mixer takes it is dropped (counted as
/// expired): after a mixer stall, late audio is not worth playing. With the
/// jitter buffer's 600 ms span this bounds the receive backlog.
const MAX_PACKET_AGE: Duration = Duration::from_millis(500);

/// Packets taken from one track per wake at most, so a flooding peer cannot
/// keep the mixer from mixing (16 per 5 ms is 16× a normal stream).
const MAX_PACKETS_PER_WAKE: usize = 16;

/// How often per-track counters are published.
const STATS_INTERVAL: Duration = Duration::from_secs(1);

/// How often per-track counters are logged (debug level).
const LOG_INTERVAL: Duration = Duration::from_secs(5);

/// One received RTP audio packet, stamped on arrival.
#[derive(Debug, Clone)]
pub struct RtpIn {
    pub ssrc: u32,
    pub seq: u16,
    pub ts: u32,
    /// RTP marker bit: first packet of a talk spurt.
    pub marker: bool,
    pub arrival: Instant,
    pub payload: Vec<u8>,
}

/// Makes the decoder of a new track.
pub type DecoderFactory = Arc<dyn Fn() -> Option<Box<dyn FrameDecoder + Send>> + Send + Sync>;

impl FrameDecoder for Box<dyn FrameDecoder + Send> {
    fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
        (**self).decode(packet, out)
    }
    fn decode_fec(&mut self, next: &[u8], out: &mut [f32]) -> Result<usize, String> {
        (**self).decode_fec(next, out)
    }
    fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
        (**self).conceal(out)
    }
    fn can_recover(&self, next: &[u8], lost_per_channel: usize) -> bool {
        (**self).can_recover(next, lost_per_channel)
    }
    fn duration(&self, packet: &[u8]) -> Option<u32> {
        (**self).duration(packet)
    }
    fn reset(&mut self) {
        (**self).reset();
    }
}

/// Per-person playback gains, keyed by user id (1.0 when unset, 0.0 = muted).
///
/// Shared between the voice state and every mixer, so a volume chosen for
/// someone applies to all of their audio tracks and outlives a single call.
/// Written from commands, read by the mixer task once per 20 ms frame —
/// never from the real-time audio callback.
#[derive(Clone, Default)]
pub struct UserGains(Arc<RwLock<HashMap<Uuid, f32>>>);

impl UserGains {
    /// Set `user_id`'s gain, clamped to `0.0..=MAX_USER_GAIN`.
    pub fn set(&self, user_id: Uuid, gain: f32) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, MAX_USER_GAIN)
        } else {
            1.0
        };
        if let Ok(mut gains) = self.0.write() {
            if (gain - 1.0).abs() < f32::EPSILON {
                gains.remove(&user_id);
            } else {
                gains.insert(user_id, gain);
            }
        }
    }

    /// Gain for `user_id` (1.0 when never set).
    pub fn get(&self, user_id: Uuid) -> f32 {
        self.0
            .read()
            .ok()
            .and_then(|gains| gains.get(&user_id).copied())
            .unwrap_or(1.0)
    }
}

/// Per-track playout counters, readable outside the mixer task.
/// Keyed by track instance; holds the track id with the counters.
pub type PlayoutStatsMap = Arc<Mutex<HashMap<TrackInstance, (String, PlayoutStats)>>>;

/// Identifies one registered track instance. A participant who rejoins gets
/// a new instance with the same track id; removing the old one by its
/// instance never touches the new one.
pub type TrackInstance = u64;

/// Source of track instances, unique for the process lifetime.
static NEXT_INSTANCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A registered track as its RTP reader sees it.
pub struct TrackHandle {
    pub instance: TrackInstance,
    pub packets: mpsc::Sender<RtpIn>,
}

/// One 20 ms frame in microseconds.
const FRAME_US: u64 = 20_000;

/// Decides how many frames the mixer renders per wake.
///
/// Frames are rendered evenly, one per 20 ms of elapsed time, so every
/// track's jitter buffer is ticked at an even pace whatever the device's
/// block size; the output queue absorbs the device's blocks.
///
/// The pace is matched to the device at the queue's low points, right
/// after each callback. The pacer learns the block sizes the device really
/// takes (from the queue's drop at each callback, over the last 5 s) and
/// holds the low point at one frame plus the spread between the largest
/// and the smallest block: before any callback the queue then holds at
/// least the block that callback takes, however the sizes vary. Below the
/// target it renders extra frames at once; above it by a frame or more at
/// every low point of the last second, it skips one frame (at most one per
/// second). Device clock drift thus becomes an occasional extra or skipped
/// tick, which the jitter buffer absorbs like sender drift.
///
/// Rendering never runs more than the target plus the largest block plus
/// two frames ahead. Before the first callback the largest supported block
/// is assumed for up to a second; when no callback comes for a second the
/// device counts as absent: rendering stops three frames ahead and
/// [`Pacer::take_went_absent`] tells the mixer to drop the stale audio.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // independent flags of the pacing state
pub struct Pacer {
    credit_us: u64,
    now_us: u64,
    seen_callbacks: u64,
    since_callback_us: u64,
    /// Observed blocks and low points: `(time, samples)`.
    blocks: History,
    lows: History,
    target: usize,
    last_skip_us: u64,
    skip_pending: bool,
    /// Frames of the last call that are silence.
    silence: usize,
    absent: bool,
    went_absent: bool,
    resumed: bool,
}

/// A small ring of timed samples.
#[derive(Debug)]
struct History {
    items: [(u64, usize); HISTORY_LEN],
    len: usize,
    pos: usize,
}

const HISTORY_LEN: usize = 256;

impl History {
    const fn new() -> Self {
        Self {
            items: [(0, 0); HISTORY_LEN],
            len: 0,
            pos: 0,
        }
    }

    fn push(&mut self, at: u64, value: usize) {
        self.items[self.pos] = (at, value);
        self.pos = (self.pos + 1) % HISTORY_LEN;
        self.len = (self.len + 1).min(HISTORY_LEN);
    }

    /// `(min, max, oldest time)` of the entries since `since`.
    fn range_since(&self, since: u64) -> Option<(usize, usize, u64)> {
        self.items[..self.len]
            .iter()
            .filter(|(at, _)| *at >= since)
            .fold(None, |acc, &(at, v)| match acc {
                None => Some((v, v, at)),
                Some((lo, hi, first)) => Some((lo.min(v), hi.max(v), first.min(at))),
            })
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self {
            credit_us: 0,
            now_us: 0,
            seen_callbacks: 0,
            since_callback_us: 0,
            blocks: History::new(),
            lows: History::new(),
            target: OUTPUT_HEADROOM_SAMPLES,
            last_skip_us: 0,
            skip_pending: false,
            silence: 0,
            absent: false,
            went_absent: false,
            resumed: false,
        }
    }
}

/// Without a device callback for this long, the device counts as absent.
const DEVICE_ABSENT_US: u64 = 1_000_000;

/// How long observed block sizes are remembered.
const BLOCK_MEMORY_US: u64 = 5_000_000;

/// Low points that must all stay above the target before a frame is
/// skipped, and the least time between two skips.
const SKIP_WINDOW_US: u64 = 1_000_000;

/// Least time between two skips while the queue is far (four frames or
/// more) above its target.
const FAST_SKIP_US: u64 = 200_000;

impl Pacer {
    /// Frames to render now, `elapsed_us` after the previous wake, with
    /// `queued` samples waiting for the device, the smallest and largest
    /// block it asked for recently (`blocks`, from [`OutputLevel`]) and its
    /// completed `callbacks`.
    pub fn frames(
        &mut self,
        elapsed_us: u64,
        queued: usize,
        blocks: Option<(usize, usize)>,
        callbacks: u64,
    ) -> usize {
        self.now_us += elapsed_us;
        let cap = MAX_FRAMES_PER_WAKE as u64 * FRAME_US;
        self.credit_us = (self.credit_us + elapsed_us).min(cap);
        let mut due = (self.credit_us / FRAME_US) as usize;
        let mut silence = 0usize;
        self.credit_us %= FRAME_US;
        self.since_callback_us = self.since_callback_us.saturating_add(elapsed_us);
        if !self.absent && callbacks > 0 && self.since_callback_us >= DEVICE_ABSENT_US {
            self.absent = true;
            self.went_absent = true;
        }

        if callbacks != self.seen_callbacks {
            let count = callbacks.wrapping_sub(self.seen_callbacks);
            self.seen_callbacks = callbacks;
            self.since_callback_us = 0;
            let _ = count;
            if std::mem::take(&mut self.absent) {
                self.resumed = true;
            }
            if let Some((smallest, largest)) = blocks {
                self.blocks.push(self.now_us, smallest);
                self.blocks.push(self.now_us, largest);
            }
            // The device has just taken its block: the queue's low point.
            self.lows.push(self.now_us, queued);
            let since = self.now_us.saturating_sub(BLOCK_MEMORY_US);
            let (smallest, largest, _) =
                self.blocks
                    .range_since(since)
                    .unwrap_or((0, 0, self.now_us));
            let room = crate::audio::MAX_QUEUE_SAMPLES - 2 * FRAME_SAMPLES;
            self.target =
                (OUTPUT_HEADROOM_SAMPLES + largest - smallest).min(room - largest.min(room));
            self.target = self.target.max(OUTPUT_HEADROOM_SAMPLES.min(room));
            if queued < self.target {
                let extra = (self.target - queued).div_ceil(FRAME_SAMPLES);
                if extra == 1 {
                    // A frame short: the device clock runs a little fast.
                    // An extra tick, which the jitter buffers absorb.
                    due += 1;
                } else {
                    // Far short (start, a larger block than before, a new
                    // device): fill the queue with silence. Ticking the
                    // jitter buffers in a burst would starve them.
                    silence += extra;
                }
            } else {
                let window = self.now_us.saturating_sub(SKIP_WINDOW_US);
                let lows = self.lows.range_since(window);
                // A large excess (after the device's blocks got smaller)
                // drains faster than drift.
                let far =
                    lows.is_some_and(|(lowest, ..)| lowest >= self.target + 4 * FRAME_SAMPLES);
                let gap = if far { FAST_SKIP_US } else { SKIP_WINDOW_US };
                let settled = self.now_us.saturating_sub(self.last_skip_us) >= gap;
                let all_high = lows.is_some_and(|(lowest, _, first)| {
                    lowest >= self.target + FRAME_SAMPLES && first <= window + 2 * FRAME_US
                });
                if settled && all_high {
                    self.skip_pending = true;
                    self.last_skip_us = self.now_us;
                }
            }
        }

        // A skip takes effect at the next frame that is due.
        if self.skip_pending && due > 0 {
            due -= 1;
            self.skip_pending = false;
        }
        // The ring's capacity bounds rendering; before the first callback
        // the largest supported block plus a frame, without a device three
        // frames. Excess is drained by skipping (above), never by stopping:
        // the tracks keep their even pace.
        let bound = if self.absent {
            3 * FRAME_SAMPLES
        } else if callbacks == 0 {
            crate::audio::MAX_REQUEST_SAMPLES + 3 * FRAME_SAMPLES
        } else {
            crate::audio::MAX_QUEUE_SAMPLES - FRAME_SAMPLES
        };
        while due + silence > 0 && queued + (due + silence) * FRAME_SAMPLES > bound {
            if silence > 0 {
                silence -= 1;
            } else {
                due -= 1;
            }
        }
        let total = (due + silence).min(MAX_FRAMES_PER_WAKE);
        self.silence = silence.min(total);
        total
    }

    /// How many of the frames the last [`Pacer::frames`] asked for are
    /// silence (rendered first, without ticking the tracks).
    pub const fn silence(&self) -> usize {
        self.silence
    }

    /// The device just went absent (no callback for a second), or came
    /// back after that: what is queued and buffered is stale.
    pub fn take_went_absent(&mut self) -> bool {
        std::mem::take(&mut self.went_absent) | std::mem::take(&mut self.resumed)
    }
}

/// A registered remote track.
struct Track {
    /// `"{user_id}:{source}"`.
    track_id: String,
    packets: mpsc::Receiver<RtpIn>,
    playout: TrackPlayout<Box<dyn FrameDecoder + Send>>,
    /// Parsed from the `"{user_id}:{source}"` track id; `None` → unity gain.
    user_id: Option<Uuid>,
    last_log: Instant,
}

/// Commands sent to the mixer run loop.
enum MixerCommand {
    AddTrack {
        instance: TrackInstance,
        track_id: String,
        packets: mpsc::Receiver<RtpIn>,
    },
    RemoveTrack {
        instance: TrackInstance,
    },
}

/// Handle for the background audio mixer task.
///
/// Cloneable and Send — callers interact with the mixer exclusively through
/// the command channel.
#[derive(Clone)]
pub struct AudioMixer {
    command_tx: mpsc::Sender<MixerCommand>,
    gains: UserGains,
    stats: PlayoutStatsMap,
    /// Current instance per track id (a replacement takes over at once).
    current: Arc<Mutex<HashMap<String, TrackInstance>>>,
    /// Set by [`AudioMixer::shutdown`].
    stop: Arc<tokio::sync::watch::Sender<bool>>,
}

impl AudioMixer {
    /// Start a mixer that sends mixed PCM to `output_tx` whenever `level`
    /// says the device needs more, applying the shared per-user `gains`.
    pub fn new(output_tx: mpsc::Sender<Vec<f32>>, gains: UserGains, level: OutputLevel) -> Self {
        let factory: DecoderFactory = Arc::new(|| {
            OpusFrameDecoder::new()
                .map(|d| Box::new(d) as Box<dyn FrameDecoder + Send>)
                .map_err(|e| tracing::error!("Failed to create Opus decoder: {e}"))
                .ok()
        });
        Self::with_decoder(output_tx, gains, level, factory)
    }

    /// Like [`AudioMixer::new`] with a custom decoder (tests).
    pub fn with_decoder(
        output_tx: mpsc::Sender<Vec<f32>>,
        gains: UserGains,
        level: OutputLevel,
        factory: DecoderFactory,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel(32);
        let stats = PlayoutStatsMap::default();
        let (stop, stopped) = tokio::sync::watch::channel(false);
        tokio::spawn(Self::run(
            command_rx,
            output_tx,
            gains.clone(),
            level,
            factory,
            stats.clone(),
            stopped,
        ));
        Self {
            command_tx,
            gains,
            stats,
            current: Arc::default(),
            stop: Arc::new(stop),
        }
    }

    /// Set how loud `user_id` plays (1.0 = unchanged, 0.0 = muted, max 2.0).
    /// Applies to all of the user's audio tracks from the next 20 ms frame.
    pub fn set_user_gain(&self, user_id: Uuid, gain: f32) {
        self.gains.set(user_id, gain);
    }

    /// Register a new audio track instance. Returns its instance id and the
    /// sender for its RTP packets.
    pub async fn add_track(&self, track_id: String) -> TrackHandle {
        let instance = NEXT_INSTANCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut current) = self.current.lock() {
            current.insert(track_id.clone(), instance);
        }
        let (tx, packets) = mpsc::channel(PACKET_QUEUE);
        let _ = self
            .command_tx
            .send(MixerCommand::AddTrack {
                instance,
                track_id,
                packets,
            })
            .await;
        TrackHandle {
            instance,
            packets: tx,
        }
    }

    /// End the mixer now, whatever else still holds a handle (a teardown
    /// that times out, a reader stuck on a silent track). Its output and
    /// every track's packet queue close, which ends the PCM thread and
    /// wakes the RTP readers.
    pub fn shutdown(&self) {
        let _ = self.stop.send(true);
    }

    /// Whether `instance` is still the current instance of `track_id`;
    /// if so, it stops being current (the caller may report the track as
    /// gone). A retired instance gets `false`.
    pub fn release(&self, track_id: &str, instance: TrackInstance) -> bool {
        let Ok(mut current) = self.current.lock() else {
            return false;
        };
        if current.get(track_id) == Some(&instance) {
            current.remove(track_id);
            true
        } else {
            false
        }
    }

    /// Remove one track instance (drops its buffer and decoder). Another
    /// instance with the same track id is not affected.
    pub async fn remove_track(&self, instance: TrackInstance) {
        let _ = self
            .command_tx
            .send(MixerCommand::RemoveTrack { instance })
            .await;
    }

    /// Latest playout counters of every track (published once per second).
    pub fn playout_stats(&self) -> Vec<(String, PlayoutStats)> {
        self.stats
            .lock()
            .map(|map| map.values().map(|(id, v)| (id.clone(), *v)).collect())
            .unwrap_or_default()
    }

    /// Background mixing loop.
    ///
    /// Owns all tracks (packet receivers, jitter buffers, decoders) — they
    /// never leave this task.
    async fn run(
        mut command_rx: mpsc::Receiver<MixerCommand>,
        output_tx: mpsc::Sender<Vec<f32>>,
        gains: UserGains,
        level: OutputLevel,
        factory: DecoderFactory,
        stats: PlayoutStatsMap,
        mut stopped: tokio::sync::watch::Receiver<bool>,
    ) {
        let epoch = Instant::now();
        let mut tracks: HashMap<TrackInstance, Track> = HashMap::new();
        let mut ticker = tokio::time::interval(WAKE_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut frame = vec![0.0f32; FRAME_SAMPLES];
        let mut last_stats = Instant::now();
        let mut pacer = Pacer::default();
        let mut last_wake = Instant::now();

        'run: loop {
            // When no tracks are registered, block-wait for the next command
            // instead of spinning the timer.
            if tracks.is_empty() {
                tokio::select! {
                    cmd = command_rx.recv() => match cmd {
                        Some(cmd) => {
                            Self::apply_command(cmd, &mut tracks, &factory, &stats);
                            continue;
                        }
                        None => break, // every mixer handle dropped
                    },
                    _ = stopped.wait_for(|stop| *stop) => break,
                }
            }

            tokio::select! {
                _ = ticker.tick() => {}
                _ = stopped.wait_for(|stop| *stop) => break,
            }
            if output_tx.is_closed() || *stopped.borrow() {
                break; // playback stopped or the session ended
            }

            // 1. Pending commands (non-blocking).
            loop {
                match command_rx.try_recv() {
                    Ok(cmd) => Self::apply_command(cmd, &mut tracks, &factory, &stats),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    // Every handle is gone (and with them the RTP readers).
                    Err(mpsc::error::TryRecvError::Disconnected) => break 'run,
                }
            }

            // 2. Move arrived packets into the jitter buffers.
            let mut closed = Vec::new();
            for (id, track) in &mut tracks {
                for _ in 0..MAX_PACKETS_PER_WAKE {
                    match track.packets.try_recv() {
                        Ok(p) if p.arrival.elapsed() > MAX_PACKET_AGE => track.playout.expire(),
                        Ok(p) => {
                            let arrival_ms = ms_since(epoch, p.arrival);
                            track
                                .playout
                                .push(p.ssrc, p.seq, p.ts, p.marker, arrival_ms, p.payload);
                        }
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            closed.push(*id);
                            break;
                        }
                    }
                }
            }
            for instance in closed {
                debug!(instance, "Mixer: track producer disconnected, removing");
                Self::remove(&mut tracks, &stats, instance);
            }
            if tracks.is_empty() {
                continue;
            }

            // 3. One frame per track per output frame, evenly in time and matched to what the
            //    device consumes (see `Pacer`).
            let now = Instant::now();
            let elapsed_us = now.saturating_duration_since(last_wake).as_micros() as u64;
            last_wake = now;
            let now_ms = ms_since(epoch, now);
            let due = pacer.frames(
                elapsed_us,
                level.queued(),
                level.request_range(),
                level.callbacks(),
            );
            if pacer.take_went_absent() {
                // The device stopped taking audio: what is queued and
                // buffered will be stale when it comes back.
                level.request_flush();
                for track in tracks.values_mut() {
                    track.playout.flush();
                }
            }
            let quiet = pacer.silence();
            for n in 0..due {
                let mut mixed = vec![0.0f32; FRAME_SAMPLES];
                // Filling the output queue with silence does not tick the
                // tracks (see `Pacer`).
                let tracks = tracks.values_mut().filter(|_| n >= quiet);
                for track in tracks {
                    if track.playout.next_frame(now_ms, &mut frame) {
                        let gain = track.user_id.map_or(1.0, |user| gains.get(user));
                        for (out, sample) in mixed.iter_mut().zip(frame.iter()) {
                            *out += sample * gain;
                        }
                    }
                }
                // Clip to prevent distortion.
                for sample in &mut mixed {
                    *sample = sample.clamp(-1.0, 1.0);
                }
                level.produced(FRAME_SAMPLES);
                if output_tx.send(mixed).await.is_err() {
                    break 'run; // playback dropped
                }
            }

            // 4. Counters for the stats command and the debug log.
            if last_stats.elapsed() >= STATS_INTERVAL {
                last_stats = Instant::now();
                level.roll_window();
                Self::publish_stats(&mut tracks, &stats);
            }
        }

        if let Ok(mut map) = stats.lock() {
            map.clear();
        }
        info!("Mixer: run loop exited");
    }

    fn publish_stats(tracks: &mut HashMap<TrackInstance, Track>, stats: &PlayoutStatsMap) {
        let Ok(mut map) = stats.lock() else {
            return;
        };
        for (instance, track) in tracks.iter_mut() {
            let snapshot = track.playout.stats();
            let id = &track.track_id;
            map.insert(*instance, (id.clone(), snapshot));
            if track.last_log.elapsed() >= LOG_INTERVAL {
                track.last_log = Instant::now();
                let b = snapshot.buffer;
                debug!(
                    track_id = %id,
                    received = b.packets_received,
                    reordered = b.reordered,
                    duplicates = b.duplicates,
                    late_dropped = b.late_dropped,
                    discarded = b.discarded,
                    rejected = snapshot.rejected,
                    expired = snapshot.expired,
                    lost = b.lost,
                    concealed_plc = snapshot.concealed_plc,
                    recovered_fec = snapshot.recovered_fec,
                    underruns = b.underruns,
                    drift_drops = b.drift_drops,
                    drift_inserts = b.drift_inserts,
                    resets = b.resets,
                    target_ms = snapshot.target_delay_ms,
                    level_ms = snapshot.level_ms,
                    "Mixer: track playout"
                );
            }
        }
    }

    fn remove(
        tracks: &mut HashMap<TrackInstance, Track>,
        stats: &PlayoutStatsMap,
        instance: TrackInstance,
    ) {
        tracks.remove(&instance);
        if let Ok(mut map) = stats.lock() {
            map.remove(&instance);
        }
    }

    /// Apply a single command to the track map.
    fn apply_command(
        cmd: MixerCommand,
        tracks: &mut HashMap<TrackInstance, Track>,
        factory: &DecoderFactory,
        stats: &PlayoutStatsMap,
    ) {
        match cmd {
            MixerCommand::AddTrack {
                instance,
                track_id,
                packets,
            } => {
                let Some(decoder) = factory() else {
                    // Dropping `packets` closes the reader's queue: it stops.
                    return;
                };
                debug!(track_id = %track_id, instance, "Mixer: added track");
                // A replacement (participant rejoined) retires its
                // predecessor now: the old instance stops feeding the mix.
                let retired: Vec<TrackInstance> = tracks
                    .iter()
                    .filter(|(_, t)| t.track_id == track_id)
                    .map(|(i, _)| *i)
                    .collect();
                for old in retired {
                    Self::remove(tracks, stats, old);
                }
                let user_id = track_id
                    .split(':')
                    .next()
                    .and_then(|user| Uuid::parse_str(user).ok());
                tracks.insert(
                    instance,
                    Track {
                        track_id,
                        packets,
                        playout: TrackPlayout::new(decoder),
                        user_id,
                        last_log: Instant::now(),
                    },
                );
            }
            MixerCommand::RemoveTrack { instance } => {
                debug!(instance, "Mixer: removed track");
                Self::remove(tracks, stats, instance);
            }
        }
    }
}

/// Milliseconds from `epoch` to `at` (0 for instants before it).
fn ms_since(epoch: Instant, at: Instant) -> u64 {
    at.saturating_duration_since(epoch).as_millis() as u64
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uuid::Uuid;

    use super::*;

    /// Mock decoder: packet `[v]` decodes to a frame of `v / scale` and
    /// counts its decodes.
    struct ConstDecoder {
        scale: f32,
        decodes: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl FrameDecoder for ConstDecoder {
        fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
            self.decodes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            out[..FRAME_SAMPLES].fill(f32::from(packet[0]) / self.scale);
            Ok(FRAME_SAMPLES / 2)
        }
        fn decode_fec(&mut self, _next: &[u8], out: &mut [f32]) -> Result<usize, String> {
            out.fill(0.0);
            Ok(out.len() / 2)
        }
        fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
            out.fill(0.0);
            Ok(out.len() / 2)
        }
        fn can_recover(&self, _next: &[u8], _lost: usize) -> bool {
            false
        }
        fn duration(&self, _packet: &[u8]) -> Option<u32> {
            Some(960)
        }
        fn reset(&mut self) {}
    }

    fn const_factory() -> DecoderFactory {
        scaled_factory(1000.0, Arc::default())
    }

    fn scaled_factory(scale: f32, decodes: Arc<std::sync::atomic::AtomicUsize>) -> DecoderFactory {
        Arc::new(move || {
            Some(Box::new(ConstDecoder {
                scale,
                decodes: decodes.clone(),
            }) as Box<dyn FrameDecoder + Send>)
        })
    }

    fn packet(seq: u16, value: u8) -> RtpIn {
        RtpIn {
            ssrc: 1,
            seq,
            ts: u32::from(seq) * 960,
            marker: false,
            arrival: Instant::now(),
            payload: vec![value],
        }
    }

    /// A fake output device: consumes one frame every 20 ms and collects
    /// the first sample of every frame it is given.
    fn fake_device(
        mut out_rx: mpsc::Receiver<Vec<f32>>,
        level: OutputLevel,
    ) -> tokio::task::JoinHandle<Vec<f32>> {
        tokio::spawn(async move {
            let mut seen = Vec::new();
            let stream = level.begin_stream();
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            let deadline = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < deadline {
                tick.tick().await;
                level.note_request(stream, FRAME_SAMPLES);
                while let Ok(frame) = out_rx.try_recv() {
                    assert_eq!(frame.len(), FRAME_SAMPLES);
                    seen.push(frame[0]);
                }
                level.consumed(FRAME_SAMPLES);
                level.callback_done(stream);
            }
            seen
        })
    }

    /// Mix a short stream of `value / 250` from one user at `gain`.
    /// Returns the mixed samples seen and how often the track decoded.
    async fn mix_user(gain: Option<f32>, value: u8) -> (Vec<f32>, usize) {
        let (out_tx, out_rx) = mpsc::channel(8);
        let level = OutputLevel::new();
        let decodes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mixer = AudioMixer::with_decoder(
            out_tx,
            UserGains::default(),
            level.clone(),
            scaled_factory(250.0, decodes.clone()),
        );
        let user = Uuid::new_v4();
        if let Some(gain) = gain {
            mixer.set_user_gain(user, gain);
        }
        let device = fake_device(out_rx, level);
        let tx = mixer.add_track(format!("{user}:microphone")).await.packets;
        for seq in 0..10u16 {
            tx.send(packet(seq, value)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let seen = device.await.unwrap();
        (seen, decodes.load(std::sync::atomic::Ordering::Relaxed))
    }

    #[tokio::test]
    async fn mixer_applies_user_gain() {
        let (unity, _) = mix_user(None, 200).await;
        assert!(unity.iter().any(|s| (s - 0.8).abs() < 1e-6), "{unity:?}");

        let (half, _) = mix_user(Some(0.5), 200).await;
        assert!(half.iter().any(|s| (s - 0.4).abs() < 1e-6), "{half:?}");
        assert!(half.iter().all(|s| *s < 0.4 + 1e-6));

        // Boost is allowed but the mix is clipped to [-1, 1]: 1.0 × 2.
        let (boosted, _) = mix_user(Some(2.0), 250).await;
        assert!(
            boosted.iter().any(|s| (s - 1.0).abs() < 1e-6),
            "{boosted:?}"
        );
        assert!(boosted.iter().all(|s| *s <= 1.0), "{boosted:?}");
    }

    #[tokio::test]
    async fn zero_gain_is_silent() {
        let (muted, decodes) = mix_user(Some(0.0), 250).await;
        assert_ne!(muted.len(), 0);
        // The track did play (decoded), it was only silenced.
        assert!(decodes >= 5, "{decodes} decodes");
        assert!(muted.iter().all(|s| *s == 0.0));
    }

    #[tokio::test]
    async fn gain_is_per_user_and_survives_a_new_mixer() {
        let gains = UserGains::default();
        let quiet = Uuid::new_v4();
        let loud = Uuid::new_v4();
        gains.set(quiet, 0.0);
        let (out_tx, out_rx) = mpsc::channel(8);
        let level = OutputLevel::new();
        let mixer = AudioMixer::with_decoder(out_tx, gains.clone(), level.clone(), const_factory());
        let device = fake_device(out_rx, level);
        let quiet_tx = mixer.add_track(format!("{quiet}:microphone")).await.packets;
        let loud_tx = mixer.add_track(format!("{loud}:microphone")).await.packets;
        for seq in 0..10u16 {
            quiet_tx.send(packet(seq, 250)).await.unwrap();
            loud_tx.send(packet(seq, 100)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let seen = device.await.unwrap();
        // The muted user never reaches the mix.
        assert!(seen.iter().any(|s| (s - 0.1).abs() < 1e-6), "{seen:?}");
        assert!(seen
            .iter()
            .all(|s| s.abs() < 1e-6 || (s - 0.1).abs() < 1e-6));
        assert!(gains.get(quiet).abs() < f32::EPSILON);
    }

    /// The old mixer kept only the newest frame per 20 ms tick, so packets
    /// arriving two at a time lost every second one. Now every packet of
    /// every track is mixed exactly once, in order.
    #[tokio::test]
    async fn mixer_mixes_every_packet_of_every_track() {
        let (out_tx, out_rx) = mpsc::channel(8);
        let level = OutputLevel::new();
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
        let device = fake_device(out_rx, level);
        let a = mixer
            .add_track(format!("{}:microphone", Uuid::new_v4()))
            .await;
        let b = mixer
            .add_track(format!("{}:microphone", Uuid::new_v4()))
            .await;
        // 40 packets per track, delivered in pairs every 40 ms.
        for pair in 0..20u16 {
            for seq in [pair * 2, pair * 2 + 1] {
                a.packets.send(packet(seq, seq as u8)).await.unwrap();
                b.packets.send(packet(seq, 100 + seq as u8)).await.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        let seen = device.await.unwrap();
        // Frame for packet i of both tracks = (i + 100 + i) / 1000.
        let indices: Vec<u16> = seen
            .iter()
            .filter(|s| **s > 0.0)
            .map(|s| ((s * 1000.0 - 100.0) / 2.0).round() as u16)
            .collect();
        assert!(indices.windows(2).all(|w| w[1] > w[0]), "{indices:?}");
        // All 40 (bar the tail still buffered when the device stopped).
        assert!(indices.len() >= 36, "{indices:?}");
        assert!(indices.windows(2).filter(|w| w[1] != w[0] + 1).count() <= 1);
    }

    #[tokio::test]
    async fn mixer_produces_only_what_the_device_needs() {
        let (out_tx, mut out_rx) = mpsc::channel(64);
        let level = OutputLevel::new();
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
        let tx = mixer
            .add_track(format!("{}:microphone", Uuid::new_v4()))
            .await
            .packets;
        for seq in 0..5u16 {
            tx.send(packet(seq, 1)).await.unwrap();
        }
        // No device consumes anything: after a second without a callback
        // the mixer stops rendering.
        tokio::time::sleep(Duration::from_millis(1_300)).await;
        let mut produced = 0;
        while out_rx.try_recv().is_ok() {
            produced += 1;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let mut later = 0;
        while out_rx.try_recv().is_ok() {
            later += 1;
        }
        assert!(produced <= 55, "{produced}");
        assert_eq!(later, 0, "kept rendering without a device");
        assert_eq!(level.queued(), produced * FRAME_SAMPLES);
        // A device appears and plays it all: the mixer follows.
        let stream = level.begin_stream();
        level.consumed(produced * FRAME_SAMPLES);
        level.callback_done(stream);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(out_rx.try_recv().is_ok());
    }

    #[tokio::test]
    async fn closed_tracks_are_removed_and_mixer_stops_with_its_handles() {
        let (out_tx, mut out_rx) = mpsc::channel(64);
        let level = OutputLevel::new();
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
        let tx = mixer.add_track("t:microphone".into()).await.packets;
        tx.send(packet(0, 1)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop(tx);
        drop(mixer);
        // The task ends: the output channel closes.
        let closed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                level.consumed(FRAME_SAMPLES * 4);
                if out_rx.recv().await.is_none() {
                    break;
                }
            }
        })
        .await;
        assert!(closed.is_ok(), "mixer task still running");
    }

    /// Simulated output device taking `block_ms` per callback, the pusher's
    /// queue limit and the mixer's pacing, for 20 s (1 ms steps). Returns
    /// frames produced, samples discarded by the queue and samples the
    /// device found missing, all in the last 10 s.
    fn device_run(block_ms: usize) -> (usize, usize, usize) {
        let (produced, discarded, missing, _) = device_run_consumed(block_ms);
        (produced, discarded, missing)
    }

    fn device_run_consumed(block_ms: usize) -> (usize, usize, usize, usize) {
        let level = OutputLevel::new();
        let stream = level.begin_stream();
        let mut pacer = Pacer::default();
        let block = block_ms * 96; // stereo samples at 48 kHz
        let mut queue = 0usize;
        let (mut produced, mut discarded, mut missing, mut consumed) = (0, 0, 0, 0);
        for t in 0..20_000usize {
            let steady = t >= 10_000;
            if t % 5 == 0 {
                let n = pacer.frames(
                    5_000,
                    level.queued(),
                    level.request_range(),
                    level.callbacks(),
                );
                for _ in 0..n {
                    level.produced(FRAME_SAMPLES);
                    queue += FRAME_SAMPLES;
                    let excess = queue.saturating_sub(crate::audio::MAX_QUEUE_SAMPLES);
                    queue -= excess;
                    level.consumed(excess);
                    if steady {
                        produced += 1;
                        discarded += excess;
                    }
                }
            }
            if t % block_ms == 0 {
                level.note_request(stream, block);
                let take = queue.min(block);
                queue -= take;
                level.consumed(take);
                level.callback_done(stream);
                if steady {
                    missing += block - take;
                    consumed += take;
                }
            }
            if t % 1000 == 999 {
                level.roll_window();
            }
        }
        (produced, discarded, missing, consumed)
    }

    #[test]
    fn device_blocks_from_10_to_400_ms_are_fed_in_real_time() {
        for block_ms in [10, 20, 21, 40, 85, 100, 160, 180, 200, 250, 341, 400] {
            let (produced, discarded, missing) = device_run(block_ms);
            // 10 s at 50 frames per second, give or take the device blocks
            // that straddle the edges of the window.
            let slack = block_ms / 20 + 2;
            assert!(
                produced.abs_diff(500) <= slack,
                "{block_ms} ms blocks: {produced} frames in 10 s"
            );
            assert_eq!(discarded, 0, "{block_ms} ms blocks: queue discarded audio");
            assert_eq!(missing, 0, "{block_ms} ms blocks: device starved");
        }
    }

    #[test]
    fn blocks_beyond_the_supported_size_stay_real_time() {
        // Larger than the block estimate's cap: the pacer follows the blocks
        // it observes, the queue holds them, the mixer never runs ahead and
        // nothing is discarded.
        let (produced, discarded, _missing, consumed) = device_run_consumed(500);
        assert!(
            produced.abs_diff(consumed / FRAME_SAMPLES) <= 30,
            "{produced}"
        );
        assert!(produced <= 505, "{produced}");
        assert_eq!(discarded, 0);
    }

    #[test]
    fn block_estimate_follows_the_device() {
        let level = OutputLevel::new();
        let stream = level.begin_stream();
        level.note_request(stream, 400 * 96); // one priming callback
        level.roll_window();
        for _ in 0..3 {
            level.note_request(stream, 10 * 96);
            level.roll_window();
        }
        assert_eq!(level.request(), 10 * 96);
        level.begin_stream(); // another device
        assert_eq!(level.request(), 0);
    }

    #[tokio::test]
    async fn mixer_stops_when_playback_closes_while_the_device_is_idle() {
        let (out_tx, out_rx) = mpsc::channel(64);
        let level = OutputLevel::new();
        // The device holds plenty: the mixer has nothing to send.
        level.produced(10 * FRAME_SAMPLES);
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
        let tx = mixer.add_track("t:microphone".into()).await.packets;
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(out_rx); // playback stopped
        let stopped = tokio::time::timeout(Duration::from_secs(1), async {
            while !tx.is_closed() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        assert!(stopped.is_ok(), "mixer kept running without playback");
        drop(mixer);
    }

    /// A participant rejoins: the new track arrives while the old one is
    /// still unwinding, and the old one's late removal must not touch it.
    #[tokio::test]
    async fn late_removal_of_a_replaced_track_keeps_the_new_one() {
        let (out_tx, out_rx) = mpsc::channel(8);
        let level = OutputLevel::new();
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
        let device = fake_device(out_rx, level);
        let id = format!("{}:microphone", Uuid::new_v4());
        let old = mixer.add_track(id.clone()).await;
        let new = mixer.add_track(id.clone()).await.packets;
        mixer.remove_track(old.instance).await; // the old instance unwinds
        for seq in 0..20u16 {
            new.send(packet(seq, 200))
                .await
                .expect("replacement still live");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let seen = device.await.unwrap();
        assert!(seen.iter().any(|s| (s - 0.2).abs() < 1e-6), "{seen:?}");
        drop(old);
    }

    /// Outcome of five simulated minutes of a remote stream played through
    /// one track behind a device taking `block_ms` per callback.
    #[derive(Debug)]
    struct PlayRun {
        /// Counters accumulated after the warm-up.
        stats: PlayoutStats,
        conceals: usize,
        device_missing: usize,
        target_ms: f64,
    }

    /// Packets arrive every `period_us` (20 000 = exact) with no network
    /// jitter; the mixer wakes every 5 ms and asks `pace` how many frames
    /// to render; the device takes `block_ms` per callback. Concealment and
    /// device gaps are counted after a 5 s warm-up.
    fn play_run(
        block_ms: u64,
        period_us: u64,
        mut pace: impl FnMut(u64, &OutputLevel) -> (usize, usize),
    ) -> PlayRun {
        let conceals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let decoder = CountingDecoder {
            conceals: conceals.clone(),
        };
        let mut playout = TrackPlayout::new(Box::new(decoder) as Box<dyn FrameDecoder + Send>);
        let level = OutputLevel::new();
        let generation = level.begin_stream();
        let block = block_ms as usize * 96;
        let mut queue = 0usize;
        let mut frame = vec![0.0f32; FRAME_SAMPLES];
        let (mut seq, mut device_missing) = (0u32, 0usize);
        let mut warm_conceals = 0;
        let mut warm = PlayoutStats::default();
        for t_ms in 0..300_000u64 {
            let t_us = t_ms * 1000;
            while u64::from(seq) * period_us <= t_us {
                playout.push(
                    1,
                    seq as u16,
                    seq.wrapping_mul(960),
                    seq == 0,
                    u64::from(seq) * period_us / 1000,
                    vec![1],
                );
                seq += 1;
            }
            if t_ms == 5_000 {
                warm_conceals = conceals.load(std::sync::atomic::Ordering::Relaxed);
                warm = playout.stats();
            }
            if t_ms % 5 == 0 {
                let (n, quiet) = pace(t_ms, &level);
                for i in 0..n {
                    if i >= quiet {
                        playout.next_frame(t_ms, &mut frame);
                    }
                    level.produced(FRAME_SAMPLES);
                    queue += FRAME_SAMPLES;
                }
            }
            if t_ms % block_ms == 0 && t_ms > 0 {
                level.note_request(generation, block);
                let take = queue.min(block);
                queue -= take;
                level.consumed(take);
                level.callback_done(generation);
                if t_ms > 5_000 {
                    device_missing += block - take;
                }
            }
            if t_ms % 1000 == 999 {
                level.roll_window();
            }
        }
        let mut stats = playout.stats();
        let (b, w) = (&mut stats.buffer, warm.buffer);
        b.underruns -= w.underruns;
        b.drift_drops -= w.drift_drops;
        b.drift_inserts -= w.drift_inserts;
        b.lost -= w.lost;
        PlayRun {
            stats,
            conceals: conceals.load(std::sync::atomic::Ordering::Relaxed) - warm_conceals,
            device_missing,
            target_ms: stats.target_delay_ms,
        }
    }

    /// Decoder that counts concealment calls.
    struct CountingDecoder {
        conceals: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl FrameDecoder for CountingDecoder {
        fn decode(&mut self, _packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
            out[..FRAME_SAMPLES].fill(0.5);
            Ok(FRAME_SAMPLES / 2)
        }
        fn decode_fec(&mut self, _next: &[u8], out: &mut [f32]) -> Result<usize, String> {
            self.conceals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            out.fill(0.0);
            Ok(out.len() / 2)
        }
        fn conceal(&mut self, out: &mut [f32]) -> Result<usize, String> {
            self.conceals
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            out.fill(0.0);
            Ok(out.len() / 2)
        }
        fn can_recover(&self, _next: &[u8], _lost: usize) -> bool {
            false
        }
        fn duration(&self, _packet: &[u8]) -> Option<u32> {
            Some(960)
        }
        fn reset(&mut self) {}
    }

    const SWEEP_BLOCKS: [u64; 8] = [10, 20, 50, 100, 160, 200, 300, 400];

    /// With a clean network, the device's block size must not matter to
    /// the jitter buffer: no underruns, no concealment, no drift
    /// adjustments, target at its floor.
    #[test]
    fn clean_stream_plays_untouched_behind_any_device_block() {
        for block_ms in SWEEP_BLOCKS {
            let mut pacer = Pacer::default();
            let run = play_run(block_ms, 20_000, |_, level| {
                let n = pacer.frames(
                    5_000,
                    level.queued(),
                    level.request_range(),
                    level.callbacks(),
                );
                (n, pacer.silence())
            });
            let b = run.stats.buffer;
            assert_eq!(
                (b.underruns, run.conceals, b.drift_drops, b.drift_inserts),
                (0, 0, 0, 0),
                "{block_ms} ms blocks: {run:?}"
            );
            assert_eq!(run.device_missing, 0, "{block_ms} ms blocks: {run:?}");
            assert!(
                (run.target_ms - 40.0).abs() < 1e-9,
                "{block_ms} ms: {run:?}"
            );
        }
    }

    /// The same with the sender's clock 0.1 % off: only the expected
    /// adjustments (4 min 55 s after warm-up × 0.1 % ≈ 15 frames).
    #[test]
    fn drifting_stream_gets_only_the_expected_adjustments_behind_any_block() {
        for (period_us, slow) in [(20_020u64, true), (19_980, false)] {
            for block_ms in SWEEP_BLOCKS {
                let mut pacer = Pacer::default();
                let run = play_run(block_ms, period_us, |_, level| {
                    let n = pacer.frames(
                        5_000,
                        level.queued(),
                        level.request_range(),
                        level.callbacks(),
                    );
                    (n, pacer.silence())
                });
                let b = run.stats.buffer;
                let (wanted, other) = if slow {
                    (b.drift_inserts, b.drift_drops)
                } else {
                    (b.drift_drops, b.drift_inserts)
                };
                assert!(
                    (12..=18).contains(&wanted),
                    "{block_ms} ms, {period_us}: {run:?}"
                );
                assert_eq!(other, 0, "{block_ms} ms, {period_us}: {run:?}");
                assert_eq!(b.underruns, 0, "{block_ms} ms, {period_us}: {run:?}");
                assert_eq!(run.device_missing, 0, "{block_ms} ms, {period_us}: {run:?}");
            }
        }
    }

    /// A replacement retires its predecessor when it is added: the old
    /// instance can no longer feed audio into the mix, and only the current
    /// instance may report the track as gone.
    #[tokio::test]
    async fn a_replacement_retires_its_predecessor_at_once() {
        let (out_tx, _out_rx) = mpsc::channel(8);
        let mixer = AudioMixer::with_decoder(
            out_tx,
            UserGains::default(),
            OutputLevel::new(),
            const_factory(),
        );
        let id = format!("{}:microphone", Uuid::new_v4());
        let old = mixer.add_track(id.clone()).await;
        let new = mixer.add_track(id.clone()).await;
        let retired = tokio::time::timeout(Duration::from_secs(1), async {
            while !old.packets.is_closed() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(retired.is_ok(), "the old instance still feeds the mix");
        assert!(!new.packets.is_closed());
        assert!(
            !mixer.release(&id, old.instance),
            "old instance released the track"
        );
        assert!(mixer.release(&id, new.instance));
    }

    /// A leave that times out must still end the mixer, its PCM output and
    /// the readers, even while a silent track's reader still holds a handle.
    #[tokio::test]
    async fn shutdown_ends_mixer_output_and_readers_whatever_holds_a_handle() {
        let (out_tx, mut out_rx) = mpsc::channel(8);
        let mixer = AudioMixer::with_decoder(
            out_tx,
            UserGains::default(),
            OutputLevel::new(),
            const_factory(),
        );
        let handle = mixer.add_track("silent:microphone".into()).await;
        // A reader on a track that never delivers a packet, holding a mixer
        // handle the way the real reader task does.
        let reader = {
            let mixer = mixer.clone();
            tokio::spawn(async move {
                let _keep = mixer;
                tokio::select! {
                    () = std::future::pending::<()>() => {}
                    () = handle.packets.closed() => {}
                }
            })
        };
        tokio::time::sleep(Duration::from_millis(30)).await;
        mixer.shutdown();
        let ended = tokio::time::timeout(Duration::from_secs(1), async {
            while out_rx.recv().await.is_some() {}
            reader.await.unwrap();
        })
        .await;
        assert!(ended.is_ok(), "mixer, output or reader still alive");
    }

    /// After a mixer stall, packets that waited longer than the receive
    /// budget are dropped and counted instead of being played late.
    #[tokio::test]
    async fn packets_older_than_the_receive_budget_are_not_played() {
        let (out_tx, out_rx) = mpsc::channel(8);
        let level = OutputLevel::new();
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
        let device = fake_device(out_rx, level);
        let tx = mixer
            .add_track(format!("{}:microphone", Uuid::new_v4()))
            .await
            .packets;
        let stale = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        for seq in 0..8u16 {
            let mut p = packet(seq, 10); // value 0.01: stale audio
            p.arrival = stale + Duration::from_millis(u64::from(seq) * 20);
            tx.send(p).await.unwrap();
        }
        for seq in 8..40u16 {
            tx.send(packet(seq, 200)).await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Counters are published once a second while the mixer runs.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let stats = mixer.playout_stats();
        let seen = device.await.unwrap();
        assert!(seen.iter().any(|s| (s - 0.2).abs() < 1e-6), "{seen:?}");
        assert!(
            !seen.iter().any(|s| (s - 0.01).abs() < 1e-6),
            "stale audio played"
        );
        assert_eq!(stats[0].1.expired, 8, "{stats:?}");
    }

    /// A device asking for 20, 20, 20, then `big` ms, each callback after
    /// the previous block's duration, at every phase of the 5 ms wake:
    /// after warm-up it never finds the queue short.
    #[test]
    fn variable_device_blocks_never_find_the_queue_short() {
        for big in [60u64, 100, 200, 400] {
            for phase in 0..5u64 {
                let level = OutputLevel::new();
                let stream = level.begin_stream();
                let mut pacer = Pacer::default();
                let mut queue = 0usize;
                let (mut pos, mut block_ms, mut next_cb, mut short) =
                    (0usize, 20u64, 20u64, 0usize);
                for t in 0..60_000u64 {
                    if t % 5 == phase {
                        let n = pacer.frames(
                            5_000,
                            level.queued(),
                            level.request_range(),
                            level.callbacks(),
                        );
                        level.produced(n * FRAME_SAMPLES);
                        queue += n * FRAME_SAMPLES;
                    }
                    if t == next_cb {
                        let block = block_ms as usize * 96;
                        level.note_request(stream, block);
                        let take = queue.min(block);
                        queue -= take;
                        level.consumed(take);
                        level.callback_done(stream);
                        if t > 5_000 {
                            short += block - take;
                        }
                        next_cb = t + block_ms;
                        pos = (pos + 1) % 4;
                        block_ms = if pos == 3 { big } else { 20 };
                    }
                    if t % 1000 == 999 {
                        level.roll_window();
                    }
                }
                assert_eq!(short, 0, "20/20/20/{big} ms at wake phase {phase}");
            }
        }
    }

    #[test]
    fn pacer_renders_evenly_and_meets_the_device_at_its_low_point() {
        let f = FRAME_SAMPLES;
        let b40 = Some((40 * 96, 40 * 96));
        let mut pacer = Pacer::default();
        // No callback yet: one frame per 20 ms, evenly.
        assert_eq!(pacer.frames(20_000, 0, None, 0), 1);
        assert_eq!(pacer.frames(10_000, f, None, 0), 0);
        assert_eq!(pacer.frames(10_000, f, None, 0), 1);
        // A 40 ms device took its block and left the queue empty: one frame
        // to the headroom, plus the even pace.
        assert_eq!(pacer.frames(20_000, 0, b40, 1), 2);
        // Even pace between callbacks.
        assert_eq!(pacer.frames(20_000, 2 * f, b40, 1), 1);
        // Never more than three per wake, even after a long stall.
        assert_eq!(pacer.frames(1_000_000, 0, b40, 2), 3);
        // No callback for a second: absent, once; three frames ahead at most.
        assert_eq!(pacer.frames(1_100_000, 3 * f, b40, 2), 0);
        assert!(pacer.take_went_absent());
        assert!(!pacer.take_went_absent());
        assert_eq!(pacer.frames(20_000, 2 * f, b40, 2), 1);
        assert_eq!(pacer.frames(20_000, 3 * f, b40, 2), 0);
    }

    /// Blocks of different sizes raise the low point by their spread.
    #[test]
    fn pacer_holds_the_spread_of_block_sizes_in_reserve() {
        let f = FRAME_SAMPLES;
        let mut pacer = Pacer::default();
        // A device mixing 20 and 100 ms blocks; the queue's low point is one
        // frame: the pacer tops it up to one frame + 80 ms at once.
        let due = pacer.frames(5_000, f, Some((20 * 96, 100 * 96)), 1);
        assert_eq!(due, 3, "capped at three per wake");
        let due = pacer.frames(5_000, 4 * f, Some((20 * 96, 100 * 96)), 2);
        assert_eq!(due, 1);
    }

    /// With a queue a few frames too full, the pacer drains it by one frame
    /// per second, never faster, and stops at its target.
    #[test]
    fn pacer_skips_at_most_once_a_second_and_only_when_every_low_point_is_high() {
        let f = FRAME_SAMPLES;
        let mut pacer = Pacer::default();
        let mut queue = 4 * f; // three frames above the target
        let mut callbacks = 0u64;
        let (mut rendered, mut skips) = (0u64, Vec::new());
        for t in (0..10_000u64).step_by(5) {
            if t % 20 == 0 {
                callbacks += 1;
                queue -= f.min(queue); // a 20 ms device
            }
            let due = pacer.frames(5_000, queue, Some((20 * 96, 20 * 96)), callbacks);
            queue += due * f;
            rendered += due as u64;
            let expected = (t + 5) / 20;
            if rendered + skips.len() as u64 + 1 == expected {
                skips.push(t);
            }
        }
        assert_eq!(queue, 2 * f, "settled one frame above the device's take");
        assert!((2..=4).contains(&skips.len()), "{skips:?}");
        assert!(skips.windows(2).all(|w| w[1] - w[0] >= 1_000), "{skips:?}");
    }
}
