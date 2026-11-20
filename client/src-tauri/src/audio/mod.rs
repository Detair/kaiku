//! Audio Input/Output
//!
//! Handles audio capture, playback, encoding/decoding with cpal and opus.
//!
//! This module provides a Send + Sync audio handle that moves non-thread-safe
//! `cpal::Stream` objects into background tasks.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use thiserror::Error;

mod devices;
pub mod format;
mod handle;
pub mod ring;

pub use handle::{AudioErrorEvent, AudioErrorKind, AudioHandle, AUDIO_ERROR_EVENT};

/// Audio configuration constants
pub const SAMPLE_RATE: u32 = 48000;
/// Playback/mixer channel count (remote peers may send stereo). Capture is
/// opened in whatever format the device supports and downmixed to mono
/// 48 kHz for RNNoise/Opus (see `format::CaptureFormat`).
pub const CHANNELS: u16 = 2;
pub const FRAME_SIZE_MS: usize = 20;
/// 960 samples per channel.
pub const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS) / 1000;

/// `RNNoise` processes 480-sample frames (10ms at 48kHz mono)
pub const RNNOISE_FRAME_SIZE: usize = 480;

/// One encoded microphone frame on its way to the RTP sender, with its
/// place in the capture's timeline (the RTP clock follows it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    /// Identifies the capture stream (a restarted microphone gets a new
    /// one, counting from zero again).
    pub capture: u32,
    /// Index of this 20 ms frame in the capture stream; every captured
    /// frame counts, sent or not (gate closed, muted, not queued).
    pub frame: u64,
    /// When the capture produced it.
    pub captured_at: std::time::Instant,
    /// Gated or muted frames came right before this one.
    pub talkspurt: bool,
    pub opus: Vec<u8>,
}

/// Packet loss the voice encoder plans for (percent).
///
/// With in-band FEC on, libopus spends this much redundancy (LBRR: a
/// low-bitrate copy of the previous frame in each packet) only in SILK and
/// hybrid modes. 10 % suits a family on Wi-Fi: high enough that the encoder
/// actually emits LBRR at speech bitrates, low enough that the main frame
/// keeps most of the bits. The native sender reads no RTCP, so the value is
/// fixed rather than following the measured loss like a browser does.
pub const VOICE_PACKET_LOSS_PERC: i32 = 10;

/// The Opus encoder for the microphone: 48 kHz mono, `Application::Voip`, 20 ms frames
/// (set by the caller's frame size), in-band FEC on.
pub fn voice_encoder() -> Result<opus::Encoder, AudioError> {
    let err = |e: opus::Error| AudioError::EncoderError(e.to_string());
    let mut encoder =
        opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip)
            .map_err(err)?;
    encoder.set_inband_fec(true).map_err(err)?;
    encoder
        .set_packet_loss_perc(VOICE_PACKET_LOSS_PERC)
        .map_err(err)?;
    Ok(encoder)
}

/// Audio queued between the mixer and the output device, in interleaved
/// samples, the device's recent block size, and a count of completed
/// device callbacks.
///
/// The mixer adds what it sends, the output callback subtracts what the
/// device consumed and counts the callback. Both sides only use atomics,
/// so the real-time callback never waits on it.
///
/// Each output stream gets a generation ([`OutputLevel::begin_stream`]);
/// only the active one is measured, so the overlap of a device switch and
/// a window roll cannot mix the old device's block size into the new
/// one's. The block size is the largest request of the current and the
/// previous one-second window (the mixer rolls the window), capped at
/// [`MAX_REQUEST_SAMPLES`].
#[derive(Clone, Default)]
pub struct OutputLevel {
    queued: Arc<AtomicUsize>,
    /// `(generation << 32) | samples`: largest and smallest request of the
    /// current and the previous window.
    recent: Arc<AtomicU64>,
    previous: Arc<AtomicU64>,
    recent_min: Arc<AtomicU64>,
    previous_min: Arc<AtomicU64>,
    active: Arc<AtomicU32>,
    callbacks: Arc<AtomicU64>,
    /// Set when the queued audio went stale (the device was absent).
    flush: Arc<AtomicBool>,
}

/// One 20 ms stereo frame in interleaved samples.
const FRAME_SAMPLES: usize = FRAME_SIZE * CHANNELS as usize;

/// Largest device block the pacing follows: 400 ms of stereo 48 kHz. A
/// device asking for more per callback hears a short gap each callback;
/// the mixer never runs ahead of what is consumed.
pub const MAX_REQUEST_SAMPLES: usize = 400 * 96;

