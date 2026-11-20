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
/// drains them every 5 ms; a full queue means the mixer stalled.
pub const PACKET_QUEUE: usize = 64;

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
    fn has_fec(&self, packet: &[u8]) -> bool {
        (**self).has_fec(packet)
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
/// block size; the output queue absorbs the device's blocks. The pace is
/// matched to the device at one fixed phase of its cycle: right after a
/// callback, the queue is at its low point, which is held at one frame
/// (one extra frame when below, one skipped when a frame above). Device
/// clock drift thus becomes an occasional extra or skipped tick, which the
/// jitter buffer absorbs like sender drift. Rendering never runs more than
/// one device block plus three frames ahead of the device. Before the
/// first callback that block is not known yet, so for up to a second the
/// largest supported block is assumed (the first callback of a 400 ms
/// device then finds its block); when no callback comes for a second
/// (device stalled or gone), rendering stops three frames ahead.
#[derive(Debug, Default)]
pub struct Pacer {
    credit_us: u64,
    seen_callbacks: u64,
    since_callback_us: u64,
}

/// Without a device callback for this long, the device counts as absent.
const DEVICE_ABSENT_US: u64 = 1_000_000;

impl Pacer {
    /// Frames to render now, `elapsed_us` after the previous wake, with
    /// `queued` samples waiting for the device, its recent block size
    /// `request` and its completed `callbacks`.
    pub fn frames(
        &mut self,
        elapsed_us: u64,
        queued: usize,
        request: usize,
        callbacks: u64,
    ) -> usize {
        let cap = MAX_FRAMES_PER_WAKE as u64 * FRAME_US;
        self.credit_us = (self.credit_us + elapsed_us).min(cap);
        let mut due = (self.credit_us / FRAME_US) as usize;
        self.credit_us %= FRAME_US;
        self.since_callback_us = self.since_callback_us.saturating_add(elapsed_us);
        if callbacks != self.seen_callbacks {
            self.seen_callbacks = callbacks;
            self.since_callback_us = 0;
            // The device has just taken its block: the queue's low point.
            if queued < OUTPUT_HEADROOM_SAMPLES {
                due += (OUTPUT_HEADROOM_SAMPLES - queued).div_ceil(FRAME_SAMPLES);
            } else if queued >= OUTPUT_HEADROOM_SAMPLES + FRAME_SAMPLES {
                due = due.saturating_sub(1);
            }
        }
        let block = if self.since_callback_us >= DEVICE_ABSENT_US {
            0
        } else if callbacks == 0 {
            crate::audio::MAX_REQUEST_SAMPLES
        } else {
            request
        };
        let bound = block + 3 * FRAME_SAMPLES;
        while due > 0 && queued + due * FRAME_SAMPLES > bound {
            due -= 1;
        }
        due.min(MAX_FRAMES_PER_WAKE)
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
        tokio::spawn(Self::run(
            command_rx,
            output_tx,
            gains.clone(),
            level,
            factory,
            stats.clone(),
        ));
        Self {
            command_tx,
            gains,
            stats,
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
                match command_rx.recv().await {
                    Some(cmd) => {
                        Self::apply_command(cmd, &mut tracks, &factory, &stats);
                        continue;
                    }
                    None => break, // every mixer handle dropped
                }
            }

            ticker.tick().await;
            if output_tx.is_closed() {
                break; // playback stopped
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
                level.request(),
                level.callbacks(),
            );
            for _ in 0..due {
                let mut mixed = vec![0.0f32; FRAME_SAMPLES];
                for track in tracks.values_mut() {
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
        fn has_fec(&self, _packet: &[u8]) -> bool {
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
        assert!(!muted.is_empty());
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
                let n = pacer.frames(5_000, level.queued(), level.request(), level.callbacks());
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
        // Larger than supported: each 500 ms callback finds 420 ms queued and
        // plays a short gap. The mixer follows what is consumed and never
        // runs ahead; nothing is discarded.
        let (produced, discarded, missing, consumed) = device_run_consumed(500);
        assert!(
            produced.abs_diff(consumed / FRAME_SAMPLES) <= 30,
            "{produced}"
        );
        assert!(produced <= 505, "{produced}");
        assert!(missing > 0);
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
        mut pace: impl FnMut(u64, &OutputLevel) -> usize,
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
                for _ in 0..pace(t_ms, &level) {
                    playout.next_frame(t_ms, &mut frame);
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
        fn has_fec(&self, _packet: &[u8]) -> bool {
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
                pacer.frames(5_000, level.queued(), level.request(), level.callbacks())
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
                    pacer.frames(5_000, level.queued(), level.request(), level.callbacks())
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

    #[test]
    fn pacer_renders_evenly_and_meets_the_device_at_its_low_point() {
        let mut pacer = Pacer::default();
        // No callback yet: one frame per 20 ms, evenly.
        assert_eq!(pacer.frames(20_000, 0, 0, 0), 1);
        assert_eq!(pacer.frames(10_000, FRAME_SAMPLES, 0, 0), 0);
        assert_eq!(pacer.frames(10_000, FRAME_SAMPLES, 0, 0), 1);
        // Still none after a second: the device counts as absent, at most
        // three frames ahead.
        assert_eq!(pacer.frames(1_000_000, 3 * FRAME_SAMPLES, 0, 0), 0);
        assert_eq!(pacer.frames(20_000, 2 * FRAME_SAMPLES, 0, 0), 1);
        assert_eq!(pacer.frames(20_000, 3 * FRAME_SAMPLES, 0, 0), 0);
        // A 100 ms device took its first block and left the queue empty:
        // one frame to reach the headroom, plus the even pace.
        let block = 100 * 96;
        assert_eq!(pacer.frames(20_000, 0, block, 1), 2);
        // Even pace between callbacks, however large the block.
        assert_eq!(pacer.frames(20_000, 2 * FRAME_SAMPLES, block, 1), 1);
        // At the next low point the queue is a frame too full (device
        // clock slower): one frame is skipped.
        assert_eq!(pacer.frames(20_000, 2 * FRAME_SAMPLES, block, 2), 0);
        // Never more than three per wake, even after a long stall.
        assert_eq!(pacer.frames(1_000_000, 0, block, 3), 3);
    }
}
