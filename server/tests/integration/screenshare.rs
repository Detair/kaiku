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
    let stream_id = Uuid::new_v4();
    let json = format!(
        r#"{{"stream_id":"{stream_id}","quality":"high","has_audio":true,"source_label":"Display 1"}}"#
    );
    let req: ScreenShareStartRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(req.stream_id, stream_id);
    assert_eq!(req.quality, Quality::High);
    assert!(req.has_audio);
    assert_eq!(req.source_label, "Display 1");
}

#[test]
fn test_screen_share_info_roundtrip() {
    let stream_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let info = ScreenShareInfo::new(
        stream_id,
        user_id,
        "testuser".to_string(),
        "Display 1".to_string(),
        true,
        Quality::High,
    );

    let json = serde_json::to_string(&info).unwrap();
    let deserialized: ScreenShareInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.stream_id, stream_id);
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

    let stream_id = Uuid::new_v4();

    let event = ServerEvent::ScreenShareStarted {
        channel_id,
        user_id,
        stream_id,
        username: "alice".to_string(),
        source_label: "Display 1".to_string(),
        has_audio: true,
        quality: Quality::High,
        started_at: "2026-01-01T00:00:00+00:00".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"screen_share_started\""));
    assert!(json.contains(&format!("\"channel_id\":\"{channel_id}\"")));
    assert!(json.contains(&format!("\"user_id\":\"{user_id}\"")));
    assert!(json.contains(&format!("\"stream_id\":\"{stream_id}\"")));
    assert!(json.contains("\"username\":\"alice\""));
    assert!(json.contains("\"source_label\":\"Display 1\""));
    assert!(json.contains("\"has_audio\":true"));
    assert!(json.contains("\"started_at\":\"2026-01-01T00:00:00+00:00\""));
}

#[test]
fn test_server_event_screen_share_stopped_serialization() {
    use vc_server::ws::ServerEvent;

    let channel_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let stream_id = Uuid::new_v4();

    let event = ServerEvent::ScreenShareStopped {
        channel_id,
        user_id,
        stream_id,
        reason: "user_stopped".to_string(),
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"screen_share_stopped\""));
    assert!(json.contains(&format!("\"stream_id\":\"{stream_id}\"")));
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
// Multi-Stream Unit Tests (no database/Redis required)
// ============================================================================
use vc_server::voice::sfu::Room;

/// A single user can add multiple screen share streams to a room.
#[tokio::test]
async fn test_user_can_start_multiple_streams() {
    let channel_id = Uuid::new_v4();
    let room = Room::new(channel_id, 10);
    let user_id = Uuid::new_v4();

    // Start stream 1 — should succeed
    let stream1 = Uuid::new_v4();
    let info1 = ScreenShareInfo::new(
        stream1,
        user_id,
        "alice".to_string(),
        "Display 1".to_string(),
        true,
        Quality::High,
    );
    room.add_screen_share(info1).await;
    assert_eq!(room.get_user_stream_count(user_id).await, 1);

    // Start stream 2 — should succeed
    let stream2 = Uuid::new_v4();
    let info2 = ScreenShareInfo::new(
        stream2,
        user_id,
        "alice".to_string(),
        "Firefox".to_string(),
        false,
        Quality::Medium,
    );
    room.add_screen_share(info2).await;
    assert_eq!(room.get_user_stream_count(user_id).await, 2);

    // Start stream 3 — should succeed
    let stream3 = Uuid::new_v4();
    let info3 = ScreenShareInfo::new(
        stream3,
        user_id,
        "alice".to_string(),
        "VS Code".to_string(),
        false,
        Quality::Low,
    );
    room.add_screen_share(info3).await;
    assert_eq!(room.get_user_stream_count(user_id).await, 3);

    // All 3 streams should be in the room
    let all_shares = room.get_screen_shares().await;
    assert_eq!(all_shares.len(), 3);

    // Each stream has its own unique stream_id
    let stream_ids: std::collections::HashSet<Uuid> =
        all_shares.iter().map(|s| s.stream_id).collect();
    assert_eq!(stream_ids.len(), 3);
    assert!(stream_ids.contains(&stream1));
    assert!(stream_ids.contains(&stream2));
    assert!(stream_ids.contains(&stream3));
}

