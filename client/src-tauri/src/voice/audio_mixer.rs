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

/// How many 20 ms frames to produce now, given the samples already queued
/// for the device and the largest block it asks for at once.
pub fn frames_needed(queued: usize, request: usize) -> usize {
    let target = request + OUTPUT_HEADROOM_SAMPLES;
    target
        .saturating_sub(queued)
        .div_ceil(FRAME_SAMPLES)
        .min(MAX_FRAMES_PER_WAKE)
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
                                .push(p.ssrc, p.seq, p.ts, arrival_ms, p.payload);
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

            // 3. One frame per track per output frame, as many as the device needs right now.
            let now_ms = ms_since(epoch, Instant::now());
            for _ in 0..frames_needed(level.queued(), level.request()) {
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

    /// Mock decoder: packet `[v]` decodes to a frame of `v / 1000`.
    struct ConstDecoder;

    impl FrameDecoder for ConstDecoder {
        fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> Result<usize, String> {
            out[..FRAME_SAMPLES].fill(f32::from(packet[0]) / 1000.0);
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
        Arc::new(|| Some(Box::new(ConstDecoder) as Box<dyn FrameDecoder + Send>))
    }

    fn packet(seq: u16, value: u8) -> RtpIn {
        RtpIn {
            ssrc: 1,
            seq,
            ts: u32::from(seq) * 960,
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
            let mut tick = tokio::time::interval(Duration::from_millis(20));
            let deadline = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < deadline {
                tick.tick().await;
                level.note_request(FRAME_SAMPLES);
                while let Ok(frame) = out_rx.try_recv() {
                    assert_eq!(frame.len(), FRAME_SAMPLES);
                    seen.push(frame[0]);
                }
                level.consumed(FRAME_SAMPLES);
            }
            seen
        })
    }

    /// Mix a short burst-free stream of `value` from one user at `gain`.
    async fn mix_user(gain: Option<f32>, value: u8) -> Vec<f32> {
        let (out_tx, out_rx) = mpsc::channel(8);
        let level = OutputLevel::new();
        let mixer =
            AudioMixer::with_decoder(out_tx, UserGains::default(), level.clone(), const_factory());
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
        device.await.unwrap()
    }

    #[tokio::test]
    async fn mixer_applies_user_gain() {
        let unity = mix_user(None, 200).await;
        assert!(unity.iter().any(|s| (s - 0.2).abs() < 1e-6), "{unity:?}");

        let half = mix_user(Some(0.5), 200).await;
        assert!(half.iter().any(|s| (s - 0.1).abs() < 1e-6), "{half:?}");
        assert!(half.iter().all(|s| *s < 0.1 + 1e-6));

        // Boost is allowed but the mix stays clipped to [-1, 1].
        let boosted = mix_user(Some(2.0), 250).await;
        assert!(
            boosted.iter().any(|s| (s - 0.5).abs() < 1e-6),
            "{boosted:?}"
        );
    }

    #[tokio::test]
    async fn zero_gain_is_silent() {
        let muted = mix_user(Some(0.0), 250).await;
        assert!(!muted.is_empty());
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
        // No device consumes anything: at most one block + headroom.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut produced = 0;
        while out_rx.try_recv().is_ok() {
            produced += 1;
        }
        assert_eq!(produced, frames_needed(0, 0));
        assert_eq!(level.queued(), produced * FRAME_SAMPLES);
        // The device plays it: the mixer follows.
        level.consumed(FRAME_SAMPLES);
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
        let block = block_ms * 96; // stereo samples at 48 kHz
        let mut queue = 0usize;
        let (mut produced, mut discarded, mut missing, mut consumed) = (0, 0, 0, 0);
        for t in 0..20_000usize {
            let steady = t >= 10_000;
            if t % 5 == 0 {
                for _ in 0..frames_needed(level.queued(), level.request()) {
                    level.produced(FRAME_SAMPLES);
                    queue += FRAME_SAMPLES;
                    let excess = crate::audio::queue_excess(queue);
                    queue -= excess;
                    level.consumed(excess);
                    if steady {
                        produced += 1;
                        discarded += excess;
                    }
                }
            }
            if t % block_ms == 0 {
                level.note_request(block);
                let take = queue.min(block);
                queue -= take;
                level.consumed(take);
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
        level.note_request(400 * 96); // one priming callback
        level.roll_window();
        for _ in 0..3 {
            level.note_request(10 * 96);
            level.roll_window();
        }
        assert_eq!(level.request(), 10 * 96);
        level.reset_request();
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

    #[test]
    fn frames_needed_keeps_one_block_plus_a_frame_queued() {
        // Nothing known about the device yet: one frame.
        assert_eq!(frames_needed(0, 0), 1);
        assert_eq!(frames_needed(FRAME_SAMPLES, 0), 0);
        // Device takes 1024 stereo frames (2048 samples) per callback.
        assert_eq!(frames_needed(0, 2048), 3);
        assert_eq!(frames_needed(2048, 2048), 1);
        assert_eq!(frames_needed(2048 + FRAME_SAMPLES, 2048), 0);
        // Never more than three per wake.
        assert_eq!(frames_needed(0, 48_000), MAX_FRAMES_PER_WAKE);
    }
}
