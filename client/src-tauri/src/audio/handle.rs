//! Audio Handle - Send + Sync wrapper for audio system
//!
//! This module provides a thread-safe handle to the audio system by moving
//! non-Send/Sync types (`cpal::Stream`) into background tasks.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, FromSample, Host, Sample, SampleFormat, SizedSample};
use opus::{Channels as OpusChannels, Decoder, Encoder};
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::format::{negotiate_capture_format, CaptureFormat, CaptureFrontEnd};
use super::{
    AudioDevice, AudioDeviceList, AudioError, VadConfig, CHANNELS, FRAME_SIZE, RNNOISE_FRAME_SIZE,
    SAMPLE_RATE,
};

/// Tauri event emitted when an audio stream dies at runtime; payload is an
/// [`AudioErrorEvent`].
pub const AUDIO_ERROR_EVENT: &str = "voice:audio_error";

/// No capture callback for this long means the microphone is gone (some
/// backends never report an unplugged device, the stream just stops).
pub const CAPTURE_STALL_TIMEOUT: Duration = Duration::from_secs(3);

/// How often the capture owner thread checks for a stall.
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(500);

/// Which audio stream failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioErrorKind {
    CaptureFailed,
    PlaybackFailed,
}

/// Payload of [`AUDIO_ERROR_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AudioErrorEvent {
    pub kind: AudioErrorKind,
    /// Device name as shown in the device list.
    pub device: String,
}

/// Where audio failures are delivered (the Tauri app in production).
pub type AudioErrorSink = Arc<dyn Fn(&AudioErrorEvent) + Send + Sync>;

/// Reports one stream's failure at most once, from whichever path notices
/// first (backend error callback or stall watchdog).
#[derive(Clone)]
struct FailureReporter {
    event: Arc<AudioErrorEvent>,
    sink: Option<AudioErrorSink>,
    reported: Arc<AtomicBool>,
}

impl FailureReporter {
    fn new(kind: AudioErrorKind, device: impl Into<String>, sink: Option<AudioErrorSink>) -> Self {
        Self {
            event: Arc::new(AudioErrorEvent {
                kind,
                device: device.into(),
            }),
            sink,
            reported: Arc::new(AtomicBool::new(false)),
        }
    }

    fn report(&self, reason: &str) {
        if self.reported.swap(true, Ordering::AcqRel) {
            return;
        }
        error!(
            kind = ?self.event.kind,
            device = %self.event.device,
            "Audio stream failed: {reason}"
        );
        if let Some(sink) = &self.sink {
            sink(&self.event);
        }
    }
}

/// CPAL error callback: a lost or invalidated device is fatal for the
/// stream and is reported; under/overruns and backend hiccups are glitches
/// (a stream that really stopped is caught by the capture watchdog).
fn stream_error_callback(reporter: FailureReporter) -> impl FnMut(cpal::StreamError) + Send {
    move |err| match err {
        cpal::StreamError::DeviceNotAvailable | cpal::StreamError::StreamInvalidated => {
            reporter.report(&err.to_string());
        }
        other => warn!(kind = ?reporter.event.kind, "Audio stream glitch: {other}"),
    }
}

/// Time of the last capture callback, written lock-free from the callback.
#[derive(Clone)]
struct Heartbeat {
    epoch: Instant,
    last_ms: Arc<AtomicU64>,
}

