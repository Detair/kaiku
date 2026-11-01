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

use super::format::{
    negotiate_capture_format, push_chunk_len, CaptureFormat, CaptureFrontEnd, MAX_PUSH_SAMPLES,
};
use super::{
    devices, AudioDevice, AudioDeviceList, AudioError, VadConfig, CHANNELS, FRAME_SIZE,
    RNNOISE_FRAME_SIZE, SAMPLE_RATE,
};

/// Tauri event emitted when an audio stream dies at runtime; payload is an
/// [`AudioErrorEvent`].
pub const AUDIO_ERROR_EVENT: &str = "voice:audio_error";

/// Tauri event withdrawing an earlier [`AUDIO_ERROR_EVENT`]: audio of that
/// `kind` works again (callbacks resumed, or another device was chosen).
/// Same payload; `kind` names the failure that cleared.
pub const AUDIO_RECOVERED_EVENT: &str = "voice:audio_recovered";

/// No capture callback for this long, once callbacks have started, means
/// the microphone is gone (some backends never report an unplugged device,
/// the stream just stops).
pub const CAPTURE_STALL_TIMEOUT: Duration = Duration::from_secs(3);

/// How long a started stream may take to deliver its first callback before
/// it counts as dead. Generous: Bluetooth headsets switching to their
/// headset profile and slow USB devices can take several seconds.
pub const CAPTURE_FIRST_CALLBACK_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the capture owner thread checks for a stall.
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(500);

/// Which audio stream failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioErrorKind {
    CaptureFailed,
    PlaybackFailed,
}

/// Payload of [`AUDIO_ERROR_EVENT`] and [`AUDIO_RECOVERED_EVENT`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AudioErrorEvent {
    pub kind: AudioErrorKind,
    /// Device label as shown in the device list (or the stored selection
    /// when the device is no longer listed).
    pub device: String,
}

/// Whether a stream failed or recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioNotice {
    Failed,
    Recovered,
}

impl AudioNotice {
    /// Tauri event name for this notice.
    pub const fn event_name(self) -> &'static str {
        match self {
            Self::Failed => AUDIO_ERROR_EVENT,
            Self::Recovered => AUDIO_RECOVERED_EVENT,
        }
    }
}

/// Where audio failures and recoveries are delivered (the Tauri app in
/// production).
pub type AudioErrorSink = Arc<dyn Fn(AudioNotice, &AudioErrorEvent) + Send + Sync>;

/// Tracks one stream's failure: reported at most once while failed (from
/// whichever path notices first: backend error callback or stall
/// watchdog), and withdrawn when the stream works again.
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
            sink(AudioNotice::Failed, &self.event);
        }
    }

    /// Withdraw a reported failure (no-op if none is reported).
    fn recover(&self) {
        if !self.reported.swap(false, Ordering::AcqRel) {
            return;
        }
        info!(
            kind = ?self.event.kind,
            device = %self.event.device,
            "Audio stream works again"
        );
        if let Some(sink) = &self.sink {
            sink(AudioNotice::Recovered, &self.event);
        }
    }

    fn is_failed(&self) -> bool {
        self.reported.load(Ordering::Acquire)
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

/// Marks "no callback yet" in [`Heartbeat::last_ms`].
const NO_BEAT: u64 = u64::MAX;

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
            last_ms: Arc::new(AtomicU64::new(NO_BEAT)),
        }
    }

    #[cfg(test)]
    const fn epoch(&self) -> Instant {
        self.epoch
    }

    /// Called at the top of every capture callback (one clock read + store).
    fn beat(&self) {
        self.beat_at(Instant::now());
    }

    fn beat_at(&self, now: Instant) {
        let ms = now.saturating_duration_since(self.epoch).as_millis() as u64;
        self.last_ms.store(ms.min(NO_BEAT - 1), Ordering::Relaxed);
    }

    /// Time of the last callback, `None` before the first one.
    fn last(&self) -> Option<Instant> {
        match self.last_ms.load(Ordering::Relaxed) {
            NO_BEAT => None,
            ms => Some(self.epoch + Duration::from_millis(ms)),
        }
    }
}

/// Reports a capture stream whose callbacks never started or stopped
/// arriving, and withdraws the report once callbacks flow again.
///
/// Armed when the stream is running (after `play()`), so the time a device
/// needs to open does not count. Until the first callback the generous
/// [`CAPTURE_FIRST_CALLBACK_TIMEOUT`] applies, afterwards
/// [`CAPTURE_STALL_TIMEOUT`].
struct CaptureWatchdog {
    heartbeat: Heartbeat,
    reporter: FailureReporter,
    armed_at: Instant,
    /// When this watchdog first saw the stream as failed (by itself or via
    /// the backend error callback); callbacks after it mean recovery.
    failed_since: Option<Instant>,
}

impl CaptureWatchdog {
    const fn new(heartbeat: Heartbeat, reporter: FailureReporter, armed_at: Instant) -> Self {
        Self {
            heartbeat,
            reporter,
            armed_at,
            failed_since: None,
        }
    }

