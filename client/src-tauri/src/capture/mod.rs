//! Capture Module
//!
//! Native screen capture (`scap`) and webcam capture (`nokhwa`).
//! Provides source enumeration, frame capture, and color space conversion to I420.

pub mod capturer;
pub mod convert;
pub mod source;
pub mod webcam;

#[cfg(all(test, target_os = "linux"))]
mod pipewire_ffi_tests;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Capture-related errors.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum CaptureError {
    #[error("No capture sources available")]
    NoSources,
    #[error("Source not found: {0}")]
    SourceNotFound(String),
    #[error("Capture not supported on this platform")]
    NotSupported,
    #[error("Permission denied for screen capture")]
    PermissionDenied,
    #[error("Capture already running")]
    AlreadyRunning,
    #[error("Capture not running")]
    NotRunning,
    #[error("Capture error: {0}")]
    Internal(String),
}

/// Type of capture source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CaptureSourceType {
    Monitor,
    Window,
}

/// A capture source (monitor or window) with optional thumbnail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureSource {
    /// Unique identifier for this source.
    pub id: String,
    /// Human-readable name (monitor name or window title).
    pub name: String,
    /// Source type.
    pub source_type: CaptureSourceType,
    /// Base64-encoded PNG thumbnail (~200px wide), if available.
    pub thumbnail: Option<String>,
    /// Whether this is the primary monitor.
    pub is_primary: bool,
}

/// A raw I420 (YUV 4:2:0 planar) frame ready for encoding.
#[allow(dead_code)]
pub struct I420Frame {
    /// Y plane data.
    pub y: Vec<u8>,
    /// U plane data (quarter resolution).
    pub u: Vec<u8>,
    /// V plane data (quarter resolution).
    pub v: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
}
