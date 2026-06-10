//! Audio Handle - Send + Sync wrapper for audio system
//!
//! This module provides a thread-safe handle to the audio system by moving
//! non-Send/Sync types (`cpal::Stream`) into background tasks.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, Host};
use opus::{Channels as OpusChannels, Decoder, Encoder};
use tauri::Emitter;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::{
    AudioDevice, AudioDeviceList, AudioError, VadConfig, CAPTURE_CHANNELS, CHANNELS, FRAME_SIZE,
    RNNOISE_FRAME_SIZE, SAMPLE_RATE,
};

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

    /// Control channel for capture task
    capture_control: Option<mpsc::Sender<CaptureControl>>,

    /// Control channel for playback task
    playback_control: Option<mpsc::Sender<PlaybackControl>>,

    /// Control channel for mic test task
    mic_test_control: Option<mpsc::Sender<()>>,

    /// Selected input device name
    input_device_name: Option<String>,

    /// Selected output device name
    output_device_name: Option<String>,

    /// VAD + noise suppression configuration
    vad_config: VadConfig,

    /// Tauri app handle for emitting events from capture thread
    app_handle: Option<tauri::AppHandle>,
}

/// Control messages for capture task
enum CaptureControl {
    Stop,
}

/// Control messages for playback task
enum PlaybackControl {
    Stop,
    /// Rebuild the output stream on a new device, keeping the same sample
    /// buffer (mid-call speaker switching). Boxed: `cpal::Device` is large.
    SwitchDevice(Box<Device>),
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
            capture_control: None,
            playback_control: None,
            mic_test_control: None,
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

