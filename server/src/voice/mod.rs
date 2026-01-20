//! Voice Service (SFU)
//!
//! WebRTC Selective Forwarding Unit for voice channels.
//!
//! Voice signaling is handled through WebSocket (see ws/mod.rs).
//! This module provides:
//! - SFU server for managing voice rooms and peer connections
//! - Track routing for RTP packet forwarding
//! - HTTP endpoints for ICE server configuration
//! - DM voice call signaling

pub mod call;
pub mod call_handlers;
pub mod call_service;
pub mod error;
mod handlers;
mod peer;
mod quality;
mod rate_limit;
pub mod screen_share;
pub mod sfu;
mod metrics;
mod signaling;
mod stats;
mod track;
mod track_types;
pub mod ws_handler;

use axum::{routing::get, Router};

use crate::api::AppState;

// Re-exports
pub use error::VoiceError;
pub use quality::Quality;
pub use screen_share::{
    ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo, ScreenShareStartRequest,
};
pub use sfu::{ParticipantInfo, Room, SfuServer};
pub use track_types::{TrackInfo, TrackKind, TrackSource};
pub use stats::{UserStats, VoiceStats};

/// Create voice router.
///
/// Note: Voice join/leave are handled via WebSocket events.
/// This router only provides ICE server configuration.
pub fn router() -> Router<AppState> {
    Router::new().route("/ice-servers", get(handlers::get_ice_servers))
}