    fn check(&mut self, now: Instant) {
        let last = self.heartbeat.last();

        if self.reporter.is_failed() {
            let since = *self.failed_since.get_or_insert(now);
            if last.is_some_and(|beat| beat > since) {
                self.failed_since = None;
                self.reporter.recover();
            }
            return;
        }
        self.failed_since = None;

        let (reference, timeout, reason) = match last {
            Some(beat) => (beat, CAPTURE_STALL_TIMEOUT, "no audio from the microphone"),
            None => (
                self.armed_at,
                CAPTURE_FIRST_CALLBACK_TIMEOUT,
                "the microphone never delivered audio",
            ),
        };
        if now.saturating_duration_since(reference) > timeout {
            self.reporter.report(reason);
            self.failed_since = Some(now);
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
/// returned as an error instead of leaving a silent call behind. With
/// `watchdog`, a [`CaptureWatchdog`] is armed once the stream is running and
/// checked every [`WATCHDOG_INTERVAL`].
async fn spawn_stream_owner<S, B>(
    build: B,
    watchdog: Option<(Heartbeat, FailureReporter)>,
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
            // Armed only now: opening the device does not eat the deadline.
            let mut watchdog = watchdog.map(|(heartbeat, reporter)| {
                CaptureWatchdog::new(heartbeat, reporter, Instant::now())
            });
            loop {
                match control_rx.recv_timeout(WATCHDOG_INTERVAL) {
                    Ok(CaptureControl::Stop) | Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                        break
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {
                        if let Some(watchdog) = watchdog.as_mut() {
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
    /// Failure state of this stream (unplugged, stalled; cleared on recovery).
    reporter: FailureReporter,
    /// Opened on the system default because the selected mic was busy.
    fell_back: bool,
}

/// A capture stream as seen by the switching logic.
trait CaptureStream {
    /// Start sending frames.
    fn activate(&self);
    /// Not currently reported as failed.
    fn is_healthy(&self) -> bool;
    /// Stop and release the device.
    fn stop(self) -> impl std::future::Future<Output = ()> + Send;
}

impl CaptureStream for CaptureTask {
    fn activate(&self) {
        self.active.store(true, Ordering::SeqCst);
    }

    fn is_healthy(&self) -> bool {
        !self.reporter.is_failed()
    }

    async fn stop(self) {
        self.active.store(false, Ordering::SeqCst);
        self.owner.stop().await;
    }
}

/// Opens capture streams (built but inactive) for [`switch_capture`].
trait CaptureFactory {
    type Task: CaptureStream + Send;
    fn spawn(
        &self,
        device: Option<&str>,
    ) -> impl std::future::Future<Output = Result<Self::Task, AudioError>> + Send;
    /// Tell the user no microphone is capturing any more.
    fn report_failure(&self, device: Option<&str>);
    /// Withdraw an earlier "microphone stopped working" for `device`.
    fn report_recovered(&self, device: Option<&str>);
}

/// Move capture to `target` (make-before-break).
///
/// Returns `Ok(true)` when a new stream now captures, `Ok(false)` when the
/// healthy current stream already uses `target`. On error the previous
/// microphone keeps (or is restored to) capturing and `current` is
/// unchanged; if even that fails, the loss is reported via the factory.
async fn switch_capture<F: CaptureFactory + Sync>(
    factory: &F,
    capture: &mut Option<F::Task>,
    current: &mut Option<String>,
    target: Option<String>,
) -> Result<bool, AudioError> {
    // Re-applying the device that is already capturing fine (e.g. the saved
    // device on join) is a no-op; a failed stream is rebuilt.
    let old_healthy = capture.as_ref().is_some_and(CaptureStream::is_healthy);
    if old_healthy && target == *current {
        return Ok(false);
    }

    let new_task = match factory.spawn(target.as_deref()).await {
        Ok(task) => task,
        Err(first) if capture.is_some() => {
            // Some devices allow only one open stream (re-selecting the
            // same microphone): release the old stream and retry once.
            debug!("Capture switch failed with the old stream open ({first}); retrying");
            if let Some(old) = capture.take() {
                old.stop().await;
            }
            match factory.spawn(target.as_deref()).await {
                Ok(task) => task,
                Err(e) => {
                    warn!("Capture switch failed: {e}; restoring the previous microphone");
                    match factory.spawn(current.as_deref()).await {
                        Ok(task) => {
                            task.activate();
                            *capture = Some(task);
                            if !old_healthy {
                                // The failed mic works again after the rebuild.
                                factory.report_recovered(current.as_deref());
                            }
                        }
                        Err(restore) if current.is_some() => {
                            // E.g. the saved mic was busy at join and capture
                            // runs on the default: restoring the saved one
                            // fails again, the default still works.
                            warn!(
                                "Previous microphone could not be restored ({restore}); using \
                                 the system default"
                            );
                            match factory.spawn(None).await {
                                Ok(task) => {
                                    task.activate();
                                    *capture = Some(task);
                                    if !old_healthy {
                                        factory.report_recovered(current.as_deref());
                                    }
                                    *current = None;
                                    // Not the old state any more: say so.
                                    return Err(AudioError::UsingSystemDefault(e.to_string()));
                                }
                                Err(e) => {
                                    error!("System default microphone could not be opened: {e}");
                                    factory.report_failure(current.as_deref());
                                }
                            }
                        }
                        Err(restore) => {
                            error!("Previous microphone could not be restored: {restore}");
                            factory.report_failure(current.as_deref());
                        }
                    }
                    return Err(e);
                }
            }
        }
        Err(e) => return Err(e),
    };

    if let Some(old) = capture.take() {
        old.stop().await;
    }
    new_task.activate();
    *capture = Some(new_task);
    *current = target;
    Ok(true)
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

    /// Selected input device id
    input_device_id: Option<String>,

    /// Selected output device id
    output_device_id: Option<String>,

    /// Label of the output device actually playing (may be the default
    /// when the selected one was busy).
    active_output: ActiveLabel,

    /// Generation of the output selection, renewed by every
    /// `set_output_device` and playback start.
    output_generation: u64,

    /// Last generation handed out; only grows (a failed switch restores
    /// `output_generation`, but new generations stay unique).
    generation_counter: u64,

    /// Generation for which a playback task fell back to the system default
    /// because the selected speaker was busy (0 = none). Written by the
    /// task, also after `start_*playback` stopped waiting; it only applies
    /// while that generation is current, so a late fallback of an old task
    /// cannot clear a newer selection. See [`Self::sync_output_fallback`].
    output_fallback_mark: Arc<AtomicU64>,

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
    /// The reply says whether the new device could be opened.
    SwitchDevice(
        Box<LabelledDevice>,
        std::sync::mpsc::SyncSender<Result<(), AudioError>>,
    ),
}

/// Device name as listed by `enumerate_devices`.
fn device_label(device: &Device) -> String {
    device
        .description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| "default".to_string())
}

/// Pick the device for `name` (`None` = system default).
///
/// `named` is the device found under `name`, `default` the system default.
/// With `fallback`, a saved device that is gone (headset unplugged between
/// calls) falls back to the system default instead of failing the call;
/// an explicit switch passes `false` so the user learns the choice failed.
fn resolve_device<D>(
    name: Option<&str>,
    named: Option<D>,
    default: impl FnOnce() -> Option<D>,
    fallback: bool,
    is_input: bool,
) -> Result<D, AudioError> {
    let no_default = || {
        if is_input {
            AudioError::NoInputDevice
        } else {
            AudioError::NoOutputDevice
        }
    };
    match (name, named) {
        (Some(_), Some(device)) => Ok(device),
        (Some(name), None) if fallback => {
            warn!("Audio device {name:?} not found; using the system default");
            default().ok_or_else(no_default)
        }
        (Some(name), None) => Err(AudioError::DeviceNotFound(name.to_string())),
        (None, _) => default().ok_or_else(no_default),
    }
}

/// A device CPAL reports, with what the device list needs.
struct ListedDevice {
    device: Device,
    /// Stable unique id (CPAL device id, e.g. `alsa:sysdefault:CARD=USB`).
    id: String,
    label: String,
    /// Name older client versions stored as the selection (the description).
    legacy_name: String,
    direction: cpal::DeviceDirection,
    /// Second ALSA description line (e.g. "IEC958 (S/PDIF) Digital Audio
    /// Output"), if any.
    detail: Option<String>,
    /// Offered by default; `false` for ALSA PCMs only shown with "Show all
    /// ALSA devices". Always `true` off ALSA.
    curated: bool,
    /// Stands for the system default.
    system_default: bool,
}

/// Whether `host` is ALSA (Linux/BSD), whose raw PCM list needs curating.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
))]
fn is_alsa(host: &Host) -> bool {
    host.id() == cpal::HostId::Alsa
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd"
)))]
const fn is_alsa(_host: &Host) -> bool {
    false
}

/// CPAL device id as a string; the description name if the backend has none.
fn device_id_string(device: &Device) -> String {
    device
        .id()
        .map_or_else(|_| device_label(device), |id| id.to_string())
}

/// Every device CPAL reports for one direction (`is_input` = capture),
/// labelled by its name.
fn raw_devices(host: &Host, is_input: bool) -> Result<Vec<ListedDevice>, AudioError> {
    Ok(if is_input {
        host.input_devices()
    } else {
        host.output_devices()
    }
    .map_err(|e| AudioError::ConfigError(e.to_string()))?
    .filter_map(|device| {
        let desc = device.description().ok()?;
        let name = desc.name().to_string();
        Some(ListedDevice {
            id: device_id_string(&device),
            label: name.clone(),
            legacy_name: name,
            direction: desc.direction(),
            detail: desc.extended().get(1).cloned(),
            curated: true,
            system_default: false,
            device,
        })
    })
    .collect())
}

/// Every device for one direction: the offered ones first, then (ALSA only)
/// the PCMs the curated list hides (`curated: false`), labelled with their
/// PCM name. Off ALSA every device is offered under its name.
fn list_devices(host: &Host, is_input: bool) -> Result<Vec<ListedDevice>, AudioError> {
    let mut raw = raw_devices(host, is_input)?;
    if !is_alsa(host) {
        let default = if is_input {
            host.default_input_device()
        } else {
            host.default_output_device()
        };
        let default_id = default.as_ref().map(device_id_string);
        for device in &mut raw {
            device.system_default = default_id.as_deref() == Some(device.id.as_str());
        }
        return Ok(raw);
    }

    let pcms: Vec<devices::AlsaPcm> = raw
        .iter()
        .map(|d| devices::AlsaPcm {
            // The ALSA device id is `alsa:<PCM name>`.
            pcm_id: d.id.strip_prefix("alsa:").unwrap_or(&d.id).to_string(),
            description: d.legacy_name.clone(),
            direction: d.direction,
        })
        .collect();
    let mut slots: Vec<Option<ListedDevice>> = raw.into_iter().map(Some).collect();
    let mut listed = Vec::with_capacity(slots.len());
    for curated in devices::curate_alsa_pcms(&pcms, is_input) {
        let Some(index) = pcms.iter().position(|p| p.pcm_id == curated.pcm_id) else {
            continue;
        };
        if let Some(mut device) = slots[index].take() {
            device.label = curated.label;
            device.system_default = curated.system_default;
            listed.push(device);
        }
    }
    for (pcm, slot) in pcms.iter().zip(slots) {
        if let Some(mut device) = slot {
            device.label =
                devices::raw_pcm_label(&pcm.pcm_id, &device.legacy_name, device.detail.as_deref());
            device.curated = false;
            listed.push(device);
        }
    }
    Ok(listed)
}