impl Heartbeat {
    fn new() -> Self {
        Self {
            epoch: Instant::now(),
            last_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    #[cfg(test)]
    const fn epoch(&self) -> Instant {
        self.epoch
    }

    /// Called at the top of every capture callback (one clock read + store).
    fn beat(&self) {
        self.last_ms
            .store(self.epoch.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    fn last(&self) -> Instant {
        self.epoch + Duration::from_millis(self.last_ms.load(Ordering::Relaxed))
    }
}

/// Reports a capture stream whose callbacks stopped arriving.
struct CaptureWatchdog {
    heartbeat: Heartbeat,
    reporter: FailureReporter,
}

impl CaptureWatchdog {
    const fn new(heartbeat: Heartbeat, reporter: FailureReporter) -> Self {
        Self {
            heartbeat,
            reporter,
        }
    }

    fn check(&self, now: Instant) {
        if now.saturating_duration_since(self.heartbeat.last()) > CAPTURE_STALL_TIMEOUT {
            self.reporter.report("no audio from the microphone");
        }
    }
}

/// Control messages for capture-side owner threads (capture, mic test).
enum CaptureControl {
    Stop,
}

/// A thread that owns a (non-`Send`) CPAL stream until told to stop.
struct StreamOwner {
    control: std_mpsc::Sender<CaptureControl>,
    thread: JoinHandle<()>,
}

impl StreamOwner {
    /// Stop the stream and wait until it has been dropped (the device is
    /// released when this returns).
    async fn stop(self) {
        let _ = self.control.send(CaptureControl::Stop);
        let thread = self.thread;
        let _ = tokio::task::spawn_blocking(move || thread.join()).await;
    }
}

/// Build a stream on a dedicated thread and keep it alive until `Stop`.
///
/// Waits for the build: a stream that cannot be created or started is
/// returned as an error instead of leaving a silent call behind. While the
/// stream runs, `watchdog` (if any) is checked every [`WATCHDOG_INTERVAL`].
async fn spawn_stream_owner<S, B>(
    build: B,
    watchdog: Option<CaptureWatchdog>,
) -> Result<StreamOwner, AudioError>
where
    B: FnOnce() -> Result<S, AudioError> + Send + 'static,
    S: 'static,
{
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (control, control_rx) = std_mpsc::channel();

    let thread = std::thread::Builder::new()
        .name("kaiku-audio-in".into())
        .spawn(move || {
            let stream = match build() {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            loop {
                match control_rx.recv_timeout(WATCHDOG_INTERVAL) {
                    Ok(CaptureControl::Stop) | Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                        break
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(watchdog) = &watchdog {
                            watchdog.check(Instant::now());
                        }
                    }
                }
            }
            drop(stream);
            debug!("Audio input stream owner stopped");
        })
        .map_err(|e| AudioError::StreamError(e.to_string()))?;

    ready_rx.await.map_err(|_| {
        AudioError::StreamError("audio thread ended before the stream started".into())
    })??;
    Ok(StreamOwner { control, thread })
}

/// A running capture stream. Frames are only sent while `active` is set, so
/// a replacement stream can be built before the old one is released.
struct CaptureTask {
    owner: StreamOwner,
    active: Arc<AtomicBool>,
    /// Set once the stream was reported as failed (unplugged, stalled).
    failed: Arc<AtomicBool>,
}

impl CaptureTask {
    async fn stop(self) {
        self.active.store(false, Ordering::SeqCst);
        self.owner.stop().await;
    }
}

/// Audio handle that can be safely shared across threads
pub struct AudioHandle {
    /// Audio host (thread-safe)
    host: Arc<Host>,

    /// Muted state (atomic for thread-safe access)
    muted: Arc<AtomicBool>,

    /// Deafened state (atomic for thread-safe access)
    deafened: Arc<AtomicBool>,

    /// Microphone test level (0-100)
    mic_test_level: Arc<AtomicU8>,

    /// Running capture stream, if any
    capture: Option<CaptureTask>,

    /// Where captured Opus frames go; kept to rebuild capture on a new device
    capture_output_tx: Option<mpsc::Sender<Vec<u8>>>,

    /// Control channel for playback task
    playback_control: Option<mpsc::Sender<PlaybackControl>>,

    /// Running mic test stream, if any
    mic_test: Option<StreamOwner>,

    /// Selected input device name
    input_device_name: Option<String>,

    /// Selected output device name
    output_device_name: Option<String>,

    /// VAD + noise suppression configuration
    vad_config: VadConfig,

    /// Tauri app handle for emitting events from capture thread
    app_handle: Option<tauri::AppHandle>,
}

/// Control messages for playback task
enum PlaybackControl {
    Stop,
    /// Rebuild the output stream on a new device, keeping the same sample
    /// buffer (mid-call speaker switching). Boxed: `cpal::Device` is large.
    SwitchDevice(Box<Device>),
}

/// Device name as listed by `enumerate_devices`.
fn device_label(device: &Device) -> String {
    device
        .description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| "default".to_string())
}

impl AudioHandle {
    /// Create a new audio handle
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();

        Ok(Self {
            host: Arc::new(host),
            muted: Arc::new(AtomicBool::new(false)),
            deafened: Arc::new(AtomicBool::new(false)),
            mic_test_level: Arc::new(AtomicU8::new(0)),
            capture: None,
            capture_output_tx: None,
            playback_control: None,
            mic_test: None,
            input_device_name: None,
            output_device_name: None,
            vad_config: VadConfig::new(),
            app_handle: None,
        })
    }

    /// Enumerate all audio devices
    pub fn enumerate_devices(&self) -> Result<AudioDeviceList, AudioError> {
        let default_input = self.host.default_input_device();
        let default_output = self.host.default_output_device();

        let default_input_name = default_input
            .as_ref()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string());
        let default_output_name = default_output
            .as_ref()
            .and_then(|d| d.description().ok())
            .map(|desc| desc.name().to_string());

        let inputs: Vec<AudioDevice> = self
            .host
            .input_devices()
            .map_err(|e| AudioError::ConfigError(e.to_string()))?
            .filter_map(|d| {
                d.description().ok().map(|desc| {
                    let name = desc.name().to_string();
                    AudioDevice {
                        device_id: name.clone(),
                        label: name.clone(),
                        is_default: Some(&name) == default_input_name.as_ref(),
                    }
                })
            })
            .collect();

        let outputs: Vec<AudioDevice> = self
            .host
            .output_devices()
            .map_err(|e| AudioError::ConfigError(e.to_string()))?
            .filter_map(|d| {
                d.description().ok().map(|desc| {
                    let name = desc.name().to_string();
                    AudioDevice {
                        device_id: name.clone(),
                        label: name.clone(),
                        is_default: Some(&name) == default_output_name.as_ref(),
                    }
                })
            })
            .collect();

        Ok(AudioDeviceList { inputs, outputs })
    }

    /// Set the input device by name (`None` = system default).
    ///
    /// Outside a call this only stores the choice. During a call the capture
    /// stream moves to the new device (see [`Self::switch_capture_device`]);
    /// if that fails, the previous microphone keeps running and the stored
    /// choice is unchanged.
    pub async fn set_input_device(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        if self.capture_output_tx.is_none() {
            self.input_device_name = device_id;
            return Ok(());
        }
        self.switch_capture_to(device_id).await
    }

    /// Move a running capture to the input device `name` mid-call.
    ///
    /// The new stream is built first and only then replaces the old one, so
    /// a failed switch keeps the current microphone. Also the recovery path
    /// after "Microphone stopped working": picking a microphone restarts
    /// capture even when the previous stream died.
    pub async fn switch_capture_device(&mut self, name: &str) -> Result<(), AudioError> {
        self.switch_capture_to(Some(name.to_string())).await
    }

    async fn switch_capture_to(&mut self, device_name: Option<String>) -> Result<(), AudioError> {
        let Some(output_tx) = self.capture_output_tx.clone() else {
            self.input_device_name = device_name;
            return Ok(());
        };
        // Re-applying the device that is already capturing fine (e.g. the
        // saved device on join) is a no-op; a failed stream is rebuilt.
        let healthy = self
            .capture
            .as_ref()
            .is_some_and(|task| !task.failed.load(Ordering::Acquire));
        if healthy && device_name == self.input_device_name {
            return Ok(());
        }

        let new_task = match self
            .spawn_capture(device_name.as_deref(), output_tx.clone())
            .await
        {
            Ok(task) => task,
            Err(first) if self.capture.is_some() => {
                // Some devices allow only one open stream (re-selecting the
                // same microphone): release the old stream and retry once.
                debug!("Capture switch failed with the old stream open ({first}); retrying");
                if let Some(old) = self.capture.take() {
                    old.stop().await;
                }
                match self
                    .spawn_capture(device_name.as_deref(), output_tx.clone())
                    .await
                {
                    Ok(task) => task,
                    Err(e) => {
                        warn!("Capture switch failed: {e}; restoring the previous microphone");
                        let previous = self.input_device_name.clone();
                        match self.spawn_capture(previous.as_deref(), output_tx).await {
                            Ok(task) => {
                                task.active.store(true, Ordering::SeqCst);
                                self.capture = Some(task);
                            }
                            Err(restore) => {
                                error!("Previous microphone could not be restored: {restore}");
                                self.report_capture_failure(previous.as_deref());
                            }
                        }
                        return Err(e);
                    }
                }
            }
            Err(e) => return Err(e),
        };

        if let Some(old) = self.capture.take() {
            old.stop().await;
        }
        new_task.active.store(true, Ordering::SeqCst);
        self.capture = Some(new_task);
        self.input_device_name = device_name;
        // The old stream may have left the speaking indicator on.
        if let Some(app) = &self.app_handle {
            let _ = app.emit("voice:speaking", false);
        }
        info!("Capture switched to {:?}", self.input_device_name);
        Ok(())
    }

    /// Set the output device by name.
    ///
    /// Stores the name for future playback starts AND live-switches any
    /// running playback task: the task rebuilds its CPAL stream on the new
    /// device while keeping the same sample buffer, so audio moves to the
    /// new speaker mid-call without dropping frames.
    ///
    /// Errors if the named device doesn't exist (the stored name is still
    /// updated, matching the lookup-or-default behavior at stream start).
    pub async fn set_output_device(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        self.output_device_name = device_id;
        if let Some(control) = &self.playback_control {
            let device = self.get_device(self.output_device_name.as_deref(), false)?;
            if control
                .send(PlaybackControl::SwitchDevice(Box::new(device)))
                .await
                .is_err()
            {
                debug!("Playback task not running; output device applies on next start");
            }
        }
        Ok(())
    }

    /// Get device by name
    fn get_device(&self, device_name: Option<&str>, is_input: bool) -> Result<Device, AudioError> {
        match device_name {
            Some(name) => {
                let mut devices = if is_input {
                    self.host.input_devices()
                } else {
                    self.host.output_devices()
                }
                .map_err(|e| AudioError::ConfigError(e.to_string()))?;

                devices
                    .find(|d| {
                        d.description()
                            .map(|desc| desc.name() == name)
                            .unwrap_or(false)
                    })
                    .ok_or_else(|| AudioError::DeviceNotFound(name.to_string()))
            }
            None => {
                if is_input {
                    self.host
                        .default_input_device()
                        .ok_or(AudioError::NoInputDevice)
                } else {
                    self.host
                        .default_output_device()
                        .ok_or(AudioError::NoOutputDevice)
                }
            }
        }
    }

    /// Delivers audio failures to the webview as [`AUDIO_ERROR_EVENT`].
    fn error_sink(&self) -> Option<AudioErrorSink> {
        let app = self.app_handle.clone()?;
        Some(Arc::new(move |event: &AudioErrorEvent| {
            if let Err(e) = app.emit(AUDIO_ERROR_EVENT, event) {
                warn!("Failed to emit {AUDIO_ERROR_EVENT}: {e}");
            }
        }))
    }

    /// Tell the user capture is gone when no stream could be (re)started.
    fn report_capture_failure(&self, device_name: Option<&str>) {
        FailureReporter::new(
            AudioErrorKind::CaptureFailed,
            device_name.unwrap_or("default"),
            self.error_sink(),
        )
        .report("capture could not be restarted");
    }

    /// Build (but do not activate) a capture stream on `device_name`.
    async fn spawn_capture(
        &self,
        device_name: Option<&str>,
        output_tx: mpsc::Sender<Vec<u8>>,
    ) -> Result<CaptureTask, AudioError> {
        let device = self.get_device(device_name, true)?;
        let reporter = FailureReporter::new(
            AudioErrorKind::CaptureFailed,
            device_label(&device),
            self.error_sink(),
        );
        let heartbeat = Heartbeat::new();
        let active = Arc::new(AtomicBool::new(false));
        let ctx = CaptureContext {
            muted: self.muted.clone(),
            output_tx,
            vad_config: self.vad_config.clone(),
            app_handle: self.app_handle.clone(),
            active: active.clone(),
            heartbeat: heartbeat.clone(),
            reporter: reporter.clone(),
        };
        let failed = reporter.reported.clone();
        let watchdog = CaptureWatchdog::new(heartbeat, reporter);
        let owner =
            spawn_stream_owner(move || build_capture_stream(&device, ctx), Some(watchdog)).await?;
        Ok(CaptureTask {
            owner,
            active,
            failed,
        })
    }

    /// Start audio capture in a background thread.
    ///
    /// Returns an error when the device cannot be opened in any supported
    /// format, so the call does not continue with a dead microphone. Later
    /// failures (unplugged device, stalled stream) are emitted as
    /// [`AUDIO_ERROR_EVENT`].
    pub async fn start_capture(
        &mut self,
        output_tx: mpsc::Sender<Vec<u8>>,
    ) -> Result<(), AudioError> {
        // Stop existing capture if running
        self.stop_capture().await;

        // Kept even if the build fails: choosing another microphone during
        // the call then starts capture (see `set_input_device`).
        self.capture_output_tx = Some(output_tx.clone());
        let task = self
            .spawn_capture(self.input_device_name.as_deref(), output_tx)
            .await?;
        task.active.store(true, Ordering::SeqCst);
        self.capture = Some(task);

        info!("Audio capture started");
        Ok(())
    }

    /// Selected input device name (`None` = system default).
    pub fn input_device_name(&self) -> Option<&str> {
        self.input_device_name.as_deref()
    }

    /// Stop audio capture
    pub async fn stop_capture(&mut self) {
        self.capture_output_tx = None;
        if let Some(task) = self.capture.take() {
            task.stop().await;
            debug!("Audio capture stopped");
        }
    }

    /// Start audio playback in a background task (Opus-encoded input).
    pub async fn start_playback(
        &mut self,
        input_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<(), AudioError> {
        // Stop existing playback if running
        self.stop_playback().await;

        let device = self.get_device(self.output_device_name.as_deref(), false)?;
        let deafened = self.deafened.clone();
        let sink = self.error_sink();

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<PlaybackControl>(1);
        self.playback_control = Some(control_tx);

        // Spawn playback task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_playback_task(device, deafened, input_rx, &mut control_rx, sink);
        });

        info!("Audio playback started");
        Ok(())
    }