    /// Set the input device by name.
    ///
    /// Takes effect when the next capture stream starts; an active capture
    /// is not rebuilt (input switching mid-capture is not supported yet).
    pub fn set_input_device(&mut self, device_id: Option<String>) {
        self.input_device_name = device_id;
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

    /// Start audio capture in a background task
    pub async fn start_capture(
        &mut self,
        output_tx: mpsc::Sender<Vec<u8>>,
    ) -> Result<(), AudioError> {
        // Stop existing capture if running
        self.stop_capture().await;

        let device = self.get_device(self.input_device_name.as_deref(), true)?;
        let muted = self.muted.clone();
        let vad_config = self.vad_config.clone();
        let app_handle = self.app_handle.clone();

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<CaptureControl>(1);
        self.capture_control = Some(control_tx);

        // Spawn capture task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_capture_task(
                device,
                muted,
                output_tx,
                &mut control_rx,
                vad_config,
                app_handle,
            );
        });

        info!("Audio capture started");
        Ok(())
    }

    /// Stop audio capture
    pub async fn stop_capture(&mut self) {
        if let Some(control) = self.capture_control.take() {
            let _ = control.send(CaptureControl::Stop).await;
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

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<PlaybackControl>(1);
        self.playback_control = Some(control_tx);

        // Spawn playback task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_playback_task(device, deafened, input_rx, &mut control_rx);
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

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<PlaybackControl>(1);
        self.playback_control = Some(control_tx);

        // Spawn PCM playback task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_pcm_playback_task(device, deafened, input_rx, &mut control_rx);
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

    /// Set Tauri app handle (call during voice initialization before start_capture)
    pub fn set_app_handle(&mut self, handle: tauri::AppHandle) {
        self.app_handle = Some(handle);
    }

    /// Start microphone test
    pub async fn start_mic_test(&mut self, device_id: Option<String>) -> Result<(), AudioError> {
        // Stop existing test if running
        self.stop_mic_test().await;

        let device = self.get_device(device_id.as_deref(), true)?;
        let mic_level = self.mic_test_level.clone();

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<()>(1);
        self.mic_test_control = Some(control_tx);

        // Spawn mic test task
        tokio::task::spawn_blocking(move || {
            run_mic_test_task(device, mic_level, &mut control_rx);
        });

        info!("Microphone test started");
        Ok(())
    }

    /// Stop microphone test
    pub async fn stop_mic_test(&mut self) {
        if let Some(control) = self.mic_test_control.take() {
            let _ = control.send(()).await;
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
        self.mic_test_control.is_some()
    }

    /// Stop all audio streams
    pub async fn stop_all(&mut self) {
        self.stop_capture().await;
        self.stop_playback().await;
        self.stop_mic_test().await;
        info!("All audio streams stopped");
    }
}

/// Speech hold-open duration: keep gate open for 300ms after last speech detection
/// to avoid clipping word endings.
const SPEECH_HOLD_MS: u128 = 300;

/// Run capture task (owns the Stream)
///
/// Captures mono audio, runs each 480-sample (10ms) chunk through RNNoise for
/// VAD probability + denoising, accumulates 960-sample (20ms) Opus frames, and
/// gates output based on VAD state.
fn run_capture_task(
    device: Device,
    muted: Arc<AtomicBool>,
    output_tx: mpsc::Sender<Vec<u8>>,
    control_rx: &mut mpsc::Receiver<CaptureControl>,
    vad_config: VadConfig,
    app_handle: Option<tauri::AppHandle>,
) {
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: CAPTURE_CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: BufferSize::Default,
    };

    // Mono Opus encoder — capture is mono, playback remains stereo
    let mut encoder = match Encoder::new(SAMPLE_RATE, OpusChannels::Mono, opus::Application::Voip) {
        Ok(enc) => enc,
        Err(e) => {
            error!("Failed to create encoder: {}", e);
            return;
        }
    };

    // RNNoise denoiser — processes 480-sample frames, returns VAD probability
    let mut denoiser = nnnoiseless::DenoiseState::new();
    let mut first_frame = true;

    // RNNoise I/O buffers (480 mono samples = 10ms) — pre-allocated to avoid RT allocation
    let mut rnnoise_input: Vec<f32> = Vec::with_capacity(RNNOISE_FRAME_SIZE);
    let mut rnnoise_output: Vec<f32> = vec![0.0f32; RNNOISE_FRAME_SIZE];

    // Opus frame accumulators (960 mono samples = 20ms)
    let frame_samples = FRAME_SIZE * CAPTURE_CHANNELS as usize; // 960
    let mut denoised_accumulator: Vec<f32> = Vec::with_capacity(frame_samples);
    let mut original_accumulator: Vec<f32> = Vec::with_capacity(frame_samples);

    // VAD gate state
    let mut gate_open = false;
    let mut indicator_speaking = false;
    let mut last_speech_time = Instant::now();

    // Opus encode buffer (reused across frames)
    let mut encode_buf = vec![0u8; 4000];

    let stream = match device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            // When muted, skip all processing but emit not-speaking if needed
            if muted.load(Ordering::Relaxed) {
                if indicator_speaking {
                    indicator_speaking = false;
                    gate_open = false;
                    if let Some(ref app) = app_handle {
                        let _ = app.emit("voice:speaking", false);
                    }
                }
                return;
            }

            // Process incoming mono samples
            for &sample in data {
                // Accumulate into RNNoise 480-sample input buffer (i16 range)
                rnnoise_input.push(sample * 32767.0);

                // Also keep the original f32 sample for non-denoised path
                original_accumulator.push(sample);

                if rnnoise_input.len() == RNNOISE_FRAME_SIZE {
                    // Run RNNoise: denoises and returns VAD probability
                    rnnoise_output.fill(0.0);
                    let vad_prob = denoiser.process_frame(&mut rnnoise_output, &rnnoise_input);

                    if first_frame {
                        // First frame output has fade-in artifact — discard
                        // but still use it for accumulator sizing
                        first_frame = false;
                        rnnoise_output.fill(0.0);
                    }

                    // Scale denoised output back to f32 [-1.0, 1.0] range
                    for s in &mut rnnoise_output {
                        *s /= 32768.0;
                    }
                    denoised_accumulator.extend_from_slice(&rnnoise_output);
                    rnnoise_input.clear();

                    // --- VAD gate logic ---
                    let threshold = vad_config.get_threshold();

                    if vad_prob > threshold {
                        if !indicator_speaking {
                            indicator_speaking = true;
                            if let Some(ref app) = app_handle {
                                let _ = app.emit("voice:speaking", true);
                            }
                        }
                        last_speech_time = Instant::now();
                    } else if indicator_speaking
                        && last_speech_time.elapsed().as_millis() > SPEECH_HOLD_MS
                    {
                        indicator_speaking = false;
                        if let Some(ref app) = app_handle {
                            let _ = app.emit("voice:speaking", false);
                        }
                    }

                    // Gate decision: if VAD disabled, always open
                    gate_open = if vad_config.is_enabled() {
                        indicator_speaking
                    } else {
                        true
                    };
                }

                // Check if we have a full Opus frame (960 mono samples)
                if denoised_accumulator.len() >= frame_samples {
                    if gate_open {
                        // Choose denoised or original audio
                        let source = if vad_config.is_denoise_enabled() {
                            &denoised_accumulator[..frame_samples]
                        } else {
                            &original_accumulator[..frame_samples]
                        };

                        let samples_i16: Vec<i16> = source
                            .iter()
                            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                            .collect();

                        match encoder.encode(&samples_i16, &mut encode_buf) {
                            Ok(len) => {
                                let encoded = encode_buf[..len].to_vec();
                                if let Err(e) = output_tx.try_send(encoded) {
                                    warn!("Failed to send encoded audio: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Opus encode error: {}", e);
                            }
                        }
                    }

                    // Drain consumed samples from both accumulators
                    denoised_accumulator.drain(..frame_samples);
                    original_accumulator.drain(..frame_samples);
                }
            }
        },
        |err| {
            error!("Audio capture stream error: {}", err);
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to build capture stream: {}", e);
            return;
        }
    };

    if let Err(e) = stream.play() {
        error!("Failed to start capture stream: {}", e);
        return;
    }

    // Block until stop signal
    while let Some(msg) = control_rx.blocking_recv() {
        match msg {
            CaptureControl::Stop => break,
        }
    }

    drop(stream);
    info!("Capture task stopped");
}