/// Resolve a stored selection (id, or a description stored by older
/// versions) to the device and its label; see [`devices::find_selection`].
fn lookup(
    host: &Host,
    selection: &str,
    is_input: bool,
) -> Result<Option<ListedDevice>, AudioError> {
    let mut listed = list_devices(host, is_input)?;
    let curated: Vec<usize> = listed
        .iter()
        .enumerate()
        .filter(|(_, d)| d.curated)
        .map(|(i, _)| i)
        .collect();
    let found = devices::find_selection(
        &listed,
        &curated,
        selection,
        |d| d.id.as_str(),
        |d| d.legacy_name.as_str(),
    );
    Ok(found.map(|found| {
        let (devices::Found::Listed(index) | devices::Found::Unlisted(index)) = found;
        let device = listed.swap_remove(index);
        if device.id != selection {
            info!(
                "Audio device {selection:?} resolved by its old name to {:?}",
                device.id
            );
        }
        device
    }))
}

/// Label of the selected device for messages; the selection itself when the
/// device is gone (e.g. unplugged).
fn display_name(host: &Host, selection: &str, is_input: bool) -> String {
    lookup(host, selection, is_input)
        .ok()
        .flatten()
        .map_or_else(|| selection.to_string(), |d| d.label)
}

/// Find a device by id (`None` = system default); a description stored by
/// older client versions is still accepted. See [`resolve_device`] for
/// `fallback`.
fn find_device(
    host: &Host,
    device_id: Option<&str>,
    is_input: bool,
    fallback: bool,
) -> Result<Device, AudioError> {
    let named = match device_id {
        Some(selection) => lookup(host, selection, is_input)?.map(|d| d.device),
        None => None,
    };
    resolve_device(
        device_id,
        named,
        || default_device(host, is_input),
        fallback,
        is_input,
    )
}

/// A device with the label it has in the device list.
struct LabelledDevice {
    device: Device,
    label: String,
}

impl LabelledDevice {
    fn new(host: &Host, device: Device, is_input: bool) -> Self {
        let label = display_name(host, &device_id_string(&device), is_input);
        Self { device, label }
    }
}

/// How long `start_*playback` waits for the playback stream to open.
const PLAYBACK_READY_TIMEOUT: Duration = Duration::from_secs(10);

/// How opening the playback stream went (sent once by the playback task).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybackStart {
    Opened,
    /// Opened on the system default because the selected speaker was busy.
    FellBack,
    Failed,
}

/// Where a playback task records that it fell back, tagged with the
/// output selection generation it was started for.
struct FallbackMark {
    mark: Arc<AtomicU64>,
    generation: u64,
}

impl FallbackMark {
    /// `fetch_max`: generations only grow, so a stale task can never
    /// overwrite the pending mark of a newer one.
    fn set(&self) {
        self.mark.fetch_max(self.generation, Ordering::SeqCst);
    }
}

/// What a playback task needs to open its stream.
struct PlaybackOpen {
    device: LabelledDevice,
    /// System default to use when `device` is busy.
    fallback: Option<LabelledDevice>,
    active: ActiveLabel,
    /// Marked when playback fell back to `fallback` (outlives `ready`).
    fell_back: FallbackMark,
    /// Told how opening went, once.
    ready: tokio::sync::oneshot::Sender<PlaybackStart>,
}

/// Label of the output device that is actually playing (`None` while
/// nothing plays); shared with the playback task.
type ActiveLabel = Arc<std::sync::Mutex<Option<String>>>;

fn set_active(active: &ActiveLabel, label: Option<String>) {
    if let Ok(mut slot) = active.lock() {
        *slot = label;
    }
}

/// The system default for one direction with its label, from one device
/// enumeration: the list entry that stands for it (on ALSA that may be the
/// sound server PCM when `default` is missing), else what the host reports.
fn default_entry(host: &Host, is_input: bool) -> Option<LabelledDevice> {
    if is_alsa(host) {
        let listed = list_devices(host, is_input).ok().and_then(|listed| {
            listed
                .into_iter()
                .find(|d| d.system_default)
                .map(|d| LabelledDevice {
                    device: d.device,
                    label: d.label,
                })
        });
        if listed.is_some() {
            return listed;
        }
    }
    let device = if is_input {
        host.default_input_device()
    } else {
        host.default_output_device()
    }?;
    // Off ALSA the list labels devices by this same name.
    let label = device_label(&device);
    Some(LabelledDevice { device, label })
}

/// The system default device for one direction; see [`default_entry`].
fn default_device(host: &Host, is_input: bool) -> Option<Device> {
    default_entry(host, is_input).map(|d| d.device)
}

/// Whether `device` is the system default (falling back to it again
/// would open the same device).
fn is_system_default(host: &Host, device: &Device, is_input: bool) -> bool {
    default_device(host, is_input).is_some_and(|d| device_id_string(&d) == device_id_string(device))
}

/// Label for a selection in messages; `None` = the system default, under
/// the same label the device list and stream errors use.
fn selection_label(host: &Host, selection: Option<&str>, is_input: bool) -> String {
    match selection {
        Some(id) => display_name(host, id, is_input),
        None => {
            default_entry(host, is_input).map_or_else(|| "System default".to_string(), |d| d.label)
        }
    }
}

/// Map a failed stream build. A device that was just found but cannot be
/// opened is busy: on ALSA, CPAL reports `EBUSY` (another program or the
/// sound server holds the card exclusively) as `DeviceNotAvailable`.
fn map_build_error(err: &cpal::BuildStreamError) -> AudioError {
    match err {
        cpal::BuildStreamError::DeviceNotAvailable => AudioError::DeviceInUse,
        other => AudioError::StreamError(other.to_string()),
    }
}

/// Map a failed stream start; see [`map_build_error`].
fn map_play_error(err: &cpal::PlayStreamError) -> AudioError {
    match err {
        cpal::PlayStreamError::DeviceNotAvailable => AudioError::DeviceInUse,
        other @ cpal::PlayStreamError::BackendSpecific { .. } => {
            AudioError::StreamError(other.to_string())
        }
    }
}

/// Sink that emits [`AudioNotice`]s as Tauri events.
fn error_sink_for(app: Option<tauri::AppHandle>) -> Option<AudioErrorSink> {
    let app = app?;
    Some(Arc::new(
        move |notice: AudioNotice, event: &AudioErrorEvent| {
            if let Err(e) = app.emit(notice.event_name(), event) {
                warn!("Failed to emit {}: {e}", notice.event_name());
            }
        },
    ))
}

/// Opens capture streams for one call (production [`CaptureFactory`]).
struct CaptureSpawner {
    host: Arc<Host>,
    muted: Arc<AtomicBool>,
    vad_config: VadConfig,
    app_handle: Option<tauri::AppHandle>,
    output_tx: mpsc::Sender<Vec<u8>>,
    /// Fall back to the default microphone when the named one is gone
    /// (call start), instead of failing (explicit switch).
    fallback_to_default: bool,
}

impl CaptureSpawner {
    /// Emit a capture/playback notice not tied to a running stream.
    fn notify(&self, notice: AudioNotice, kind: AudioErrorKind, device: Option<&str>) {
        if let Some(sink) = error_sink_for(self.app_handle.clone()) {
            sink(
                notice,
                &AudioErrorEvent {
                    kind,
                    device: selection_label(&self.host, device, true),
                },
            );
        }
    }
}