    /// Start PCM playback from already-decoded f32 samples (used by the audio mixer).
    ///
    /// Unlike `start_playback`, this skips Opus decoding — the mixer has already
    /// decoded each track individually before mixing.
    pub async fn start_pcm_playback(
        &mut self,
        input_rx: mpsc::Receiver<Vec<f32>>,
    ) -> Result<(), AudioError> {
        // Stop existing playback if running
        self.stop_playback().await;

        let device = self.get_device(self.output_device_name.as_deref(), false)?;
        let deafened = self.deafened.clone();
        let sink = self.error_sink();

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<PlaybackControl>(1);
        self.playback_control = Some(control_tx);

        // Spawn PCM playback task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_pcm_playback_task(device, deafened, input_rx, &mut control_rx, sink);
        });

        info!("PCM playback started (mixer output)");
        Ok(())
    }

    /// Stop audio playback
    pub async fn stop_playback(&mut self) {
        if let Some(control) = self.playback_control.take() {
            let _ = control.send(PlaybackControl::Stop).await;
            debug!("Audio playback stopped");
        }
    }

    /// Set muted state
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        debug!("Muted: {}", muted);
    }

    /// Get muted state
    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    /// Set deafened state (also mutes)
    pub fn set_deafened(&self, deafened: bool) {
        self.deafened.store(deafened, Ordering::Relaxed);
        if deafened {
            self.muted.store(true, Ordering::Relaxed);
        }
        debug!("Deafened: {}", deafened);
    }

    /// Get deafened state
    pub fn is_deafened(&self) -> bool {
        self.deafened.load(Ordering::Relaxed)
    }

    /// Set VAD configuration
    pub fn set_vad_config(&self, enabled: bool, threshold: f32) {
        self.vad_config.set_enabled(enabled);
        self.vad_config.set_threshold(threshold);
        debug!(
            "VAD config: enabled={}, threshold={:.2}",
            enabled, threshold
        );
    }

    /// Set noise suppression
    pub fn set_noise_suppression(&self, enabled: bool) {
        self.vad_config.set_denoise(enabled);
        debug!("Noise suppression: {}", enabled);
    }

    /// Get VAD config (for passing to capture task)
    pub fn vad_config(&self) -> VadConfig {
        self.vad_config.clone()
    }

    /// Set Tauri app handle (call during voice initialization before `start_capture`)
    pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
        self.app_handle = Some(handle);
    }

    /// Start microphone test
    pub async fn start_mic_test(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        // Stop existing test if running
        self.stop_mic_test().await;

        let device = self.get_device(device_id.as_deref(), true)?;
        let mic_level = self.mic_test_level.clone();

        let owner =
            spawn_stream_owner(move || build_mic_test_stream(&device, mic_level), None).await?;
        self.mic_test = Some(owner);

        info!("Microphone test started");
        Ok(())
    }

    /// Stop microphone test
    pub async fn stop_mic_test(&mut self) {
        if let Some(owner) = self.mic_test.take() {
            owner.stop().await;
            self.mic_test_level.store(0, Ordering::Relaxed);
            debug!("Microphone test stopped");
        }
    }

    /// Get microphone test level (0-100)
    pub fn get_mic_test_level(&self) -> u8 {
        self.mic_test_level.load(Ordering::Relaxed)
    }

    /// Check if microphone test is running
    pub const fn is_mic_test_running(&self) -> bool {
        self.mic_test.is_some()
    }

    /// Stop all audio streams
    pub async fn stop_all(&mut self) {
        self.stop_capture().await;
        self.stop_playback().await;
        self.stop_mic_test().await;
        info!("All audio streams stopped");
    }
}