/// Samples the playback queue holds at most: above the most the mixer ever
/// queues (a low point covering the spread of block sizes, plus the largest
/// block, plus two frames), so in steady state nothing is discarded.
pub const MAX_QUEUE_SAMPLES: usize = 2 * MAX_REQUEST_SAMPLES + 3 * FRAME_SAMPLES;

/// Replace the value of `atomic` by `f(value)` with a lock-free
/// compare-and-swap loop (real-time safe). `Err` when `f` declines.
pub fn update_usize(
    atomic: &AtomicUsize,
    mut f: impl FnMut(usize) -> Option<usize>,
) -> Result<usize, usize> {
    let mut current = atomic.load(Ordering::Acquire);
    loop {
        let Some(new) = f(current) else {
            return Err(current);
        };
        match atomic.compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire) {
            Ok(previous) => return Ok(previous),
            Err(actual) => current = actual,
        }
    }
}

/// [`update_usize`] for `AtomicU64`.
fn update_u64(atomic: &AtomicU64, mut f: impl FnMut(u64) -> Option<u64>) -> Result<u64, u64> {
    let mut current = atomic.load(Ordering::Acquire);
    loop {
        let Some(new) = f(current) else {
            return Err(current);
        };
        match atomic.compare_exchange_weak(current, new, Ordering::AcqRel, Ordering::Acquire) {
            Ok(previous) => return Ok(previous),
            Err(actual) => current = actual,
        }
    }
}

const fn pack(generation: u32, samples: usize) -> u64 {
    ((generation as u64) << 32) | (samples as u64 & 0xFFFF_FFFF)
}

const fn unpack(value: u64) -> (u32, usize) {
    ((value >> 32) as u32, (value & 0xFFFF_FFFF) as usize)
}

impl OutputLevel {
    pub fn new() -> Self {
        Self::default()
    }

    /// The mixer sent `samples` towards the device.
    pub fn produced(&self, samples: usize) {
        self.queued.fetch_add(samples, Ordering::Relaxed);
    }

    /// The device consumed (or the queue discarded) `samples`.
    pub fn consumed(&self, samples: usize) {
        // Lock-free (a CAS loop); safe on the real-time thread.
        let _ = update_usize(&self.queued, |q| Some(q.saturating_sub(samples)));
    }

    /// A new output stream starts: it becomes the measured one. Returns
    /// its generation.
    pub fn begin_stream(&self) -> u32 {
        self.active.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }

    /// Make `generation` the measured stream again (a device switch failed
    /// and the previous stream keeps playing).
    pub fn activate(&self, generation: u32) {
        self.active.store(generation, Ordering::Release);
    }

    /// The generation currently measured.
    pub fn active_stream(&self) -> u32 {
        self.active.load(Ordering::Acquire)
    }

    /// Stream `generation` asked for a block of `samples` (real-time safe:
    /// a short CAS loop).
    pub fn note_request(&self, generation: u32, samples: usize) {
        if generation != self.active.load(Ordering::Acquire) {
            return;
        }
        let _ = update_u64(&self.recent, |value| {
            let (g, old) = unpack(value);
            let max = if g == generation {
                old.max(samples)
            } else {
                samples
            };
            Some(pack(generation, max))
        });
        let _ = update_u64(&self.recent_min, |value| {
            let (g, old) = unpack(value);
            let min = if g == generation && old > 0 {
                old.min(samples)
            } else {
                samples
            };
            Some(pack(generation, min))
        });
    }

    /// Smallest and largest block of the measured stream over the last one
    /// to two seconds (`None` before its first callback), each at most
    /// twice [`MAX_REQUEST_SAMPLES`].
    pub fn request_range(&self) -> Option<(usize, usize)> {
        let active = self.active.load(Ordering::Acquire);
        let of = |value: u64| match unpack(value) {
            (g, samples) if g == active && samples > 0 => Some(samples),
            _ => None,
        };
        let largest = [&self.recent, &self.previous]
            .iter()
            .filter_map(|v| of(v.load(Ordering::Relaxed)))
            .max()?;
        let smallest = [&self.recent_min, &self.previous_min]
            .iter()
            .filter_map(|v| of(v.load(Ordering::Relaxed)))
            .min()
            .unwrap_or(largest);
        let cap = 2 * MAX_REQUEST_SAMPLES;
        Some((smallest.min(cap), largest.min(cap)))
    }

    /// Stream `generation` finished a callback (after `consumed`).
    pub fn callback_done(&self, generation: u32) {
        if generation == self.active.load(Ordering::Acquire) {
            self.callbacks.fetch_add(1, Ordering::Release);
        }
    }

    /// What is queued for the device went stale: the next callback drops it.
    pub fn request_flush(&self) {
        self.flush.store(true, Ordering::Release);
    }