impl CaptureFactory for CaptureSpawner {
    type Task = CaptureTask;

    /// Build (but do not activate) a capture stream on `device_id`. At call
    /// start (`fallback_to_default`) a saved microphone that is gone or busy
    /// falls back to the system default; an explicit switch reports it.
    async fn spawn(&self, device_id: Option<&str>) -> Result<CaptureTask, AudioError> {
        let device = find_device(&self.host, device_id, true, self.fallback_to_default)?;
        // A busy default (selected by id, or a gone device that already
        // resolved to it) is not retried on the same device.
        let can_fall_back =
            self.fallback_to_default && !is_system_default(&self.host, &device, true);
        match self.spawn_on(device).await {
            Err(AudioError::DeviceInUse) if can_fall_back => {
                warn!("Audio device {device_id:?} is busy; using the system default");
                let default = find_device(&self.host, None, true, true)?;
                let mut task = self.spawn_on(default).await?;
                task.fell_back = true;
                Ok(task)
            }
            other => other,
        }
    }

    fn report_failure(&self, device: Option<&str>) {
        error!("No microphone could be restarted");
        self.notify(AudioNotice::Failed, AudioErrorKind::CaptureFailed, device);
    }

    fn report_recovered(&self, device: Option<&str>) {
        self.notify(
            AudioNotice::Recovered,
            AudioErrorKind::CaptureFailed,
            device,
        );
    }
}