/// Sample formats the capture path converts from.
const fn is_supported_sample_format(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32 | SampleFormat::I16 | SampleFormat::I32 | SampleFormat::U16
    )
}

/// Pick the capture format the device really supports: 48 kHz mono when
/// offered, otherwise what it has (resampled and downmixed afterwards).
fn capture_config(device: &Device) -> (CaptureFormat, SampleFormat) {
    let default = device
        .default_input_config()
        .ok()
        .filter(|c| is_supported_sample_format(c.sample_format()));
    let ranges: Vec<cpal::SupportedStreamConfigRange> = device
        .supported_input_configs()
        .map(|configs| {
            configs
                .filter(|r| is_supported_sample_format(r.sample_format()))
                .collect()
        })
        .unwrap_or_default();

    // Default first: it is the fallback when nothing offers 48 kHz.
    let supported: Vec<(u32, u32, u16)> = default
        .iter()
        .map(|d| (d.sample_rate(), d.sample_rate(), d.channels()))
        .chain(
            ranges
                .iter()
                .map(|r| (r.min_sample_rate(), r.max_sample_rate(), r.channels())),
        )
        .collect();
    let format = negotiate_capture_format(&supported);

    // Prefer f32 among the ranges that match; else the default's format.
    let sample_format = ranges
        .iter()
        .filter(|r| {
            r.channels() == format.channels
                && r.min_sample_rate() <= format.sample_rate
                && format.sample_rate <= r.max_sample_rate()
        })
        .map(cpal::SupportedStreamConfigRange::sample_format)
        .min_by_key(|f| u8::from(*f != SampleFormat::F32))
        .or_else(|| {
            default
                .as_ref()
                .map(cpal::SupportedStreamConfig::sample_format)
        })
        .unwrap_or(SampleFormat::F32);

    (format, sample_format)
}

