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
pub(crate) mod handlers;
mod metrics;
mod peer;
mod quality;
mod queries;
pub mod rate_limit;
pub mod screen_share;
pub mod sfu;
mod stats;
mod track;
mod track_types;
pub mod webcam;
pub mod ws_handler;

use axum::routing::get;
use axum::Router;
// Re-exports
pub use error::VoiceError;
pub use quality::Quality;
pub use screen_share::{
    ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo, ScreenShareLimiter,
    ScreenShareStartRequest, ScreenShareStopRequest,
};
pub use sfu::{ParticipantInfo, Room, SfuServer};
pub use stats::{UserStats, VoiceStats};
pub use track_types::{Layer, LayerPreference, TrackInfo, TrackKind, TrackSource};
pub use webcam::WebcamInfo;

use crate::api::AppState;

/// Create voice router.
///
/// Note: Voice join/leave are handled via WebSocket events.
/// This router only provides ICE server configuration.
pub fn router() -> Router<AppState> {
    Router::new().route("/ice-servers", get(handlers::get_ice_servers))
}

/// Derive time-limited TURN credentials per the coturn REST API convention
/// (`use-auth-secret`): username is `"{expiry_unix}:{subject}"`, credential is
/// `base64(HMAC-SHA1(secret, username))`.
///
/// Used for client-facing ICE config (subject = user id) and for the SFU's
/// own peer connections (subject = "sfu") — coturn accepts any subject as
/// long as the HMAC matches.
pub(crate) fn turn_rest_credentials(
    secret: &str,
    ttl_secs: u64,
    subject: &str,
) -> (String, String) {
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use base64::Engine as _;
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
        + ttl_secs;
    let username = format!("{expiry}:{subject}");

    let mut mac =
        Hmac::<Sha1>::new_from_slice(secret.as_bytes()).expect("HMAC-SHA1 accepts any key length");
    mac.update(username.as_bytes());
    let credential = BASE64_STANDARD.encode(mac.finalize().into_bytes());

    (username, credential)
}

#[cfg(test)]
mod turn_credentials_tests {
    use super::turn_rest_credentials;

    #[test]
    fn derives_rest_api_format() {
        let (username, credential) = turn_rest_credentials("s3cret", 3600, "sfu");
        let (expiry, subject) = username.split_once(':').expect("expiry:subject format");
        assert!(expiry.parse::<u64>().is_ok(), "expiry must be unix seconds");
        assert_eq!(subject, "sfu");
        assert!(
            !credential.is_empty(),
            "credential must be non-empty base64 HMAC"
        );
        // Deterministic for the same username: recompute and compare via a
        // second call with the same inputs within the same second is flaky,
        // so just assert base64 shape (HMAC-SHA1 = 20 bytes -> 28 chars).
        assert_eq!(credential.len(), 28);
    }
}