impl CaptureSpawner {
    /// Build (but do not activate) a capture stream on `device`.
    async fn spawn_on(&self, device: Device) -> Result<CaptureTask, AudioError> {
        let LabelledDevice { device, label } = LabelledDevice::new(&self.host, device, true);
        let reporter = FailureReporter::new(
            AudioErrorKind::CaptureFailed,
            label,
            error_sink_for(self.app_handle.clone()),
        );
        let heartbeat = Heartbeat::new();
        let active = Arc::new(AtomicBool::new(false));
        let ctx = CaptureContext {
            muted: self.muted.clone(),
            output_tx: self.output_tx.clone(),
            vad_config: self.vad_config.clone(),
            app_handle: self.app_handle.clone(),
            active: active.clone(),
            heartbeat: heartbeat.clone(),
            reporter: reporter.clone(),
        };
        let owner = spawn_stream_owner(
            move || build_capture_stream(&device, ctx),
            Some((heartbeat, reporter.clone())),
        )
        .await?;
        Ok(CaptureTask {
            owner,
            active,
            reporter,
            fell_back: false,
        })
    }
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
            input_device_id: None,
            output_device_id: None,
            active_output: ActiveLabel::default(),
            output_generation: 0,
            generation_counter: 0,
            output_fallback_mark: Arc::new(AtomicU64::new(0)),
            vad_config: VadConfig::new(),
            app_handle: None,
        })
    }

    /// Enumerate the audio devices to offer (curated on Linux, see
    /// [`list_devices`]). `device_id` is the stable id to pass back to
    /// `set_input_device`/`set_output_device`/`start_mic_test`.
    pub fn enumerate_devices(&self) -> Result<AudioDeviceList, AudioError> {
        let to_frontend = |is_input: bool| -> Result<Vec<AudioDevice>, AudioError> {
            Ok(list_devices(&self.host, is_input)?
                .into_iter()
                .map(|d| AudioDevice {
                    is_default: d.system_default,
                    curated: d.curated,
                    device_id: d.id,
                    label: d.label,
                    legacy_name: d.legacy_name,
                })
                .collect())
        };

        Ok(AudioDeviceList {
            inputs: to_frontend(true)?,
            outputs: to_frontend(false)?,
        })
    }

    /// Set the input device by id (`None` = system default).
    ///
    /// Outside a call this only stores the choice. During a call the capture
    /// stream moves to the new device (see [`Self::switch_capture_device`]);
    /// if that fails, the previous microphone keeps running and the stored
    /// choice is unchanged. Exception: when the previous microphone cannot be
    /// reopened either, capture moves to the system default, the stored
    /// choice becomes `None` and [`AudioError::UsingSystemDefault`] is
    /// returned.
    pub async fn set_input_device(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        if self.capture_output_tx.is_none() {
            self.input_device_id = device_id;
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

    async fn switch_capture_to(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        let Some(output_tx) = self.capture_output_tx.clone() else {
            self.input_device_id = device_id;
            return Ok(());
        };
        let spawner = self.capture_spawner(output_tx, false);
        let switched = switch_capture(
            &spawner,
            &mut self.capture,
            &mut self.input_device_id,
            device_id,
        )
        .await?;
        if switched {
            // The old stream may have left the speaking indicator on, and a
            // "Microphone stopped working" notice no longer applies.
            if let Some(app) = &self.app_handle {
                let _ = app.emit("voice:speaking", false);
            }
            spawner.notify(
                AudioNotice::Recovered,
                AudioErrorKind::CaptureFailed,
                self.input_device_id.as_deref(),
            );
            info!("Capture switched to {:?}", self.input_device_id);
        }
        Ok(())
    }

    /// Set the output device by id (`None` = system default).
    ///
    /// Stores the name for future playback starts AND live-switches any
    /// running playback task: the task rebuilds its CPAL stream on the new
    /// device while keeping the same sample buffer, so audio moves to the
    /// new speaker mid-call without dropping frames.
    ///
    /// Errors if a running playback cannot move to the device (not found or
    /// cannot be opened); the previous choice is then kept, matching the
    /// speaker that is actually playing.
    pub async fn set_output_device(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        // A late fallback means the default plays now: a failed switch must
        // restore that, not the busy speaker.
        self.sync_output_fallback();
        let previous_generation = self.output_generation;
        let previous = std::mem::replace(&mut self.output_device_id, device_id);
        self.output_generation = self.next_generation();
        let result = self.switch_running_playback().await;
        if result.is_err() {
            // Back to the previous selection, including a fallback that its
            // playback task may still report.
            self.output_device_id = previous;
            self.output_generation = previous_generation;
        }
        result
    }

    /// Move a running playback stream to `output_device_id`.
    async fn switch_running_playback(&self) -> Result<(), AudioError> {
        if let Some(control) = &self.playback_control {
            let device = self.get_device(self.output_device_id.as_deref(), false)?;
            let device = LabelledDevice::new(&self.host, device, false);
            let label = device.label.clone();
            let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
            if control
                .send(PlaybackControl::SwitchDevice(Box::new(device), reply_tx))
                .await
                .is_err()
            {
                debug!("Playback task not running; output device applies on next start");
                return Ok(());
            }
            // The playback thread answers right after trying to open it.
            let opened = tokio::task::spawn_blocking(move || reply_rx.recv())
                .await
                .ok()
                .and_then(Result::ok);
            // Audio stays on the previous speaker; tell the caller.
            match opened {
                Some(Err(AudioError::DeviceInUse)) => return Err(AudioError::DeviceInUse),
                Some(Err(e)) => {
                    return Err(AudioError::StreamError(format!(
                        "could not open output device {label}: {e}"
                    )))
                }
                Some(Ok(())) | None => {}
            }
        }
        Ok(())
    }

    /// Get device by id
    fn get_device(&self, device_id: Option<&str>, is_input: bool) -> Result<Device, AudioError> {
        find_device(&self.host, device_id, is_input, false)
    }

    /// Like `get_device`, but a saved device that is gone falls back to the
    /// system default (stream starts at call join).
    fn get_device_or_default(
        &self,
        device_id: Option<&str>,
        is_input: bool,
    ) -> Result<Device, AudioError> {
        find_device(&self.host, device_id, is_input, true)
    }

    /// System default speaker to fall back to when `device` is busy at call
    /// start (`None` when `device` already is the default).
    fn playback_fallback(&self, device: &Device) -> Option<LabelledDevice> {
        let default = default_entry(&self.host, false)?;
        (device_id_string(&default.device) != device_id_string(device)).then_some(default)
    }

    /// Wait until the playback task tried to open its stream, so the active
    /// label is set before `start_*playback` returns. Playback that fell back
    /// to the default (selected speaker busy) clears the selected id, like
    /// capture: re-selecting the busy speaker is then tried, and a failed
    /// switch restores the default.
    ///
    /// A device that hangs while opening must not block the call: after
    /// [`PLAYBACK_READY_TIMEOUT`] this counts as a failed open (the task
    /// keeps running and reports on its own if it still fails).
    ///
    /// A fallback that happens after the timeout is still applied: the task
    /// marks it in `output_fallback_mark`, which every later use of the
    /// selection reads (unless the selection changed meanwhile).
    async fn await_playback_ready(&mut self, ready: tokio::sync::oneshot::Receiver<PlaybackStart>) {
        if tokio::time::timeout(PLAYBACK_READY_TIMEOUT, ready)
            .await
            .is_err()
        {
            warn!("Output stream did not open within {PLAYBACK_READY_TIMEOUT:?}; treating it as failed");
        }
        self.sync_output_fallback();
    }

    /// Apply a playback fallback to the stored selection: the busy speaker
    /// is no longer selected, the system default is.
    /// Only a fallback for the current generation applies. A pending mark
    /// is never newer than the current generation (each start makes its
    /// generation current), so resetting the matching mark to 0 loses
    /// nothing; a stale task's later `fetch_max` lands below the current
    /// generation and is ignored.
    fn sync_output_fallback(&mut self) {
        let current = self.output_generation;
        if current != 0
            && self
                .output_fallback_mark
                .compare_exchange(current, 0, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            self.output_device_id = None;
        }
    }

    /// The selected output id as it currently applies (`None` after a
    /// fallback for the current selection that has not been synced yet).
    fn effective_output_id(&self) -> Option<&str> {
        let current = self.output_generation;
        if current != 0 && self.output_fallback_mark.load(Ordering::SeqCst) == current {
            None
        } else {
            self.output_device_id.as_deref()
        }
    }

    const fn next_generation(&mut self) -> u64 {
        self.generation_counter += 1;
        self.generation_counter
    }

    /// Start a new output generation for a playback start; the task marks
    /// a fallback with it.
    fn begin_playback_generation(&mut self) -> FallbackMark {
        self.sync_output_fallback();
        self.output_generation = self.next_generation();
        FallbackMark {
            mark: self.output_fallback_mark.clone(),
            generation: self.output_generation,
        }
    }

    /// Delivers audio failures/recoveries to the webview as Tauri events.
    fn error_sink(&self) -> Option<AudioErrorSink> {
        error_sink_for(self.app_handle.clone())
    }

    /// What is needed to open capture streams feeding `output_tx`.
    fn capture_spawner(
        &self,
        output_tx: mpsc::Sender<Vec<u8>>,
        fallback_to_default: bool,
    ) -> CaptureSpawner {
        CaptureSpawner {
            host: self.host.clone(),
            muted: self.muted.clone(),
            vad_config: self.vad_config.clone(),
            app_handle: self.app_handle.clone(),
            output_tx,
            fallback_to_default,
        }
    }

    /// Selected output device id (`None` = system default).
    pub fn output_device_id(&self) -> Option<&str> {
        self.effective_output_id()
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
            .capture_spawner(output_tx, true)
            .spawn(self.input_device_id.as_deref())
            .await?;
        if task.fell_back {
            // Capture runs on the default: choosing the saved mic again must
            // try it (not be a no-op), and a failed switch restores the
            // default, not the busy mic.
            self.input_device_id = None;
        }
        task.activate();
        self.capture = Some(task);

        info!("Audio capture started");
        Ok(())
    }

    /// Selected input device id (`None` = system default).
    pub fn input_device_id(&self) -> Option<&str> {
        self.input_device_id.as_deref()
    }

    /// Label of the input/output device for messages: the one actually in
    /// use (which is the system default when the selected one was busy),
    /// else the selected one (the stored id when the device is gone).
    pub fn active_device_label(&self, is_input: bool) -> String {
        let in_use = if is_input {
            self.capture
                .as_ref()
                .map(|task| task.reporter.event.device.clone())
        } else {
            self.active_output
                .lock()
                .ok()
                .and_then(|label| label.clone())
        };
        if let Some(label) = in_use {
            return label;
        }
        let selected = if is_input {
            self.input_device_id.as_deref()
        } else {
            self.effective_output_id()
        };
        selection_label(&self.host, selected, is_input)
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

        let fell_back = self.begin_playback_generation();
        let device = self.get_device_or_default(self.output_device_id.as_deref(), false)?;
        let fallback = self.playback_fallback(&device);
        let device = LabelledDevice::new(&self.host, device, false);
        let active = self.active_output.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let deafened = self.deafened.clone();
        let sink = self.error_sink();

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<PlaybackControl>(1);
        self.playback_control = Some(control_tx);

        // Spawn playback task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_playback_task(
                PlaybackOpen {
                    device,
                    fallback,
                    active,
                    fell_back,
                    ready: ready_tx,
                },
                deafened,
                input_rx,
                &mut control_rx,
                sink,
            );
        });

        self.await_playback_ready(ready_rx).await;
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

        let fell_back = self.begin_playback_generation();
        let device = self.get_device_or_default(self.output_device_id.as_deref(), false)?;
        let fallback = self.playback_fallback(&device);
        let device = LabelledDevice::new(&self.host, device, false);
        let active = self.active_output.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let deafened = self.deafened.clone();
        let sink = self.error_sink();

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<PlaybackControl>(1);
        self.playback_control = Some(control_tx);

        // Spawn PCM playback task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_pcm_playback_task(
                PlaybackOpen {
                    device,
                    fallback,
                    active,
                    fell_back,
                    ready: ready_tx,
                },
                deafened,
                input_rx,
                &mut control_rx,
                sink,
            );
        });

        self.await_playback_ready(ready_rx).await;
        info!("PCM playback started (mixer output)");
        Ok(())
    }

    /// Stop audio playback
    pub async fn stop_playback(&mut self) {
        if let Some(control) = self.playback_control.take() {
            let _ = control.send(PlaybackControl::Stop).await;
            set_active(&self.active_output, None);
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
    // Device samples as f32; filled in chunks of at most its capacity, so it
    // never reallocates in the callback.
    let mut converted: Vec<f32> = Vec::with_capacity(MAX_PUSH_SAMPLES);
    let channels = format.channels;

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
                convert_in_chunks(data, channels, &mut converted, |chunk| {
                    front_end.push(chunk, |frame| processor.process_frame(frame));
                });
            },
            stream_error_callback(reporter),
            None,
        )
        .map_err(|e| map_build_error(&e))?;

    stream.play().map_err(|e| map_play_error(&e))?;
    Ok(stream)
}

/// Convert device samples to f32 in whole-frame chunks of at most
/// [`MAX_PUSH_SAMPLES`], reusing `converted` (sized to that) so the audio
/// callback never reallocates, however large the backend's period is.
fn convert_in_chunks<T>(
    data: &[T],
    channels: u16,
    converted: &mut Vec<f32>,
    mut sink: impl FnMut(&[f32]),
) where
    T: SizedSample,
    f32: FromSample<T>,
{
    for chunk in data.chunks(push_chunk_len(channels)) {
        converted.clear();
        converted.extend(chunk.iter().map(|&s| f32::from_sample(s)));
        sink(converted);
    }
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
    open: PlaybackOpen,
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

    let PlaybackOpen {
        device,
        fallback,
        active,
        fell_back,
        ready,
    } = open;
    let (stream, reporter, start) = open_playback_stream(
        device,
        fallback,
        &active,
        &deafened,
        &playback_buffer,
        sink.as_ref(),
    );
    if start == PlaybackStart::FellBack {
        fell_back.set();
    }
    let _ = ready.send(start);

    playback_control_loop(
        stream,
        reporter,
        &active,
        &deafened,
        &playback_buffer,
        control_rx,
        sink,
    );
    info!("Playback task stopped");
}