/// What the capture callback needs besides the device format.
struct CaptureContext {
    muted: Arc<AtomicBool>,
    output_tx: mpsc::Sender<Vec<u8>>,
    vad_config: VadConfig,
    app_handle: Option<tauri::AppHandle>,
    active: Arc<AtomicBool>,
    heartbeat: Heartbeat,
    reporter: FailureReporter,
}

/// Open and start the capture stream (runs on the owner thread).
fn build_capture_stream(device: &Device, ctx: CaptureContext) -> Result<cpal::Stream, AudioError> {
    let (format, sample_format) = capture_config(device);
    info!(
        sample_rate = format.sample_rate,
        channels = format.channels,
        ?sample_format,
        "Opening capture stream"
    );
    match sample_format {
        SampleFormat::I16 => build_capture_stream_as::<i16>(device, format, ctx),
        SampleFormat::I32 => build_capture_stream_as::<i32>(device, format, ctx),
        SampleFormat::U16 => build_capture_stream_as::<u16>(device, format, ctx),
        _ => build_capture_stream_as::<f32>(device, format, ctx),
    }
}

fn build_capture_stream_as<T>(
    device: &Device,
    format: CaptureFormat,
    ctx: CaptureContext,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: format.channels,
        sample_rate: format.sample_rate,
        buffer_size: BufferSize::Default,
    };

    let CaptureContext {
        muted,
        output_tx,
        vad_config,
        app_handle,
        active,
        heartbeat,
        reporter,
    } = ctx;

    let mut processor = CaptureProcessor::new(output_tx, vad_config, app_handle)?;
    let mut front_end = CaptureFrontEnd::new(format);
    // Device samples as f32 (pre-allocated; grows only on an unusually large callback).
    let mut converted: Vec<f32> = Vec::with_capacity(8192);

    let stream = device
        .build_input_stream(
            &config,
            move |data: &[T], _| {
                heartbeat.beat();
                if !active.load(Ordering::Relaxed) {
                    return;
                }
                if muted.load(Ordering::Relaxed) {
                    processor.on_muted();
                    return;
                }
                converted.clear();
                converted.extend(data.iter().map(|&s| f32::from_sample(s)));
                front_end.push(&converted, |frame| processor.process_frame(frame));
            },
            stream_error_callback(reporter),
            None,
        )
        .map_err(|e| AudioError::StreamError(e.to_string()))?;

    stream
        .play()
        .map_err(|e| AudioError::StreamError(e.to_string()))?;
    Ok(stream)
}

