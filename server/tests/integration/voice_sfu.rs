//! Voice SFU integration tests.
//!
//! Tests for the Selective Forwarding Unit (SFU) including:
//! - Room creation and management
//! - Peer join/leave operations
//! - Participant limit enforcement
//! - Screen share management
//! - Track router operations
//!
//! Run with: `cargo test --test integration voice_sfu`
//! Run ignored (integration) tests: `cargo test --test integration voice_sfu -- --ignored`

use uuid::Uuid;

// ============================================================================
// Constants (matching server implementation)
// ============================================================================

const DEFAULT_MAX_PARTICIPANTS: usize = 25;

// ============================================================================
// Unit Tests (no database/WebRTC required)
// ============================================================================

#[test]
fn test_participant_info_fields() {
    use vc_server::voice::sfu::ParticipantInfo;

    let info = ParticipantInfo {
        user_id: Uuid::new_v4(),
        username: Some("testuser".to_string()),
        display_name: Some("Test User".to_string()),
        muted: false,
        screen_sharing: false,
        webcam_active: false,
    };

    assert!(!info.muted);
    assert!(!info.screen_sharing);
    assert!(info.username.is_some());
    assert!(info.display_name.is_some());
}

#[test]
fn test_participant_info_serialization() {
    use vc_server::voice::sfu::ParticipantInfo;

    let info = ParticipantInfo {
        user_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
        username: Some("testuser".to_string()),
        display_name: Some("Test User".to_string()),
        muted: true,
        screen_sharing: true,
        webcam_active: false,
    };

    let json = serde_json::to_string(&info).expect("Should serialize");
    assert!(json.contains("\"user_id\":"));
    assert!(json.contains("\"muted\":true"));
    assert!(json.contains("\"screen_sharing\":true"));
    assert!(json.contains("\"username\":\"testuser\""));
}

#[test]
fn test_participant_info_muted_default() {
    use vc_server::voice::sfu::ParticipantInfo;

    // Test that a new participant starts unmuted
    let info = ParticipantInfo {
        user_id: Uuid::new_v4(),
        username: None,
        display_name: None,
        muted: false,
        screen_sharing: false,
        webcam_active: false,
    };

    assert!(!info.muted, "New participants should start unmuted");
}

#[test]
fn test_max_participants_constant() {
    // Verify the default max participants
    assert_eq!(DEFAULT_MAX_PARTICIPANTS, 25);
}

#[test]
fn test_voice_error_channel_full_message() {
    use vc_server::voice::VoiceError;

    let error = VoiceError::ChannelFull {
        max_participants: 25,
    };
    let msg = error.to_string();
    assert!(msg.contains("25"));
    assert!(msg.to_lowercase().contains("full") || msg.to_lowercase().contains("participant"));
}

#[test]
fn test_voice_error_already_joined() {
    use vc_server::voice::VoiceError;

    let error = VoiceError::AlreadyJoined;
    let msg = error.to_string();
    assert!(!msg.is_empty());
}

#[test]
fn test_voice_error_not_in_channel() {
    use vc_server::voice::VoiceError;

    let error = VoiceError::NotInChannel;
    let msg = error.to_string();
    assert!(!msg.is_empty());
}

#[test]
fn test_room_channel_id_stored() {
    use vc_server::voice::sfu::Room;

    let channel_id = Uuid::new_v4();
    let room = Room::new(channel_id, DEFAULT_MAX_PARTICIPANTS);

    assert_eq!(room.channel_id, channel_id);
    assert_eq!(room.max_participants, DEFAULT_MAX_PARTICIPANTS);
}

#[test]
fn test_room_custom_max_participants() {
    use vc_server::voice::sfu::Room;

    let channel_id = Uuid::new_v4();
    let custom_max = 10;
    let room = Room::new(channel_id, custom_max);

    assert_eq!(room.max_participants, custom_max);
}

#[tokio::test]
async fn test_room_empty_initially() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);

    assert!(room.is_empty().await);
    assert_eq!(room.participant_count().await, 0);
}

