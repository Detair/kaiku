//! Audio Handle - Send + Sync wrapper for audio system
//!
//! This module provides a thread-safe handle to the audio system by moving
//! non-Send/Sync types (`cpal::Stream`) into background tasks.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, Host};
use opus::{Channels as OpusChannels, Decoder, Encoder};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::{AudioDevice, AudioDeviceList, AudioError, CHANNELS, FRAME_SIZE, SAMPLE_RATE};

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
}

/// Control messages for capture task
enum CaptureControl {
    Stop,
}

/// Control messages for playback task
enum PlaybackControl {
    Stop,
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

    /// Set the input device by name
    pub fn set_input_device(&mut self, device_id: Option<String>) {
        self.input_device_name = device_id;
    }

    /// Set the output device by name
    pub fn set_output_device(&mut self, device_id: Option<String>) {
        self.output_device_name = device_id;
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

        // Create control channel
        let (control_tx, mut control_rx) = mpsc::channel::<CaptureControl>(1);
        self.capture_control = Some(control_tx);

        // Spawn capture task that owns the Stream
        tokio::task::spawn_blocking(move || {
            run_capture_task(device, muted, output_tx, &mut control_rx);
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

    /// Start audio playback in a background task
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

/// Run capture task (owns the Stream)
fn run_capture_task(
    device: Device,
    muted: Arc<AtomicBool>,
    output_tx: mpsc::Sender<Vec<u8>>,
    control_rx: &mut mpsc::Receiver<CaptureControl>,
) {
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: BufferSize::Default,
    };

    let encoder = match Encoder::new(SAMPLE_RATE, OpusChannels::Stereo, opus::Application::Voip) {
        Ok(enc) => Arc::new(std::sync::Mutex::new(enc)),
        Err(e) => {
            error!("Failed to create encoder: {}", e);
            return;
        }
    };

    let sample_buffer = Arc::new(std::sync::Mutex::new(Vec::with_capacity(
        FRAME_SIZE * CHANNELS as usize * 2,
    )));
    let frame_samples = FRAME_SIZE * CHANNELS as usize;

    let encoder_clone = encoder;
    let sample_buffer_clone = sample_buffer;
    let muted_clone = muted;
    let output_tx_clone = output_tx;

    let stream = match device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            if muted_clone.load(Ordering::Relaxed) {
                return;
            }

            let mut buffer = sample_buffer_clone.lock().unwrap();
            buffer.extend_from_slice(data);

            while buffer.len() >= frame_samples {
                let frame: Vec<f32> = buffer.drain(..frame_samples).collect();

                let samples_i16: Vec<i16> = frame
                    .iter()
                    .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                    .collect();

                let mut encoded = vec![0u8; 4000];
                if let Ok(mut enc) = encoder_clone.lock() {
                    match enc.encode(&samples_i16, &mut encoded) {
                        Ok(len) => {
                            encoded.truncate(len);
                            if let Err(e) = output_tx_clone.try_send(encoded) {
                                warn!("Failed to send encoded audio: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Opus encode error: {}", e);
                        }
                    }
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
    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, StreamConfig};

    let config = StreamConfig {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        buffer_size: BufferSize::Default,
    };

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

    let playback_buffer_clone2 = playback_buffer;
    let deafened_clone = deafened;

    let stream = match device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            if deafened_clone.load(Ordering::Relaxed) {
                data.fill(0.0);
                return;
            }

            if let Ok(mut buffer) = playback_buffer_clone2.lock() {
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
            return;
        }
    };

    if let Err(e) = stream.play() {
        error!("Failed to start playback stream: {}", e);
        return;
    }

    // Block until stop signal
    while let Some(msg) = control_rx.blocking_recv() {
        match msg {
            PlaybackControl::Stop => break,
        }
    }

    drop(stream);
    info!("Playback task stopped");
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