/// Speech hold-open duration: keep gate open for 300ms after last speech detection
/// to avoid clipping word endings.
const SPEECH_HOLD_MS: u128 = 300;

/// Per-frame capture processing on 48 kHz mono 20 ms frames: `RNNoise` on
/// each 480-sample (10 ms) half for VAD probability + denoising, VAD gating,
/// Opus encoding. All buffers are allocated once, before the stream starts.
struct CaptureProcessor {
    encoder: Encoder,
    denoiser: Box<nnnoiseless::DenoiseState<'static>>,
    first_frame: bool,
    rnnoise_input: [f32; RNNOISE_FRAME_SIZE],
    rnnoise_output: [f32; RNNOISE_FRAME_SIZE],
    denoised: [f32; FRAME_SIZE],
    samples_i16: [i16; FRAME_SIZE],
    encode_buf: Vec<u8>,
    gate_open: bool,
    indicator_speaking: bool,
    last_speech_time: Instant,
    vad_config: VadConfig,
    output_tx: mpsc::Sender<Vec<u8>>,
    app_handle: Option<tauri::AppHandle>,
}

impl CaptureProcessor {
    fn new(
        output_tx: mpsc::Sender<Vec<u8>>,
        vad_config: VadConfig,
        app_handle: Option<tauri::AppHandle>,
    ) -> Result<Self, AudioError> {
        // Mono Opus encoder — capture is mono, playback remains stereo
        let encoder = Encoder::new(SAMPLE_RATE, OpusChannels::Mono, opus::Application::Voip)
            .map_err(|e| AudioError::EncoderError(e.to_string()))?;
        Ok(Self {
            encoder,
            denoiser: nnnoiseless::DenoiseState::new(),
            first_frame: true,
            rnnoise_input: [0.0; RNNOISE_FRAME_SIZE],
            rnnoise_output: [0.0; RNNOISE_FRAME_SIZE],
            denoised: [0.0; FRAME_SIZE],
            samples_i16: [0; FRAME_SIZE],
            encode_buf: vec![0u8; 4000],
            gate_open: false,
            indicator_speaking: false,
            last_speech_time: Instant::now(),
            vad_config,
            output_tx,
            app_handle,
        })
    }

    fn emit_speaking(&self, speaking: bool) {
        if let Some(app) = &self.app_handle {
            let _ = app.emit("voice:speaking", speaking);
        }
    }

    /// When muted, skip all processing but emit not-speaking if needed.
    fn on_muted(&mut self) {
        if self.indicator_speaking {
            self.indicator_speaking = false;
            self.gate_open = false;
            self.emit_speaking(false);
        }
    }

    /// Process one 960-sample 48 kHz mono frame.
    fn process_frame(&mut self, frame: &[f32]) {
        for (half, original) in frame.chunks_exact(RNNOISE_FRAME_SIZE).enumerate() {
            // RNNoise works in the i16 range.
            for (dst, &s) in self.rnnoise_input.iter_mut().zip(original) {
                *dst = s * 32767.0;
            }
            self.rnnoise_output.fill(0.0);
            let vad_prob = self
                .denoiser
                .process_frame(&mut self.rnnoise_output, &self.rnnoise_input);

            if self.first_frame {
                // First frame output has a fade-in artifact — discard.
                self.first_frame = false;
                self.rnnoise_output.fill(0.0);
            }

            let offset = half * RNNOISE_FRAME_SIZE;
            for (dst, &s) in self.denoised[offset..offset + RNNOISE_FRAME_SIZE]
                .iter_mut()
                .zip(self.rnnoise_output.iter())
            {
                *dst = s / 32768.0;
            }

            // --- VAD gate logic ---
            if vad_prob > self.vad_config.get_threshold() {
                if !self.indicator_speaking {
                    self.indicator_speaking = true;
                    self.emit_speaking(true);
                }
                self.last_speech_time = Instant::now();
            } else if self.indicator_speaking
                && self.last_speech_time.elapsed().as_millis() > SPEECH_HOLD_MS
            {
                self.indicator_speaking = false;
                self.emit_speaking(false);
            }

            // Gate decision: if VAD disabled, always open
            self.gate_open = !self.vad_config.is_enabled() || self.indicator_speaking;
        }

        if !self.gate_open {
            return;
        }

        // Choose denoised or original audio
        let source: &[f32] = if self.vad_config.is_denoise_enabled() {
            &self.denoised
        } else {
            frame
        };
        for (dst, &s) in self.samples_i16.iter_mut().zip(source) {
            *dst = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        }

        match self.encoder.encode(&self.samples_i16, &mut self.encode_buf) {
            Ok(len) => {
                let encoded = self.encode_buf[..len].to_vec();
                if let Err(e) = self.output_tx.try_send(encoded) {
                    warn!("Failed to send encoded audio: {}", e);
                }
            }
            Err(e) => {
                error!("Opus encode error: {}", e);
            }
        }
    }
}