/// Removing a single stream only removes that stream, not others from the same user.
#[tokio::test]
async fn test_remove_single_stream_preserves_others() {
    let channel_id = Uuid::new_v4();
    let room = Room::new(channel_id, 10);
    let user_id = Uuid::new_v4();

    let stream1 = Uuid::new_v4();
    let stream2 = Uuid::new_v4();

    room.add_screen_share(ScreenShareInfo::new(
        stream1,
        user_id,
        "alice".to_string(),
        "Display 1".to_string(),
        true,
        Quality::High,
    ))
    .await;
    room.add_screen_share(ScreenShareInfo::new(
        stream2,
        user_id,
        "alice".to_string(),
        "Firefox".to_string(),
        false,
        Quality::Medium,
    ))
    .await;

    assert_eq!(room.get_user_stream_count(user_id).await, 2);

    // Remove only stream 1
    let removed = room.remove_screen_share(stream1).await;
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().source_label, "Display 1");

    // Stream 2 should still exist
    assert_eq!(room.get_user_stream_count(user_id).await, 1);
    let remaining = room.get_screen_shares().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].stream_id, stream2);
    assert_eq!(remaining[0].source_label, "Firefox");
}

/// When a user leaves, all their streams are cleaned up.
#[tokio::test]
async fn test_leave_cleans_up_all_user_streams() {
    let channel_id = Uuid::new_v4();
    let room = Room::new(channel_id, 10);
    let user_id = Uuid::new_v4();
    let other_user_id = Uuid::new_v4();

    // User starts 2 streams
    room.add_screen_share(ScreenShareInfo::new(
        Uuid::new_v4(),
        user_id,
        "alice".to_string(),
        "Display 1".to_string(),
        true,
        Quality::High,
    ))
    .await;
    room.add_screen_share(ScreenShareInfo::new(
        Uuid::new_v4(),
        user_id,
        "alice".to_string(),
        "Firefox".to_string(),
        false,
        Quality::Medium,
    ))
    .await;

    // Another user has 1 stream
    let other_stream_id = Uuid::new_v4();
    room.add_screen_share(ScreenShareInfo::new(
        other_stream_id,
        other_user_id,
        "bob".to_string(),
        "Display 2".to_string(),
        false,
        Quality::High,
    ))
    .await;

    assert_eq!(room.get_screen_shares().await.len(), 3);

    // User leaves — remove all their streams
    let removed = room.remove_user_screen_shares(user_id).await;
    assert_eq!(removed.len(), 2);

    // Only the other user's stream remains
    let remaining = room.get_screen_shares().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].user_id, other_user_id);
    assert_eq!(remaining[0].stream_id, other_stream_id);

    // User's count is now 0
    assert_eq!(room.get_user_stream_count(user_id).await, 0);
}

/// Multiple users can share simultaneously in the same room.
#[tokio::test]
async fn test_multiple_users_can_share_simultaneously() {
    let channel_id = Uuid::new_v4();
    let room = Room::new(channel_id, 10);

    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let user3 = Uuid::new_v4();

    room.add_screen_share(ScreenShareInfo::new(
        Uuid::new_v4(),
        user1,
        "alice".to_string(),
        "Display 1".to_string(),
        true,
        Quality::High,
    ))
    .await;
    room.add_screen_share(ScreenShareInfo::new(
        Uuid::new_v4(),
        user2,
        "bob".to_string(),
        "Display 1".to_string(),
        false,
        Quality::Medium,
    ))
    .await;
    room.add_screen_share(ScreenShareInfo::new(
        Uuid::new_v4(),
        user3,
        "carol".to_string(),
        "Display 1".to_string(),
        true,
        Quality::Low,
    ))
    .await;

    assert_eq!(room.get_screen_shares().await.len(), 3);
    assert_eq!(room.get_user_stream_count(user1).await, 1);
    assert_eq!(room.get_user_stream_count(user2).await, 1);
    assert_eq!(room.get_user_stream_count(user3).await, 1);
}

/// Removing a non-existent stream returns None.
#[tokio::test]
async fn test_remove_nonexistent_stream_returns_none() {
    let channel_id = Uuid::new_v4();
    let room = Room::new(channel_id, 10);

    let result = room.remove_screen_share(Uuid::new_v4()).await;
    assert!(result.is_none());
}

