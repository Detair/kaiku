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
pub mod error;
mod handlers;
mod peer;
mod rate_limit;
pub mod sfu;
mod signaling;
mod track;
pub mod ws_handler;

use axum::{routing::get, Router};

use crate::api::AppState;

// Re-exports
pub use error::VoiceError;
pub use sfu::{ParticipantInfo, Room, SfuServer};

/// Create voice router.
///
/// Note: Voice join/leave are handled via WebSocket events.
/// This router only provides ICE server configuration.
pub fn router() -> Router<AppState> {
    Router::new().route("/ice-servers", get(handlers::get_ice_servers))
}