    /// Whether the queue is to be dropped (once; real-time safe).
    pub fn take_flush(&self) -> bool {
        self.flush.swap(false, Ordering::AcqRel)
    }

    /// Completed callbacks of the measured streams so far.
    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Acquire)
    }

    /// Samples sent but not yet played.
    pub fn queued(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }

    /// Recent block size of the measured stream (0 before its first
    /// callback), at most [`MAX_REQUEST_SAMPLES`].
    pub fn request(&self) -> usize {
        let active = self.active.load(Ordering::Acquire);
        let of = |value: u64| match unpack(value) {
            (g, samples) if g == active => samples,
            _ => 0,
        };
        of(self.recent.load(Ordering::Relaxed))
            .max(of(self.previous.load(Ordering::Relaxed)))
            .min(MAX_REQUEST_SAMPLES)
    }

    /// Start a new estimation window for the block size (once per second).
    pub fn roll_window(&self) {
        let active = self.active.load(Ordering::Acquire);
        let last = self.recent.swap(pack(active, 0), Ordering::Relaxed);
        // A value of an older stream is ignored by `request`.
        self.previous.store(last, Ordering::Relaxed);
        let last_min = self.recent_min.swap(pack(active, 0), Ordering::Relaxed);
        self.previous_min.store(last_min, Ordering::Relaxed);
    }
}

/// Shared VAD + noise suppression state, accessible from CPAL callback thread
#[derive(Clone)]
pub struct VadConfig {
    /// Whether VAD gating is enabled
    pub enabled: Arc<AtomicBool>,
    /// VAD threshold (0–100, mapped from 0.0–1.0 × 100 for atomic storage)
    pub threshold: Arc<AtomicU8>,
    /// Whether noise suppression (denoised output) is active
    pub denoise_enabled: Arc<AtomicBool>,
}

impl VadConfig {
    pub fn new() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            threshold: Arc::new(AtomicU8::new(50)), // 0.5 default
            denoise_enabled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_threshold(&self, threshold: f32) {
        let t = (threshold.clamp(0.0, 1.0) * 100.0) as u8;
        self.threshold.store(t, Ordering::Relaxed);
    }

    pub fn get_threshold(&self) -> f32 {
        f32::from(self.threshold.load(Ordering::Relaxed)) / 100.0
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_denoise(&self, enabled: bool) {
        self.denoise_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_denoise_enabled(&self) -> bool {
        self.denoise_enabled.load(Ordering::Relaxed)
    }
}

/// Audio errors
#[derive(Error, Debug)]
pub enum AudioError {
    #[error("No audio host available")]
    NoHost,
    #[error("No input device available")]
    NoInputDevice,
    #[error("No output device available")]
    NoOutputDevice,
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    #[error(
        "Audio device is busy: another program or the sound server is using it. Choose the system default instead."
    )]
    DeviceInUse,
    /// An earlier stream was told to stop but has not released its device
    /// yet; opening it again now would open the device twice.
    #[error("The audio device is still in use by the previous call. Try again in a moment.")]
    StillReleasing,
    /// A switch failed and the previous device could not be reopened, so
    /// capture now runs on the system default.
    #[error(
        "Could not switch the microphone ({0}). The previous microphone could not be reopened \
         either, so the system default is used now."
    )]
    UsingSystemDefault(String),
    #[error("Failed to get device config: {0}")]
    ConfigError(String),
    #[error("Failed to build stream: {0}")]
    StreamError(String),
    #[error("Opus encoder error: {0}")]
    EncoderError(String),
    #[error("Opus decoder error: {0}")]
    DecoderError(String),
    #[error("Permission denied")]
    PermissionDenied,
}

/// Audio device information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioDevice {
    pub device_id: String,
    pub label: String,
    pub is_default: bool,
    /// Offered by default. `false` for ALSA PCMs the curated list hides
    /// (shown with "Show all ALSA devices"); always `true` off Linux.
    pub curated: bool,
    /// Name older client versions stored as the selection (the device
    /// description). The UI matches a stored selection by `device_id` first
    /// and then by this name, like `devices::find_selection`.
    pub legacy_name: String,
}