/// Run playback task (owns the Stream)
fn run_playback_task(
    device: Device,
    deafened: Arc<AtomicBool>,
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    control_rx: &mut mpsc::Receiver<PlaybackControl>,
    sink: Option<AudioErrorSink>,
) {
    let decoder = match Decoder::new(SAMPLE_RATE, OpusChannels::Stereo) {
        Ok(dec) => Arc::new(std::sync::Mutex::new(dec)),
        Err(e) => {
            error!("Failed to create decoder: {}", e);
            return;
        }
    };

    let playback_buffer = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));

    // Spawn decoding task
    let decoder_clone = decoder;
    let playback_buffer_clone = playback_buffer.clone();
    std::thread::spawn(move || {
        while let Some(encoded) = input_rx.blocking_recv() {
            if let Ok(mut dec) = decoder_clone.lock() {
                let mut decoded = vec![0i16; FRAME_SIZE * CHANNELS as usize * 2];
                match dec.decode(&encoded, &mut decoded, false) {
                    Ok(len) => {
                        let samples_f32: Vec<f32> = decoded[..len]
                            .iter()
                            .map(|&s| f32::from(s) / 32768.0)
                            .collect();

                        if let Ok(mut buffer) = playback_buffer_clone.lock() {
                            buffer.extend(samples_f32);
                        }
                    }
                    Err(e) => {
                        error!("Opus decode error: {}", e);
                    }
                }
            }
        }
    });

    let reporter = FailureReporter::new(
        AudioErrorKind::PlaybackFailed,
        device_label(&device),
        sink.clone(),
    );
    let Some(stream) = build_buffer_output_stream(&device, &deafened, &playback_buffer, &reporter)
    else {
        reporter.report("output stream could not be started");
        return;
    };

    playback_control_loop(stream, &deafened, &playback_buffer, control_rx, sink);
    info!("Playback task stopped");
}

/// Build and start a CPAL output stream that drains the shared sample
/// buffer. Returns `None` (with the error logged) when the stream can't be
/// created or started on the given device. Runtime failures go to `reporter`.
fn build_buffer_output_stream(
    device: &Device,
    deafened: &Arc<AtomicBool>,
    playback_buffer: &Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
    reporter: &FailureReporter,
) -> Option<cpal::Stream> {
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: BufferSize::Default,
    };

    let playback_buffer = playback_buffer.clone();
    let deafened = deafened.clone();

    let stream = match device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            if deafened.load(Ordering::Relaxed) {
                data.fill(0.0);
                return;
            }

            if let Ok(mut buffer) = playback_buffer.lock() {
                let available = buffer.len().min(data.len());
                #[allow(clippy::needless_range_loop)]
                for i in 0..available {
                    data[i] = buffer.pop_front().unwrap();
                }
                #[allow(clippy::needless_range_loop)]
                for i in available..data.len() {
                    data[i] = 0.0;
                }
            } else {
                data.fill(0.0);
            }
        },
        stream_error_callback(reporter.clone()),
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to build playback stream: {}", e);
            return None;
        }
    };

    if let Err(e) = stream.play() {
        error!("Failed to start playback stream: {}", e);
        return None;
    }

    Some(stream)
}

/// Block on control messages until Stop, rebuilding the output stream in
/// place on `SwitchDevice`. The new stream is built BEFORE the old one is
/// dropped so a failed switch keeps audio on the previous device.
fn playback_control_loop(
    mut stream: cpal::Stream,
    deafened: &Arc<AtomicBool>,
    playback_buffer: &Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
    control_rx: &mut mpsc::Receiver<PlaybackControl>,
    sink: Option<AudioErrorSink>,
) {
    while let Some(msg) = control_rx.blocking_recv() {
        match msg {
            PlaybackControl::Stop => break,
            PlaybackControl::SwitchDevice(new_device) => {
                let reporter = FailureReporter::new(
                    AudioErrorKind::PlaybackFailed,
                    device_label(&new_device),
                    sink.clone(),
                );
                match build_buffer_output_stream(&new_device, deafened, playback_buffer, &reporter)
                {
                    Some(new_stream) => {
                        stream = new_stream;
                        info!("Playback switched to new output device");
                    }
                    None => {
                        warn!("Output device switch failed; keeping previous device");
                    }
                }
            }
        }
    }

    drop(stream);
}

/// Run PCM playback task — receives pre-decoded f32 samples from the mixer.
///
/// Structurally identical to `run_playback_task` but skips Opus decoding since
/// the audio mixer already produces mixed f32 PCM.
fn run_pcm_playback_task(
    device: Device,
    deafened: Arc<AtomicBool>,
    mut input_rx: mpsc::Receiver<Vec<f32>>,
    control_rx: &mut mpsc::Receiver<PlaybackControl>,
    sink: Option<AudioErrorSink>,
) {
    let playback_buffer = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));

    // Spawn thread to receive PCM frames and push into the ring buffer.
    let playback_buffer_clone = playback_buffer.clone();
    std::thread::spawn(move || {
        // ~200ms at 48 kHz stereo (48000 * 2 channels * 0.2s)
        const MAX_BUFFER_SAMPLES: usize = 19200;

        while let Some(pcm_f32) = input_rx.blocking_recv() {
            if let Ok(mut buffer) = playback_buffer_clone.lock() {
                buffer.extend(pcm_f32);

                // Cap buffer to prevent unbounded growth under sustained backpressure.
                if buffer.len() > MAX_BUFFER_SAMPLES {
                    let excess = buffer.len() - MAX_BUFFER_SAMPLES;
                    buffer.drain(..excess);
                }
            }
        }
    });

    let reporter = FailureReporter::new(
        AudioErrorKind::PlaybackFailed,
        device_label(&device),
        sink.clone(),
    );
    let Some(stream) = build_buffer_output_stream(&device, &deafened, &playback_buffer, &reporter)
    else {
        reporter.report("output stream could not be started");
        return;
    };

    playback_control_loop(stream, &deafened, &playback_buffer, control_rx, sink);
    info!("PCM playback task stopped");
}