/// Open playback on `device`, or on `fallback` (the system default) when
/// `device` is busy. A stream that cannot start is reported and `None` is
/// returned: the task stays alive so choosing another speaker starts it.
fn open_playback_stream(
    device: LabelledDevice,
    fallback: Option<LabelledDevice>,
    active: &ActiveLabel,
    deafened: &Arc<AtomicBool>,
    playback_buffer: &Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
    sink: Option<&AudioErrorSink>,
) -> (Option<cpal::Stream>, FailureReporter, PlaybackStart) {
    let reporter_for = |device: &LabelledDevice| {
        FailureReporter::new(
            AudioErrorKind::PlaybackFailed,
            device.label.clone(),
            sink.cloned(),
        )
    };
    let reporter = reporter_for(&device);
    let result =
        match build_buffer_output_stream(&device.device, deafened, playback_buffer, &reporter) {
            Err(AudioError::DeviceInUse) if fallback.is_some() => {
                let fallback = fallback.unwrap_or(device);
                warn!(
                    "Output device {} is busy; using the system default",
                    reporter.event.device
                );
                let reporter = reporter_for(&fallback);
                let result = build_buffer_output_stream(
                    &fallback.device,
                    deafened,
                    playback_buffer,
                    &reporter,
                );
                (result, reporter, PlaybackStart::FellBack)
            }
            result => (result, reporter, PlaybackStart::Opened),
        };
    match result {
        (Ok(stream), reporter, start) => {
            set_active(active, Some(reporter.event.device.clone()));
            (Some(stream), reporter, start)
        }
        (Err(_), reporter, _) => {
            set_active(active, None);
            reporter.report("output stream could not be started");
            (None, reporter, PlaybackStart::Failed)
        }
    }
}

/// Build and start a CPAL output stream that drains the shared sample
/// buffer (errors are logged too). Runtime failures go to `reporter`.
fn build_buffer_output_stream(
    device: &Device,
    deafened: &Arc<AtomicBool>,
    playback_buffer: &Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
    reporter: &FailureReporter,
) -> Result<cpal::Stream, AudioError> {
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
            return Err(map_build_error(&e));
        }
    };

    if let Err(e) = stream.play() {
        error!("Failed to start playback stream: {}", e);
        return Err(map_play_error(&e));
    }

    Ok(stream)
}