/// `ScreenShareStopRequest` deserializes correctly with `stream_id`.
#[test]
fn test_screen_share_stop_request_deserialization() {
    use vc_server::voice::ScreenShareStopRequest;

    let stream_id = Uuid::new_v4();
    let json = format!(r#"{{"stream_id":"{stream_id}"}}"#);
    let req: ScreenShareStopRequest = serde_json::from_str(&json).unwrap();

    assert_eq!(req.stream_id, stream_id);
}

// ============================================================================
// Integration Tests (require database + Redis)
// ============================================================================

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::PgPool;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    create_guild_with_default_role, create_test_user, create_voice_channel, generate_access_token,
    TestApp,
};

/// Screen share check endpoint requires authentication.
#[sqlx::test]
async fn test_screen_share_check_requires_auth(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let channel_id = Uuid::now_v7();

    let body = serde_json::json!({
        "stream_id": Uuid::new_v4(),
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1"
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/screenshare/check"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Screen share check endpoint requires `SCREEN_SHARE` permission.
#[sqlx::test]
async fn test_screen_share_check_requires_permission(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;

    // Guild with VIEW_CHANNEL + VOICE_CONNECT but NOT SCREEN_SHARE
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::VOICE_CONNECT;
    let guild_id = create_guild_with_default_role(&app.pool, owner_id, perms).await;
    let channel_id = create_voice_channel(&app.pool, guild_id, "voice-test").await;

    // Add a non-owner member who only has the @everyone role permissions
    sqlx::query("INSERT INTO guild_members (user_id, guild_id) VALUES ($1, $2)")
        .bind(member_id)
        .bind(guild_id)
        .execute(&app.pool)
        .await
        .expect("Failed to add member");

    // Use the non-owner member (owner bypasses permission checks)
    let token = generate_access_token(&app.config, member_id);
    let body = serde_json::json!({
        "stream_id": Uuid::new_v4(),
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1"
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/screenshare/check"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["allowed"], false);
    assert_eq!(json["error"], "no_permission");
}

/// Screen share check allows permitted users.
#[sqlx::test]
async fn test_screen_share_check_allowed(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;

    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::VOICE_CONNECT
        | GuildPermissions::SCREEN_SHARE;
    let guild_id = create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = create_voice_channel(&app.pool, guild_id, "voice-test").await;

    let token = generate_access_token(&app.config, user_id);
    let body = serde_json::json!({
        "stream_id": Uuid::new_v4(),
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1"
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/screenshare/check"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["allowed"], true);
    assert!(json.get("granted_quality").is_some());
}

/// Screen share start requires voice room membership.
#[sqlx::test]
#[ignore = "Requires SFU ScreenShareLimiter in test app"]
async fn test_screen_share_start_requires_room_membership(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;

    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::VOICE_CONNECT
        | GuildPermissions::SCREEN_SHARE;
    let guild_id = create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = create_voice_channel(&app.pool, guild_id, "voice-test").await;

    let token = generate_access_token(&app.config, user_id);
    let body = serde_json::json!({
        "stream_id": Uuid::new_v4(),
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1"
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/screenshare/start"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// Screen share stop is a no-op when not sharing.
#[sqlx::test]
async fn test_screen_share_stop_noop(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;

    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::VOICE_CONNECT
        | GuildPermissions::SCREEN_SHARE;
    let guild_id = create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = create_voice_channel(&app.pool, guild_id, "voice-test").await;

    let token = generate_access_token(&app.config, user_id);

    let stop_body = serde_json::json!({
        "stream_id": Uuid::new_v4()
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/screenshare/stop"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&stop_body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// Screen share check rejects invalid source labels (XSS attempt).
#[sqlx::test]
#[ignore = "Requires SFU ScreenShareLimiter in test app"]
async fn test_screen_share_check_invalid_source_label(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;

    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::VOICE_CONNECT
        | GuildPermissions::SCREEN_SHARE;
    let guild_id = create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = create_voice_channel(&app.pool, guild_id, "voice-test").await;

    let token = generate_access_token(&app.config, user_id);
    let body = serde_json::json!({
        "stream_id": Uuid::new_v4(),
        "quality": "medium",
        "has_audio": false,
        "source_label": "<script>alert(1)</script>"
    });

    let req = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/screenshare/check"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