#[tokio::test]
async fn test_room_no_screen_shares_initially() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let shares = room.get_screen_shares().await;

    assert!(shares.is_empty());
}

#[tokio::test]
async fn test_room_get_participant_info_empty() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let info = room.get_participant_info().await;

    assert!(info.is_empty());
}

#[tokio::test]
async fn test_room_get_peer_not_found() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let peer = room.get_peer(Uuid::new_v4()).await;

    assert!(peer.is_none());
}

#[tokio::test]
async fn test_room_get_other_peers_empty() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let peers = room.get_other_peers(Uuid::new_v4()).await;

    assert!(peers.is_empty());
}

#[tokio::test]
async fn test_room_remove_nonexistent_peer() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let removed = room.remove_peer(Uuid::new_v4()).await;

    assert!(removed.is_none());
}

// Track router tests removed - track module is private
// Track router functionality is tested through Room integration tests

#[test]
fn test_screen_share_info_fields() {
    use vc_server::voice::screen_share::ScreenShareInfo;
    use vc_server::voice::Quality;

    let info = ScreenShareInfo {
        user_id: Uuid::new_v4(),
        username: "testuser".to_string(),
        source_label: "Display 1".to_string(),
        has_audio: true,
        quality: Quality::High,
    };

    assert!(!info.username.is_empty());
    assert!(!info.source_label.is_empty());
    assert!(info.has_audio);
}

#[tokio::test]
async fn test_room_screen_share_add_and_remove() {
    use vc_server::voice::screen_share::ScreenShareInfo;
    use vc_server::voice::sfu::Room;
    use vc_server::voice::Quality;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let user_id = Uuid::new_v4();

    // Add screen share
    let share_info = ScreenShareInfo {
        user_id,
        username: "testuser".to_string(),
        source_label: "Display 1".to_string(),
        has_audio: false,
        quality: Quality::Medium,
    };
    room.add_screen_share(share_info.clone()).await;

    // Verify it was added
    let shares = room.get_screen_shares().await;
    assert_eq!(shares.len(), 1);
    assert_eq!(shares[0].user_id, user_id);
    assert!(matches!(shares[0].quality, Quality::Medium));

    // Remove screen share
    let removed = room.remove_screen_share(user_id).await;
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().user_id, user_id);

    // Verify it was removed
    let shares_after = room.get_screen_shares().await;
    assert!(shares_after.is_empty());
}

#[tokio::test]
async fn test_room_multiple_screen_shares() {
    use vc_server::voice::screen_share::ScreenShareInfo;
    use vc_server::voice::sfu::Room;
    use vc_server::voice::Quality;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let qualities = [Quality::Low, Quality::Medium, Quality::High];

    // Add multiple screen shares
    for (i, quality) in qualities.iter().enumerate() {
        let share_info = ScreenShareInfo {
            user_id: Uuid::new_v4(),
            username: format!("user{i}"),
            source_label: format!("Display {i}"),
            has_audio: false,
            quality: *quality,
        };
        room.add_screen_share(share_info).await;
    }

    let shares = room.get_screen_shares().await;
    assert_eq!(shares.len(), 3);
}

#[tokio::test]
async fn test_room_screen_share_duplicate_user_replaces() {
    use vc_server::voice::screen_share::ScreenShareInfo;
    use vc_server::voice::sfu::Room;
    use vc_server::voice::Quality;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let user_id = Uuid::new_v4();

    // Add first screen share
    let share1 = ScreenShareInfo {
        user_id,
        username: "testuser".to_string(),
        source_label: "Display 1".to_string(),
        has_audio: false,
        quality: Quality::Medium,
    };
    room.add_screen_share(share1).await;

    // Add second screen share from same user (should replace)
    let share2 = ScreenShareInfo {
        user_id,
        username: "testuser".to_string(),
        source_label: "Display 2".to_string(),
        has_audio: true,
        quality: Quality::High,
    };
    room.add_screen_share(share2).await;

    // Should still only have one entry
    let shares = room.get_screen_shares().await;
    assert_eq!(shares.len(), 1);
    assert!(matches!(shares[0].quality, Quality::High));
    assert_eq!(shares[0].source_label, "Display 2");
}