/// Run playback task (owns the Stream)
fn run_playback_task(
    device: Device,
    deafened: Arc<AtomicBool>,
    mut input_rx: mpsc::Receiver<Vec<u8>>,
    control_rx: &mut mpsc::Receiver<PlaybackControl>,
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

    let Some(stream) = build_buffer_output_stream(&device, &deafened, &playback_buffer) else {
        return;
    };

    playback_control_loop(stream, &deafened, &playback_buffer, control_rx);
    info!("Playback task stopped");
}

/// Build and start a CPAL output stream that drains the shared sample
/// buffer. Returns `None` (with the error logged) when the stream can't be
/// created or started on the given device.
fn build_buffer_output_stream(
    device: &Device,
    deafened: &Arc<AtomicBool>,
    playback_buffer: &Arc<std::sync::Mutex<std::collections::VecDeque<f32>>>,
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
        |err| {
            error!("Audio playback stream error: {}", err);
        },
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
) {
    while let Some(msg) = control_rx.blocking_recv() {
        match msg {
            PlaybackControl::Stop => break,
            PlaybackControl::SwitchDevice(new_device) => {
                match build_buffer_output_stream(&new_device, deafened, playback_buffer) {
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

    let Some(stream) = build_buffer_output_stream(&device, &deafened, &playback_buffer) else {
        return;
    };

    playback_control_loop(stream, &deafened, &playback_buffer, control_rx);
    info!("PCM playback task stopped");
}

/// Run microphone test task (owns the Stream)
fn run_mic_test_task(
    device: Device,
    mic_level: Arc<AtomicU8>,
    control_rx: &mut mpsc::Receiver<()>,
) {
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: BufferSize::Default,
    };

    let mic_level_clone = mic_level.clone();

    let stream = match device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            // Calculate RMS level
            let rms: f32 = data.iter().map(|&s| s * s).sum::<f32>() / data.len() as f32;
            let rms = rms.sqrt();

            // Convert to 0-100 scale
            let level = (rms * 100.0).min(100.0) as u8;
            mic_level_clone.store(level, Ordering::Relaxed);
        },
        |err| {
            error!("Mic test stream error: {}", err);
        },
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to build mic test stream: {}", e);
            return;
        }
    };

    if let Err(e) = stream.play() {
        error!("Failed to start mic test stream: {}", e);
        return;
    }

    // Block until stop signal
    let _ = control_rx.blocking_recv();

    drop(stream);
    mic_level.store(0, Ordering::Relaxed);
    info!("Mic test task stopped");
}
