//! Audio Input/Output
//!
//! Handles audio capture, playback, encoding/decoding with cpal and opus.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, Stream, StreamConfig};
use opus::{Channels, Decoder, Encoder};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Audio configuration constants
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 2;
pub const FRAME_SIZE_MS: usize = 20;
pub const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS) / 1000; // 960 samples per channel

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
    #[error("Device is in use by another application")]
    DeviceInUse,
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
}

/// List of audio devices
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioDeviceList {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

/// Audio pipeline for voice chat
pub struct AudioPipeline {
    host: Host,

    // Capture
    input_device: Option<Device>,
    input_stream: Option<Stream>,
    encoder: Option<Encoder>,

    // Playback
    output_device: Option<Device>,
    output_stream: Option<Stream>,
    decoder: Option<Decoder>,

    // State
    muted: Arc<AtomicBool>,
    deafened: Arc<AtomicBool>,

    // Mic test
    mic_test_stream: Option<Stream>,
    mic_test_level: Arc<AtomicU8>,

    // Channels for audio data
    capture_tx: Option<mpsc::Sender<Vec<u8>>>,
    playback_rx: Option<mpsc::Receiver<Vec<u8>>>,
}

impl AudioPipeline {
    /// Create a new audio pipeline
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();

        Ok(Self {
            host,
            input_device: None,
            input_stream: None,
            encoder: None,
            output_device: None,
            output_stream: None,
            decoder: None,
            muted: Arc::new(AtomicBool::new(false)),
            deafened: Arc::new(AtomicBool::new(false)),
            mic_test_stream: None,
            mic_test_level: Arc::new(AtomicU8::new(0)),
            capture_tx: None,
            playback_rx: None,
        })
    }

    /// Enumerate all audio devices
    pub fn enumerate_devices(&self) -> Result<AudioDeviceList, AudioError> {
        let default_input = self.host.default_input_device();
        let default_output = self.host.default_output_device();

        let default_input_name = default_input.as_ref().and_then(|d| d.name().ok());
        let default_output_name = default_output.as_ref().and_then(|d| d.name().ok());

        let inputs: Vec<AudioDevice> = self
            .host
            .input_devices()
            .map_err(|e| AudioError::ConfigError(e.to_string()))?
            .filter_map(|d| {
                d.name().ok().map(|name| AudioDevice {
                    device_id: name.clone(),
                    label: name.clone(),
                    is_default: Some(&name) == default_input_name.as_ref(),
                })
            })
            .collect();

        let outputs: Vec<AudioDevice> = self
            .host
            .output_devices()
            .map_err(|e| AudioError::ConfigError(e.to_string()))?
            .filter_map(|d| {
                d.name().ok().map(|name| AudioDevice {
                    device_id: name.clone(),
                    label: name.clone(),
                    is_default: Some(&name) == default_output_name.as_ref(),
                })
            })
            .collect();

        Ok(AudioDeviceList { inputs, outputs })
    }

    /// Set the input device by name
    pub fn set_input_device(&mut self, device_id: Option<&str>) -> Result<(), AudioError> {
        self.input_device = match device_id {
            Some(id) => {
                let device = self
                    .host
                    .input_devices()
                    .map_err(|e| AudioError::ConfigError(e.to_string()))?
                    .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                    .ok_or_else(|| AudioError::DeviceNotFound(id.to_string()))?;
                Some(device)
            }
            None => self.host.default_input_device(),
        };
        Ok(())
    }

    /// Set the output device by name
    pub fn set_output_device(&mut self, device_id: Option<&str>) -> Result<(), AudioError> {
        self.output_device = match device_id {
            Some(id) => {
                let device = self
                    .host
                    .output_devices()
                    .map_err(|e| AudioError::ConfigError(e.to_string()))?
                    .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                    .ok_or_else(|| AudioError::DeviceNotFound(id.to_string()))?;
                Some(device)
            }
            None => self.host.default_output_device(),
        };
        Ok(())
    }

    /// Initialize the opus encoder
    fn create_encoder() -> Result<Encoder, AudioError> {
        Encoder::new(SAMPLE_RATE, Channels::Stereo, opus::Application::Voip)
            .map_err(|e| AudioError::EncoderError(e.to_string()))
    }

    /// Initialize the opus decoder
    fn create_decoder() -> Result<Decoder, AudioError> {
        Decoder::new(SAMPLE_RATE, Channels::Stereo)
            .map_err(|e| AudioError::DecoderError(e.to_string()))
    }

    /// Start audio capture
    pub fn start_capture(&mut self, tx: mpsc::Sender<Vec<u8>>) -> Result<(), AudioError> {
        let device = self
            .input_device
            .clone()
            .or_else(|| self.host.default_input_device())
            .ok_or(AudioError::NoInputDevice)?;

        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let encoder = Self::create_encoder()?;
        let muted = self.muted.clone();

        // Buffer for accumulating samples
        let mut sample_buffer: Vec<f32> = Vec::with_capacity(FRAME_SIZE * CHANNELS as usize * 2);
        let frame_samples = FRAME_SIZE * CHANNELS as usize;

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if muted.load(Ordering::Relaxed) {
                        return;
                    }

                    // Accumulate samples
                    sample_buffer.extend_from_slice(data);

                    // Process complete frames
                    while sample_buffer.len() >= frame_samples {
                        let frame: Vec<f32> = sample_buffer.drain(..frame_samples).collect();

                        // Convert f32 to i16 for opus
                        let samples_i16: Vec<i16> = frame
                            .iter()
                            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                            .collect();

                        // Encode with opus
                        let mut encoded = vec![0u8; 4000]; // Max opus packet size
                        match encoder.encode(&samples_i16, &mut encoded) {
                            Ok(len) => {
                                encoded.truncate(len);
                                if let Err(e) = tx.try_send(encoded) {
                                    warn!("Failed to send encoded audio: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Opus encode error: {}", e);
                            }
                        }
                    }
                },
                |err| {
                    error!("Input stream error: {}", err);
                },
                None,
            )
            .map_err(|e| AudioError::StreamError(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::StreamError(e.to_string()))?;

        self.input_stream = Some(stream);
        self.encoder = Some(encoder);
        self.capture_tx = Some(tx);

        info!("Audio capture started");
        Ok(())
    }

    /// Stop audio capture
    pub fn stop_capture(&mut self) {
        self.input_stream = None;
        self.encoder = None;
        self.capture_tx = None;
        info!("Audio capture stopped");
    }

    /// Start audio playback
    pub fn start_playback(&mut self, mut rx: mpsc::Receiver<Vec<u8>>) -> Result<(), AudioError> {
        let device = self
            .output_device
            .clone()
            .or_else(|| self.host.default_output_device())
            .ok_or(AudioError::NoOutputDevice)?;

        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let mut decoder = Self::create_decoder()?;
        let deafened = self.deafened.clone();

        // Ring buffer for decoded audio
        let buffer = Arc::new(std::sync::Mutex::new(Vec::<f32>::with_capacity(
            FRAME_SIZE * CHANNELS as usize * 10,
        )));
        let buffer_clone = buffer.clone();

        // Spawn task to decode incoming audio
        tokio::spawn(async move {
            while let Some(encoded) = rx.recv().await {
                if deafened.load(Ordering::Relaxed) {
                    continue;
                }

                let mut decoded = vec![0i16; FRAME_SIZE * CHANNELS as usize];
                match decoder.decode(&encoded, &mut decoded, false) {
                    Ok(samples) => {
                        // Convert i16 to f32
                        let samples_f32: Vec<f32> = decoded[..samples * CHANNELS as usize]
                            .iter()
                            .map(|&s| s as f32 / 32767.0)
                            .collect();

                        if let Ok(mut buf) = buffer_clone.lock() {
                            buf.extend(samples_f32);
                            // Limit buffer size to prevent memory growth
                            if buf.len() > FRAME_SIZE * CHANNELS as usize * 20 {
                                buf.drain(..FRAME_SIZE * CHANNELS as usize * 10);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Opus decode error: {}", e);
                    }
                }
            }
        });

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(mut buf) = buffer.lock() {
                        let samples_to_copy = data.len().min(buf.len());
                        if samples_to_copy > 0 {
                            data[..samples_to_copy].copy_from_slice(&buf[..samples_to_copy]);
                            buf.drain(..samples_to_copy);
                        }
                        // Fill remaining with silence
                        for sample in &mut data[samples_to_copy..] {
                            *sample = 0.0;
                        }
                    } else {
                        // Fill with silence if buffer locked
                        for sample in data.iter_mut() {
                            *sample = 0.0;
                        }
                    }
                },
                |err| {
                    error!("Output stream error: {}", err);
                },
                None,
            )
            .map_err(|e| AudioError::StreamError(e.to_string()))?;

        stream
            .play()
            .map_err(|e| AudioError::StreamError(e.to_string()))?;

        self.output_stream = Some(stream);
        self.decoder = Some(Self::create_decoder()?);

        info!("Audio playback started");
        Ok(())
    }

    /// Stop audio playback
    pub fn stop_playback(&mut self) {
        self.output_stream = None;
        self.decoder = None;
        self.playback_rx = None;
        info!("Audio playback stopped");
    }

    /// Set muted state
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        debug!("Audio muted: {}", muted);
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
        debug!("Audio deafened: {}", deafened);
    }

    /// Get deafened state
    pub fn is_deafened(&self) -> bool {
        self.deafened.load(Ordering::Relaxed)
    }

    /// Start microphone test (local only, no network)
    pub fn start_mic_test(&mut self, device_id: Option<&str>) -> Result<(), AudioError> {
        // Stop any existing test
        self.stop_mic_test();

        // Select input device
        let device = match device_id {
            Some(id) => self
                .host
                .input_devices()
                .map_err(|e| AudioError::ConfigError(e.to_string()))?
                .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                .ok_or_else(|| AudioError::DeviceNotFound(id.to_string()))?,
            None => self
                .host
                .default_input_device()
                .ok_or(AudioError::NoInputDevice)?,
        };

        let config = StreamConfig {
            channels: CHANNELS,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let level = self.mic_test_level.clone();
        level.store(0, Ordering::Relaxed);

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Calculate RMS level
                    let sum: f32 = data.iter().map(|s| s * s).sum();
                    let rms = (sum / data.len() as f32).sqrt();

                    // Convert to 0-100 range
                    let level_pct = (rms * 200.0).min(100.0) as u8;
                    level.store(level_pct, Ordering::Relaxed);
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

        self.mic_test_stream = Some(stream);
        info!("Mic test started");
        Ok(())
    }

    /// Stop microphone test
    pub fn stop_mic_test(&mut self) {
        if self.mic_test_stream.is_some() {
            self.mic_test_stream = None;
            self.mic_test_level.store(0, Ordering::Relaxed);
            info!("Mic test stopped");
        }
    }

    /// Get current mic test level (0-100)
    pub fn get_mic_test_level(&self) -> u8 {
        self.mic_test_level.load(Ordering::Relaxed)
    }

    /// Check if mic test is running
    pub fn is_mic_test_running(&self) -> bool {
        self.mic_test_stream.is_some()
    }

    /// Stop all audio streams
    pub fn stop_all(&mut self) {
        self.stop_capture();
        self.stop_playback();
        self.stop_mic_test();
    }
}

impl Drop for AudioPipeline {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl Default for AudioPipeline {
    fn default() -> Self {
        Self::new().expect("Failed to create audio pipeline")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_pipeline_creation() {
        let pipeline = AudioPipeline::new();
        assert!(pipeline.is_ok());
    }

    #[test]
    fn test_device_enumeration() {
        let pipeline = AudioPipeline::new().unwrap();
        let devices = pipeline.enumerate_devices();
        // This may fail on CI without audio devices
        if let Ok(devices) = devices {
            println!("Input devices: {:?}", devices.inputs);
            println!("Output devices: {:?}", devices.outputs);
        }
    }
}
