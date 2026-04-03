//! Audio Input/Output
//!
//! Handles audio capture, playback, encoding/decoding with cpal and opus.
//!
//! This module provides a Send + Sync audio handle that moves non-thread-safe
//! `cpal::Stream` objects into background tasks.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use thiserror::Error;

mod handle;

pub use handle::AudioHandle;

/// Audio configuration constants
pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 2;
pub const FRAME_SIZE_MS: usize = 20;
pub const FRAME_SIZE: usize = (SAMPLE_RATE as usize * FRAME_SIZE_MS) / 1000; // 960 samples per channel

/// Capture-only channel count (mono for RNNoise compatibility).
/// Playback/mixer stays stereo (`CHANNELS`) since remote peers may send stereo.
pub const CAPTURE_CHANNELS: u16 = 1;

/// RNNoise processes 480-sample frames (10ms at 48kHz mono)
pub const RNNOISE_FRAME_SIZE: usize = 480;

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
        self.threshold.load(Ordering::Relaxed) as f32 / 100.0
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

#[cfg(test)]
mod tests {
    use super::*;

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