/// Block on control messages until Stop, rebuilding the output stream in
/// place on `SwitchDevice`. The new stream is built BEFORE the old one is
/// dropped so a failed switch keeps audio on the previous device. `stream`
/// is `None` when playback could not start; a successful switch then starts
/// it. A switch that succeeds after a reported failure withdraws the
/// "Speaker stopped working" notice.
fn playback_control_loop(
    mut stream: Option<cpal::Stream>,
    mut reporter: FailureReporter,
    active: &ActiveLabel,
    deafened: &Arc<AtomicBool>,
    playback_buffer: &Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
    control_rx: &mut mpsc::Receiver<PlaybackControl>,
    sink: Option<AudioErrorSink>,
) {
    while let Some(msg) = control_rx.blocking_recv() {
        match msg {
            PlaybackControl::Stop => break,
            PlaybackControl::SwitchDevice(new_device, reply) => {
                let new_reporter = FailureReporter::new(
                    AudioErrorKind::PlaybackFailed,
                    new_device.label.clone(),
                    sink.clone(),
                );
                match build_buffer_output_stream(
                    &new_device.device,
                    deafened,
                    playback_buffer,
                    &new_reporter,
                ) {
                    Ok(new_stream) => {
                        stream = Some(new_stream);
                        reporter.recover();
                        reporter = new_reporter;
                        set_active(active, Some(reporter.event.device.clone()));
                        let _ = reply.send(Ok(()));
                        info!("Playback switched to new output device");
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
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
    open: PlaybackOpen,
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

    let PlaybackOpen {
        device,
        fallback,
        active,
        fell_back,
        ready,
    } = open;
    let (stream, reporter, start) = open_playback_stream(
        device,
        fallback,
        &active,
        &deafened,
        &playback_buffer,
        sink.as_ref(),
    );
    if start == PlaybackStart::FellBack {
        fell_back.set();
    }
    let _ = ready.send(start);

    playback_control_loop(
        stream,
        reporter,
        &active,
        &deafened,
        &playback_buffer,
        control_rx,
        sink,
    );
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
        .map_err(|e| map_build_error(&e))?;

    stream.play().map_err(|e| map_play_error(&e))?;
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
    async fn late_playback_fallback_clears_the_busy_selection() {
        let mut handle = AudioHandle::new().unwrap();
        let a = "alsa:sysdefault:CARD=USB";
        handle.set_output_device(Some(a.into())).await.unwrap();

        // start(A); the wait times out; the task falls back afterwards.
        let mark = handle.begin_playback_generation();
        assert_eq!(handle.output_device_id(), Some(a));
        mark.set();
        assert_eq!(handle.output_device_id(), None, "the default plays now");

        // The next change starts from the default, not the busy speaker.
        let b = "alsa:hdmi:CARD=HDMI,DEV=0";
        handle.set_output_device(Some(b.into())).await.unwrap();
        assert_eq!(handle.output_device_id(), Some(b));
        handle.sync_output_fallback();
        assert_eq!(handle.output_device_id(), Some(b));
    }

    #[tokio::test]
    async fn stale_fallback_does_not_overwrite_a_newer_one() {
        let mut handle = AudioHandle::new().unwrap();
        handle
            .set_output_device(Some("alsa:sysdefault:CARD=USB".into()))
            .await
            .unwrap();

        // start(A) times out, playback restarts (B) on the same choice.
        let mark_a = handle.begin_playback_generation();
        let mark_b = handle.begin_playback_generation();
        // B falls back first, then the stale A.
        mark_b.set();
        mark_a.set();

        assert_eq!(handle.output_device_id(), None, "B's fallback survives");
        handle.sync_output_fallback();
        assert_eq!(handle.output_device_id(), None);
        // Consumed: a new choice is not cleared by anything pending.
        handle
            .set_output_device(Some("alsa:hdmi:CARD=HDMI,DEV=0".into()))
            .await
            .unwrap();
        assert_eq!(handle.output_device_id(), Some("alsa:hdmi:CARD=HDMI,DEV=0"));
    }

    #[tokio::test]
    async fn late_fallback_of_an_old_start_keeps_a_newer_selection() {
        let mut handle = AudioHandle::new().unwrap();
        handle
            .set_output_device(Some("alsa:sysdefault:CARD=USB".into()))
            .await
            .unwrap();

        // start(A), timeout, set(B), then the fallback for A arrives.
        let mark_a = handle.begin_playback_generation();
        let b = "alsa:hdmi:CARD=HDMI,DEV=0";
        handle.set_output_device(Some(b.into())).await.unwrap();
        mark_a.set();

        assert_eq!(handle.output_device_id(), Some(b));
        handle.sync_output_fallback();
        assert_eq!(handle.output_device_id(), Some(b), "B stays selected");
        // A later start for B is not affected by A's mark either.
        let _mark_b = handle.begin_playback_generation();
        assert_eq!(handle.output_device_id(), Some(b));
    }

    #[test]
    fn busy_device_maps_to_device_in_use() {
        // CPAL's ALSA backend reports EBUSY/EAGAIN as DeviceNotAvailable.
        assert!(matches!(
            map_build_error(&cpal::BuildStreamError::DeviceNotAvailable),
            AudioError::DeviceInUse
        ));
        assert!(matches!(
            map_play_error(&cpal::PlayStreamError::DeviceNotAvailable),
            AudioError::DeviceInUse
        ));
        assert!(matches!(
            map_build_error(&cpal::BuildStreamError::StreamConfigNotSupported),
            AudioError::StreamError(_)
        ));
        assert!(AudioError::DeviceInUse
            .to_string()
            .contains("system default"));
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

    type Events = Arc<Mutex<Vec<(AudioNotice, AudioErrorEvent)>>>;

    fn collecting_sink() -> (AudioErrorSink, Events) {
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        let store = events.clone();
        let sink: AudioErrorSink = Arc::new(move |notice: AudioNotice, event: &AudioErrorEvent| {
            store.lock().unwrap().push((notice, event.clone()));
        });
        (sink, events)
    }

    fn notices(events: &Events) -> Vec<AudioNotice> {
        events.lock().unwrap().iter().map(|(n, _)| *n).collect()
    }

    fn mic_reporter(sink: AudioErrorSink) -> FailureReporter {
        FailureReporter::new(AudioErrorKind::CaptureFailed, "USB Mic", Some(sink))
    }

    #[test]
    fn capture_stream_error_emits_audio_error() {
        let (sink, events) = collecting_sink();
        let mut on_error = stream_error_callback(mic_reporter(sink));

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
            vec![(
                AudioNotice::Failed,
                AudioErrorEvent {
                    kind: AudioErrorKind::CaptureFailed,
                    device: "USB Mic".into(),
                }
            )]
        );
        assert_eq!(AUDIO_ERROR_EVENT, "voice:audio_error");
        assert_eq!(AUDIO_RECOVERED_EVENT, "voice:audio_recovered");
        assert_eq!(
            serde_json::to_value(&events[0].1).unwrap(),
            serde_json::json!({ "kind": "capture_failed", "device": "USB Mic" })
        );
    }

    #[test]
    fn stalled_capture_is_reported_once() {
        let (sink, events) = collecting_sink();
        let heartbeat = Heartbeat::new();
        let start = heartbeat.epoch();
        let mut watchdog = CaptureWatchdog::new(heartbeat.clone(), mic_reporter(sink), start);

        heartbeat.beat_at(start);
        watchdog.check(start + Duration::from_secs(1));
        assert!(events.lock().unwrap().is_empty(), "fresh data is alive");

        // No callbacks for longer than the stall timeout: the mic is gone.
        watchdog.check(start + CAPTURE_STALL_TIMEOUT + Duration::from_secs(1));
        watchdog.check(start + CAPTURE_STALL_TIMEOUT + Duration::from_secs(2));
        assert_eq!(notices(&events), vec![AudioNotice::Failed]);
    }

    #[test]
    fn slow_first_callback_does_not_trip_the_watchdog() {
        let (sink, events) = collecting_sink();
        let heartbeat = Heartbeat::new();
        let t0 = heartbeat.epoch();
        // Opening took 5 s (Bluetooth profile switch, slow USB): the
        // watchdog is armed only once the stream runs.
        let armed = t0 + Duration::from_secs(5);
        let mut watchdog = CaptureWatchdog::new(heartbeat.clone(), mic_reporter(sink), armed);

        // Still waiting for the first callback, past the stall timeout but
        // within the first-callback deadline.
        watchdog.check(armed + CAPTURE_STALL_TIMEOUT + Duration::from_secs(1));
        assert!(
            events.lock().unwrap().is_empty(),
            "slow start is not a dead mic"
        );

        let first = armed + Duration::from_secs(5);
        heartbeat.beat_at(first);
        watchdog.check(first + Duration::from_secs(1));
        assert!(events.lock().unwrap().is_empty());

        // Once running, the normal stall timeout applies.
        watchdog.check(first + CAPTURE_STALL_TIMEOUT + Duration::from_secs(1));
        assert_eq!(notices(&events), vec![AudioNotice::Failed]);
    }

    #[test]
    fn capture_that_never_starts_is_reported() {
        let (sink, events) = collecting_sink();
        let heartbeat = Heartbeat::new();
        let t0 = heartbeat.epoch();
        let mut watchdog = CaptureWatchdog::new(heartbeat, mic_reporter(sink), t0);

        watchdog.check(t0 + Duration::from_secs(9));
        assert!(events.lock().unwrap().is_empty());
        watchdog.check(t0 + CAPTURE_FIRST_CALLBACK_TIMEOUT + Duration::from_secs(1));
        assert_eq!(notices(&events), vec![AudioNotice::Failed]);
    }

    #[test]
    fn recovered_capture_clears_the_error() {
        let (sink, events) = collecting_sink();
        let reporter = mic_reporter(sink);
        let heartbeat = Heartbeat::new();
        let t0 = heartbeat.epoch();
        let mut watchdog = CaptureWatchdog::new(heartbeat.clone(), reporter.clone(), t0);
        let secs = Duration::from_secs;

        heartbeat.beat_at(t0 + secs(1));
        watchdog.check(t0 + secs(5));
        assert!(reporter.is_failed());

        // Audio comes back (e.g. after resume): the error is withdrawn.
        heartbeat.beat_at(t0 + secs(6));
        watchdog.check(t0 + secs(6) + Duration::from_millis(500));
        assert!(!reporter.is_failed());
        assert_eq!(
            notices(&events),
            vec![AudioNotice::Failed, AudioNotice::Recovered]
        );

        // A later stall is reported again.
        watchdog.check(t0 + secs(10));
        assert_eq!(
            notices(&events),
            vec![
                AudioNotice::Failed,
                AudioNotice::Recovered,
                AudioNotice::Failed
            ]
        );
    }

    #[test]
    fn backend_reported_failure_clears_when_callbacks_resume() {
        let (sink, events) = collecting_sink();
        let reporter = mic_reporter(sink);
        let heartbeat = Heartbeat::new();
        let t0 = heartbeat.epoch();
        let mut watchdog = CaptureWatchdog::new(heartbeat.clone(), reporter.clone(), t0);
        let secs = Duration::from_secs;

        heartbeat.beat_at(t0 + secs(1));
        stream_error_callback(reporter.clone())(cpal::StreamError::StreamInvalidated);
        watchdog.check(t0 + secs(1) + Duration::from_millis(500));
        assert!(reporter.is_failed(), "no callback since the error");

        heartbeat.beat_at(t0 + secs(2));
        watchdog.check(t0 + secs(2) + Duration::from_millis(100));
        assert_eq!(
            notices(&events),
            vec![AudioNotice::Failed, AudioNotice::Recovered]
        );
    }

    #[test]
    fn oversized_callbacks_convert_without_growing_the_buffer() {
        let mut converted: Vec<f32> = Vec::with_capacity(MAX_PUSH_SAMPLES);
        let capacity = converted.capacity();
        // A huge i16 stereo period, far beyond the scratch size.
        let data: Vec<i16> = (0..MAX_PUSH_SAMPLES * 3 + 6)
            .map(|i| if i % 2 == 0 { i16::MAX } else { 0 })
            .collect();
        let mut seen = 0usize;
        convert_in_chunks(&data, 2, &mut converted, |chunk| {
            assert_eq!(chunk.len() % 2, 0, "chunks keep whole frames");
            assert!((chunk[0] - 1.0).abs() < 1e-3);
            seen += chunk.len();
        });
        assert_eq!(seen, data.len());
        assert_eq!(converted.capacity(), capacity);
    }

    #[test]
    fn missing_saved_device_falls_back_to_the_default_on_start() {
        // Saved headset unplugged between calls: start on the default.
        let device = resolve_device(Some("Headset"), None, || Some("Speakers"), true, false);
        assert_eq!(device.unwrap(), "Speakers");

        // An explicit mid-call switch to a missing device is an error.
        let err = resolve_device(
            Some("Headset"),
            None::<&str>,
            || Some("Speakers"),
            false,
            false,
        )
        .unwrap_err();
        assert!(matches!(err, AudioError::DeviceNotFound(ref n) if n == "Headset"));

        assert_eq!(
            resolve_device(
                Some("Headset"),
                Some("Headset"),
                || Some("Speakers"),
                true,
                false
            )
            .unwrap(),
            "Headset"
        );
        assert!(matches!(
            resolve_device(Some("Mic"), None::<&str>, || None, true, true),
            Err(AudioError::NoInputDevice)
        ));
        assert!(matches!(
            resolve_device(None, None::<&str>, || None, true, false),
            Err(AudioError::NoOutputDevice)
        ));
    }

    // --- Mic switching (make-before-break, retry, restore) ---

    /// Scripted capture factory: each `spawn` pops the next result.
    #[derive(Default)]
    struct FakeFactory {
        script: Mutex<std::collections::VecDeque<bool>>,
        log: Arc<Mutex<Vec<String>>>,
        reported: Mutex<Vec<Option<String>>>,
        recovered: Mutex<Vec<Option<String>>>,
    }

    struct FakeTask {
        name: String,
        active: AtomicBool,
        failed: bool,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl CaptureStream for FakeTask {
        fn activate(&self) {
            self.active.store(true, Ordering::SeqCst);
            self.log
                .lock()
                .unwrap()
                .push(format!("activate {}", self.name));
        }
        fn is_healthy(&self) -> bool {
            !self.failed
        }
        async fn stop(self) {
            self.log.lock().unwrap().push(format!("stop {}", self.name));
        }
    }

    impl FakeFactory {
        fn new(script: &[bool]) -> Self {
            Self {
                script: Mutex::new(script.iter().copied().collect()),
                ..Self::default()
            }
        }
        fn task(&self, name: &str, failed: bool) -> FakeTask {
            FakeTask {
                name: name.into(),
                active: AtomicBool::new(true),
                failed,
                log: self.log.clone(),
            }
        }
        fn log(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }

    impl CaptureFactory for FakeFactory {
        type Task = FakeTask;
        async fn spawn(&self, device: Option<&str>) -> Result<FakeTask, AudioError> {
            let name = device.unwrap_or("default").to_string();
            let ok = self.script.lock().unwrap().pop_front().unwrap_or(false);
            self.log
                .lock()
                .unwrap()
                .push(format!("spawn {name} {}", if ok { "ok" } else { "err" }));
            if ok {
                Ok(FakeTask {
                    name,
                    active: AtomicBool::new(false),
                    failed: false,
                    log: self.log.clone(),
                })
            } else {
                Err(AudioError::StreamError(format!("{name} busy")))
            }
        }
        fn report_failure(&self, device: Option<&str>) {
            self.reported.lock().unwrap().push(device.map(String::from));
        }
        fn report_recovered(&self, device: Option<&str>) {
            self.recovered
                .lock()
                .unwrap()
                .push(device.map(String::from));
        }
    }

    #[tokio::test]
    async fn mic_switch_builds_new_before_stopping_old() {
        let factory = FakeFactory::new(&[true]);
        let mut capture = Some(factory.task("A", false));
        let mut current = Some("A".to_string());

        let switched = switch_capture(&factory, &mut capture, &mut current, Some("B".into()))
            .await
            .unwrap();

        assert!(switched);
        assert_eq!(factory.log(), ["spawn B ok", "stop A", "activate B"]);
        assert_eq!(current.as_deref(), Some("B"));
        assert!(capture.unwrap().active.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn mic_switch_retries_after_releasing_the_old_stream() {
        // The device allows one open stream: first attempt fails, retry works.
        let factory = FakeFactory::new(&[false, true]);
        let mut capture = Some(factory.task("A", false));
        let mut current = Some("A".to_string());

        let switched = switch_capture(&factory, &mut capture, &mut current, Some("A2".into()))
            .await
            .unwrap();

        assert!(switched);
        assert_eq!(
            factory.log(),
            ["spawn A2 err", "stop A", "spawn A2 ok", "activate A2"]
        );
        assert_eq!(current.as_deref(), Some("A2"));
    }

    #[tokio::test]
    async fn failed_mic_switch_restores_the_previous_mic() {
        let factory = FakeFactory::new(&[false, false, true]);
        let mut capture = Some(factory.task("A", false));
        let mut current = Some("A".to_string());

        let err = switch_capture(&factory, &mut capture, &mut current, Some("B".into()))
            .await
            .unwrap_err();

        assert!(matches!(err, AudioError::StreamError(_)));
        assert_eq!(
            factory.log(),
            [
                "spawn B err",
                "stop A",
                "spawn B err",
                "spawn A ok",
                "activate A"
            ]
        );
        assert_eq!(current.as_deref(), Some("A"), "choice unchanged");
        assert_eq!(capture.as_ref().map(|t| t.name.as_str()), Some("A"));
        assert!(factory.reported.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn restoring_a_failed_mic_withdraws_its_notice() {
        // The current mic had failed; the new one cannot open; the old one
        // comes back on restore: its "stopped working" notice must go.
        let factory = FakeFactory::new(&[false, false, true]);
        let mut capture = Some(factory.task("A", true));
        let mut current = Some("A".to_string());

        assert!(
            switch_capture(&factory, &mut capture, &mut current, Some("B".into()))
                .await
                .is_err()
        );
        assert_eq!(
            *factory.recovered.lock().unwrap(),
            vec![Some("A".to_string())]
        );

        // A healthy mic that is restored had no notice to withdraw.
        let factory = FakeFactory::new(&[false, false, true]);
        let mut capture = Some(factory.task("A", false));
        let _ = switch_capture(&factory, &mut capture, &mut current, Some("B".into())).await;
        assert!(factory.recovered.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_restore_falls_back_to_the_system_default() {
        // The saved mic X is busy (capture runs elsewhere); switching to Y
        // fails, restoring X fails too: the default keeps the call audible.
        let factory = FakeFactory::new(&[false, false, false, true]);
        let mut capture = Some(factory.task("X", false));
        let mut current = Some("X".to_string());

        let err = switch_capture(&factory, &mut capture, &mut current, Some("Y".into()))
            .await
            .unwrap_err();

        // The caller learns that capture now runs on the default (not that
        // the old mic is still active).
        assert!(
            matches!(err, AudioError::UsingSystemDefault(ref m) if m.contains("Y busy")),
            "{err:?}"
        );
        assert!(err.to_string().contains("system default is used"));
        assert_eq!(
            factory.log(),
            [
                "spawn Y err",
                "stop X",
                "spawn Y err",
                "spawn X err",
                "spawn default ok",
                "activate default"
            ]
        );
        assert_eq!(capture.as_ref().map(|t| t.name.as_str()), Some("default"));
        assert_eq!(current, None, "capture now follows the system default");
        assert!(factory.reported.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mic_lost_when_neither_new_nor_previous_opens_is_reported() {
        let factory = FakeFactory::new(&[false, false, false]);
        let mut capture = Some(factory.task("A", false));
        let mut current = Some("A".to_string());

        assert!(
            switch_capture(&factory, &mut capture, &mut current, Some("B".into()))
                .await
                .is_err()
        );
        assert!(capture.is_none());
        assert_eq!(
            *factory.reported.lock().unwrap(),
            vec![Some("A".to_string())]
        );
    }

    #[tokio::test]
    async fn reselecting_a_healthy_mic_is_a_no_op_but_a_failed_one_restarts() {
        let factory = FakeFactory::new(&[true]);
        let mut capture = Some(factory.task("A", false));
        let mut current = Some("A".to_string());
        let switched = switch_capture(&factory, &mut capture, &mut current, Some("A".into()))
            .await
            .unwrap();
        assert!(!switched);
        assert!(factory.log().is_empty());

        let mut capture = Some(factory.task("A", true));
        let switched = switch_capture(&factory, &mut capture, &mut current, Some("A".into()))
            .await
            .unwrap();
        assert!(switched, "a dead stream on the same device is rebuilt");
        assert_eq!(factory.log(), ["spawn A ok", "stop A", "activate A"]);
    }

    #[tokio::test]
    async fn mic_switch_without_running_capture_starts_one() {
        // Capture failed to open at join: choosing a mic starts it.
        let factory = FakeFactory::new(&[false]);
        let mut capture: Option<FakeTask> = None;
        let mut current = None;
        assert!(
            switch_capture(&factory, &mut capture, &mut current, Some("B".into()))
                .await
                .is_err()
        );
        assert_eq!(
            factory.log(),
            ["spawn B err"],
            "no retry without an old stream"
        );
    }

    #[test]
    fn playback_error_kind_serializes() {
        assert_eq!(
            serde_json::to_value(AudioErrorKind::PlaybackFailed).unwrap(),
            serde_json::json!("playback_failed")
        );
    }
}