/// Open the mic-test stream in a format the device supports (runs on the
/// owner thread); the level is format-independent.
fn build_mic_test_stream(
    device: &Device,
    mic_level: Arc<AtomicU8>,
) -> Result<cpal::Stream, AudioError> {
    let (format, sample_format) = capture_config(device);
    match sample_format {
        SampleFormat::I16 => build_mic_test_stream_as::<i16>(device, format, mic_level),
        SampleFormat::I32 => build_mic_test_stream_as::<i32>(device, format, mic_level),
        SampleFormat::U16 => build_mic_test_stream_as::<u16>(device, format, mic_level),
        _ => build_mic_test_stream_as::<f32>(device, format, mic_level),
    }
}

fn build_mic_test_stream_as<T>(
    device: &Device,
    format: CaptureFormat,
    mic_level: Arc<AtomicU8>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: format.channels,
        sample_rate: format.sample_rate,
        buffer_size: BufferSize::Default,
    };

    let level = mic_level;
    let stream = device
        .build_input_stream(
            &config,
            move |data: &[T], _| {
                if data.is_empty() {
                    return;
                }
                // Calculate RMS level
                let sum: f32 = data
                    .iter()
                    .map(|&s| {
                        let s = f32::from_sample(s);
                        s * s
                    })
                    .sum();
                let rms = (sum / data.len() as f32).sqrt();

                // Convert to 0-100 scale
                level.store((rms * 100.0).min(100.0) as u8, Ordering::Relaxed);
            },
            |err| {
                error!("Mic test stream error: {}", err);
            },
            None,
        )
        .map_err(|e| AudioError::StreamError(e.to_string()))?;

    stream
        .play()
        .map_err(|e| AudioError::StreamError(e.to_string()))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;

    /// Records when the "stream" it stands in for is dropped.
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn capture_build_error_is_returned() {
        let Err(err) = spawn_stream_owner(
            || Err::<(), _>(AudioError::StreamError("device busy".into())),
            None,
        )
        .await
        else {
            panic!("a stream that fails to build must fail start");
        };
        assert!(matches!(err, AudioError::StreamError(ref m) if m == "device busy"));

        // A stream that builds is owned by the task until Stop.
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = dropped.clone();
        let Ok(owner) = spawn_stream_owner(move || Ok(Guard(flag)), None).await else {
            panic!("build succeeds");
        };
        assert!(!dropped.load(Ordering::SeqCst));
        owner.control.send(CaptureControl::Stop).unwrap();
        for _ in 0..100 {
            if dropped.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(dropped.load(Ordering::SeqCst), "stream dropped after Stop");
    }

    fn collecting_sink() -> (AudioErrorSink, Arc<Mutex<Vec<AudioErrorEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let store = events.clone();
        let sink: AudioErrorSink = Arc::new(move |event: &AudioErrorEvent| {
            store.lock().unwrap().push(event.clone());
        });
        (sink, events)
    }

    #[test]
    fn capture_stream_error_emits_audio_error() {
        let (sink, events) = collecting_sink();
        let reporter = FailureReporter::new(AudioErrorKind::CaptureFailed, "USB Mic", Some(sink));
        let mut on_error = stream_error_callback(reporter);

        // A glitch is not a dead microphone.
        on_error(cpal::StreamError::BufferUnderrun);
        assert!(events.lock().unwrap().is_empty());

        // Unplugged: reported once, however often the backend repeats it.
        on_error(cpal::StreamError::DeviceNotAvailable);
        on_error(cpal::StreamError::DeviceNotAvailable);
        on_error(cpal::StreamError::StreamInvalidated);

        let events = events.lock().unwrap();
        assert_eq!(
            *events,
            vec![AudioErrorEvent {
                kind: AudioErrorKind::CaptureFailed,
                device: "USB Mic".into(),
            }]
        );
        assert_eq!(AUDIO_ERROR_EVENT, "voice:audio_error");
        assert_eq!(
            serde_json::to_value(&events[0]).unwrap(),
            serde_json::json!({ "kind": "capture_failed", "device": "USB Mic" })
        );
    }

    #[test]
    fn stalled_capture_is_reported_once() {
        let (sink, events) = collecting_sink();
        let reporter = FailureReporter::new(AudioErrorKind::CaptureFailed, "USB Mic", Some(sink));
        let heartbeat = Heartbeat::new();
        let watchdog = CaptureWatchdog::new(heartbeat.clone(), reporter);
        let start = heartbeat.epoch();

        heartbeat.beat();
        watchdog.check(start + Duration::from_secs(1));
        assert!(events.lock().unwrap().is_empty(), "fresh data is alive");

        // No callbacks for longer than the stall timeout: the mic is gone.
        watchdog.check(start + CAPTURE_STALL_TIMEOUT + Duration::from_secs(1));
        watchdog.check(start + CAPTURE_STALL_TIMEOUT + Duration::from_secs(2));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, AudioErrorKind::CaptureFailed);
    }

    #[test]
    fn playback_error_kind_serializes() {
        assert_eq!(
            serde_json::to_value(AudioErrorKind::PlaybackFailed).unwrap(),
            serde_json::json!("playback_failed")
        );
    }
}
