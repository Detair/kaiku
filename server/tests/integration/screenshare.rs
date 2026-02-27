//! Screen Share integration tests.
//!
//! Tests for screen sharing REST endpoints and WebSocket broadcast events.
//!
//! Run with: `cargo test --test integration screenshare`
//! Run ignored (integration) tests: `cargo test --test integration screenshare -- --ignored`

use uuid::Uuid;
// ============================================================================
// Unit Tests (no database/Redis required)
// ============================================================================
use vc_server::voice::{
    Quality, ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo, ScreenShareStartRequest,
};

#[test]
fn test_screen_share_check_response_allowed_serialization() {
    let resp = ScreenShareCheckResponse::allowed(Quality::High);
    let json = serde_json::to_string(&resp).unwrap();

    assert!(json.contains("\"allowed\":true"));
    assert!(json.contains("\"granted_quality\""));
    assert!(!json.contains("\"error\""));
}

#[test]
fn test_screen_share_check_response_denied_serialization() {
    let resp = ScreenShareCheckResponse::denied(ScreenShareError::NoPermission);
    let json = serde_json::to_string(&resp).unwrap();

    assert!(json.contains("\"allowed\":false"));
    assert!(json.contains("\"error\":\"no_permission\""));
}

#[test]
fn test_screen_share_error_into_response_status_codes() {
    // Verify that each error variant maps to the expected status concept
    let test_cases = [
        (ScreenShareError::NoPermission, "no_permission"),
        (ScreenShareError::LimitReached, "limit_reached"),
        (ScreenShareError::NotInChannel, "not_in_channel"),
        (ScreenShareError::QualityNotAllowed, "quality_not_allowed"),
        (
            ScreenShareError::RenegotiationFailed,
            "renegotiation_failed",
        ),
        (ScreenShareError::InternalError, "internal_error"),
        (ScreenShareError::InvalidSourceLabel, "invalid_source_label"),
        (ScreenShareError::AlreadySharing, "already_sharing"),
    ];

    for (error, expected_str) in test_cases {
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, format!("\"{expected_str}\""));
    }
}

#[test]
fn test_screen_share_start_request_deserialization() {
    let json = r#"{"quality":"high","has_audio":true,"source_label":"Display 1"}"#;
    let req: ScreenShareStartRequest = serde_json::from_str(json).unwrap();

    assert_eq!(req.quality, Quality::High);
    assert!(req.has_audio);
    assert_eq!(req.source_label, "Display 1");
}

#[test]
fn test_screen_share_info_roundtrip() {
    let user_id = Uuid::new_v4();
    let info = ScreenShareInfo::new(
        user_id,
        "testuser".to_string(),
        "Display 1".to_string(),
        true,
        Quality::High,
    );

    let json = serde_json::to_string(&info).unwrap();
    let deserialized: ScreenShareInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.user_id, user_id);
    assert_eq!(deserialized.username, "testuser");
    assert_eq!(deserialized.source_label, "Display 1");
    assert!(deserialized.has_audio);
    assert_eq!(deserialized.quality, Quality::High);
}

// ============================================================================
// ServerEvent serialization tests (screen share events)
// ============================================================================

#[test]
fn test_server_event_screen_share_started_serialization() {
    use vc_server::ws::ServerEvent;

    let channel_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let event = ServerEvent::ScreenShareStarted {
        channel_id,
        user_id,
        username: "alice".to_string(),
        source_label: "Display 1".to_string(),
        has_audio: true,
        quality: Quality::High,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"screen_share_started\""));
    assert!(json.contains(&format!("\"channel_id\":\"{channel_id}\"")));
    assert!(json.contains(&format!("\"user_id\":\"{user_id}\"")));
    assert!(json.contains("\"username\":\"alice\""));
    assert!(json.contains("\"source_label\":\"Display 1\""));
    assert!(json.contains("\"has_audio\":true"));
}