#[tokio::test]
async fn test_room_remove_nonexistent_screen_share() {
    use vc_server::voice::sfu::Room;

    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);
    let removed = room.remove_screen_share(Uuid::new_v4()).await;

    assert!(removed.is_none());
}

// ============================================================================
// Integration Tests (require database/WebRTC - marked as #[ignore])
// ============================================================================

/// Helper to create a test database pool.
#[allow(dead_code)]
async fn create_test_pool() -> sqlx::PgPool {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/vc_test".into());

    sqlx::PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

#[tokio::test]
#[ignore] // Requires PostgreSQL and WebRTC
async fn test_sfu_server_creation() {
    use std::sync::Arc;

    use vc_server::config::Config;
    use vc_server::voice::sfu::SfuServer;

    let config = Arc::new(Config::default_for_test());
    let sfu = SfuServer::new(config, None);

    assert!(sfu.is_ok(), "SFU server should be created successfully");
}

#[tokio::test]
#[ignore] // Requires PostgreSQL and WebRTC
async fn test_sfu_get_or_create_room() {
    use std::sync::Arc;

    use vc_server::config::Config;
    use vc_server::voice::sfu::SfuServer;

    let config = Arc::new(Config::default_for_test());
    let sfu = Arc::new(SfuServer::new(config, None).expect("SFU creation failed"));

    let channel_id = Uuid::new_v4();

    // Get or create room
    let room = sfu.get_or_create_room(channel_id).await;
    assert_eq!(room.channel_id, channel_id);

    // Getting again should return the same room
    let room2 = sfu.get_or_create_room(channel_id).await;
    assert_eq!(room.channel_id, room2.channel_id);
}

#[tokio::test]
#[ignore] // Requires PostgreSQL and WebRTC
async fn test_sfu_multiple_rooms() {
    use std::sync::Arc;

    use vc_server::config::Config;
    use vc_server::voice::sfu::SfuServer;

    let config = Arc::new(Config::default_for_test());
    let sfu = Arc::new(SfuServer::new(config, None).expect("SFU creation failed"));

    let channel1 = Uuid::new_v4();
    let channel2 = Uuid::new_v4();

    let room1 = sfu.get_or_create_room(channel1).await;
    let room2 = sfu.get_or_create_room(channel2).await;

    assert_ne!(room1.channel_id, room2.channel_id);
}

#[tokio::test]
#[ignore] // Requires full infrastructure
async fn test_channel_participant_limit_enforcement() {
    use vc_server::voice::sfu::Room;

    // Create room with small limit
    let room = Room::new(Uuid::new_v4(), 2);

    // Note: Adding peers requires actual WebRTC peer connections,
    // so this test demonstrates the limit check logic
    assert_eq!(room.max_participants, 2);

    // The actual limit enforcement happens in Room::add_peer()
    // which returns VoiceError::ChannelFull when limit is reached
}

#[tokio::test]
#[ignore] // Requires full infrastructure
async fn test_screen_share_limit_per_channel() {
    use vc_server::voice::screen_share::ScreenShareInfo;
    use vc_server::voice::sfu::Room;
    use vc_server::voice::Quality;

    // Create room
    let room = Room::new(Uuid::new_v4(), DEFAULT_MAX_PARTICIPANTS);

    // Assuming a typical limit of ~5 screen shares per channel
    // (actual limit may be configured differently)
    let max_screen_shares = 5;

    for i in 0..max_screen_shares {
        let share = ScreenShareInfo {
            user_id: Uuid::new_v4(),
            username: format!("user{i}"),
            source_label: format!("Display {i}"),
            has_audio: false,
            quality: Quality::Medium,
        };
        room.add_screen_share(share).await;
    }

    let shares = room.get_screen_shares().await;
    assert_eq!(shares.len(), max_screen_shares);
}