/// List of audio devices
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioDeviceList {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_level_counts_produced_and_consumed_samples() {
        let level = OutputLevel::new();
        let device_side = level.clone();
        assert_eq!(level.queued(), 0);
        level.produced(1920);
        level.produced(1920);
        device_side.consumed(1000);
        assert_eq!(level.queued(), 2840);
        // Never below zero (the queue may discard more than was counted).
        device_side.consumed(10_000);
        assert_eq!(level.queued(), 0);
        assert_eq!(level.request(), 0);
        let stream = level.begin_stream();
        device_side.note_request(stream, 2048);
        device_side.note_request(stream, 512);
        assert_eq!(level.request(), 2048);
        device_side.callback_done(stream);
        assert_eq!(level.callbacks(), 1);
    }

    #[test]
    fn a_device_switch_measures_only_the_new_device() {
        let level = OutputLevel::new();
        let old = level.begin_stream();
        level.note_request(old, 400 * 96);
        let new = level.begin_stream();
        // The new device's first callback arrives at once; the old stream
        // still runs for a moment and must not count.
        level.note_request(new, 10 * 96);
        level.note_request(old, 400 * 96);
        level.callback_done(old);
        assert_eq!(level.request(), 10 * 96);
        assert_eq!(level.callbacks(), 0);
        // A window roll racing the switch cannot bring the old value back.
        level.roll_window();
        assert_eq!(level.request(), 10 * 96);
        level.roll_window();
        assert_eq!(level.request(), 0);
        // A failed switch hands measuring back to the old stream.
        level.activate(old);
        level.note_request(old, 100 * 96);
        assert_eq!(level.request(), 100 * 96);
    }

    #[test]
    fn voice_encoder_plans_for_packet_loss_with_inband_fec() {
        let mut encoder = voice_encoder().expect("encoder");
        assert!(encoder.get_inband_fec().unwrap());
        assert_eq!(
            encoder.get_packet_loss_perc().unwrap(),
            VOICE_PACKET_LOSS_PERC
        );
        // Sample rate and channels are unchanged.
        assert_eq!(encoder.get_sample_rate().unwrap(), SAMPLE_RATE);
        let mut out = [0u8; 4000];
        let frame = [0i16; FRAME_SIZE];
        assert!(encoder.encode(&frame, &mut out).unwrap() > 0);
    }

    #[test]
    fn test_create_audio_handle() {
        let handle = AudioHandle::new();
        assert!(handle.is_ok());
    }

    #[test]
    fn test_enumerate_devices() {
        let handle = AudioHandle::new().unwrap();
        let devices = handle.enumerate_devices();
        // May fail on CI without audio hardware, so just check it returns a result
        let _ = devices;
    }

    /// Manual check on real hardware:
    /// `cargo test -p vc-client --lib audio::tests::print_devices -- --ignored --nocapture`
    #[test]
    #[ignore = "needs audio hardware; prints the device list"]
    fn print_devices() {
        let handle = AudioHandle::new().unwrap();
        let devices = handle.enumerate_devices().unwrap();
        for (kind, list) in [("input", &devices.inputs), ("output", &devices.outputs)] {
            for d in list {
                println!(
                    "{kind}: {:<40} {} default={}",
                    d.label, d.device_id, d.is_default
                );
            }
        }
        let ids: std::collections::HashSet<_> =
            devices.inputs.iter().map(|d| &d.device_id).collect();
        assert_eq!(ids.len(), devices.inputs.len(), "input ids must be unique");
        let ids: std::collections::HashSet<_> =
            devices.outputs.iter().map(|d| &d.device_id).collect();
        assert_eq!(
            ids.len(),
            devices.outputs.len(),
            "output ids must be unique"
        );
    }

    #[test]
    fn test_mute_state() {
        let handle = AudioHandle::new().unwrap();
        assert!(!handle.is_muted());

        handle.set_muted(true);
        assert!(handle.is_muted());

        handle.set_muted(false);
        assert!(!handle.is_muted());
    }

    #[test]
    fn test_vad_config_defaults() {
        let config = VadConfig::new();
        assert!(!config.is_enabled());
        assert!((config.get_threshold() - 0.5).abs() < 0.01);
        assert!(!config.is_denoise_enabled());
    }

    #[test]
    fn test_vad_config_setters() {
        let config = VadConfig::new();
        config.set_enabled(true);
        config.set_threshold(0.7);
        config.set_denoise(true);
        assert!(config.is_enabled());
        assert!((config.get_threshold() - 0.7).abs() < 0.02); // u8 quantization
        assert!(config.is_denoise_enabled());
    }

    #[test]
    fn test_vad_config_threshold_clamping() {
        let config = VadConfig::new();
        config.set_threshold(1.5); // above max
        assert!((config.get_threshold() - 1.0).abs() < 0.01);
        config.set_threshold(-0.5); // below min
        assert!(config.get_threshold() < 0.01);
    }

    #[test]
    fn test_deafen_state() {
        let handle = AudioHandle::new().unwrap();
        assert!(!handle.is_deafened());

        handle.set_deafened(true);
        assert!(handle.is_deafened());
        assert!(handle.is_muted()); // Deafen also mutes

        handle.set_deafened(false);
        assert!(!handle.is_deafened());
    }
}