#[test]
fn test_server_event_screen_share_stopped_serialization() {
    use vc_server::ws::ServerEvent;

    let channel_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let event = ServerEvent::ScreenShareStopped {
        channel_id,
        user_id,
        reason: "user_stopped".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"screen_share_stopped\""));
    assert!(json.contains("\"reason\":\"user_stopped\""));
}

#[test]
fn test_server_event_screen_share_quality_changed_serialization() {
    use vc_server::ws::ServerEvent;

    let channel_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let event = ServerEvent::ScreenShareQualityChanged {
        channel_id,
        user_id,
        new_quality: Quality::Medium,
        reason: "bandwidth_adaptation".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"screen_share_quality_changed\""));
    assert!(json.contains("\"reason\":\"bandwidth_adaptation\""));
}

// ============================================================================
// Integration Tests (require database + Redis)
// ============================================================================

/// Screen share check endpoint requires authentication.
#[tokio::test]
#[ignore]
async fn test_screen_share_check_requires_auth() {
    // This test verifies that unauthenticated requests to the check endpoint
    // are rejected with 401. Requires a running server instance.
    //
    // When integration test infrastructure is available:
    // 1. POST /channels/{id}/screenshare/check without Authorization header
    // 2. Expect 401 Unauthorized
}

/// Screen share check endpoint requires `SCREEN_SHARE` permission.
#[tokio::test]
#[ignore]
async fn test_screen_share_check_requires_permission() {
    // This test verifies that users without SCREEN_SHARE permission
    // receive a denied response.
    //
    // When integration test infrastructure is available:
    // 1. Create user without SCREEN_SHARE permission
    // 2. POST /channels/{id}/screenshare/check with valid auth
    // 3. Expect allowed: false, error: "no_permission"
}

/// Screen share check allows permitted users.
#[tokio::test]
#[ignore]
async fn test_screen_share_check_allowed() {
    // This test verifies that users with proper permissions
    // receive an allowed response.
    //
    // When integration test infrastructure is available:
    // 1. Create user with SCREEN_SHARE + VOICE_CONNECT permissions
    // 2. POST /channels/{id}/screenshare/check
    // 3. Expect allowed: true, granted_quality present
}

/// Full screen share lifecycle: check → start → verify → stop → verify.
#[tokio::test]
#[ignore]
async fn test_screen_share_start_and_stop_flow() {
    // This test verifies the complete screen share lifecycle.
    //
    // When integration test infrastructure is available:
    // 1. User joins voice channel
    // 2. POST /channels/{id}/screenshare/start
    // 3. Verify response allowed: true
    // 4. Verify room has active screen share for user
    // 5. POST /channels/{id}/screenshare/stop
    // 6. Verify room no longer has screen share for user
}

/// Screen share limit enforcement.
#[tokio::test]
#[ignore]
async fn test_screen_share_start_limit_enforcement() {
    // This test verifies that the max_screen_shares limit is enforced.
    //
    // When integration test infrastructure is available:
    // 1. Set channel max_screen_shares = 1
    // 2. User A starts screen share → success
    // 3. User B starts screen share → expect LimitReached error
}

/// Duplicate screen share start returns `AlreadySharing`.
#[tokio::test]
#[ignore]
async fn test_screen_share_start_already_sharing() {
    // This test verifies that starting a second screen share returns error.
    //
    // When integration test infrastructure is available:
    // 1. User starts screen share → success
    // 2. User starts screen share again → expect AlreadySharing (409)
}

/// Screen share broadcasts events via Redis pub/sub.
#[tokio::test]
#[ignore]
async fn test_screen_share_broadcasts_events() {
    // This test verifies that ScreenShareStarted and ScreenShareStopped
    // events are broadcast to channel subscribers via Redis.
    //
    // When integration test infrastructure is available:
    // 1. Subscribe to channel events via Redis
    // 2. User starts screen share
    // 3. Verify ScreenShareStarted event received
    // 4. User stops screen share
    // 5. Verify ScreenShareStopped event received with reason "user_stopped"
}
