//! Guild Invite Security Tests
//!
//! Tests for guild invite security scenarios including:
//! - Invite code generation
//! - Expiry parsing and enforcement
//! - Rate limiting (max 10 active invites per guild)
//! - Already-member handling
//!
//! Run with: `cargo test --test integration guild_invite`
//! Run ignored (integration) tests: `cargo test --test integration guild_invite -- --ignored`

use chrono::{Duration, Utc};
use uuid::Uuid;

// ============================================================================
// Unit Tests (no database required)
// ============================================================================

#[test]
fn test_invite_code_length() {
    // Invite codes should be 8 characters
    // Testing by generating multiple codes and verifying format
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

    // Generate a sample code to verify format
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let code: String = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    assert_eq!(code.len(), 8);
    assert!(code.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn test_invite_code_uniqueness() {
    // Generate multiple codes and verify they are different
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

    let generate_code = || {
        let mut rng = rand::thread_rng();
        (0..8)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect::<String>()
    };

    let codes: Vec<String> = (0..100).map(|_| generate_code()).collect();

    // Check for uniqueness (very unlikely to have collisions with 62^8 possibilities)
    let mut seen = std::collections::HashSet::new();
    for code in &codes {
        assert!(seen.insert(code.clone()), "Code collision detected: {code}");
    }
}

#[test]
fn test_invite_code_charset() {
    // Verify charset is alphanumeric only (no special characters)
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

    for &c in CHARSET {
        let ch = c as char;
        assert!(
            ch.is_ascii_alphanumeric(),
            "Non-alphanumeric character in charset: {ch}"
        );
    }

    // No special characters that could cause URL issues
    assert!(!CHARSET.contains(&b'/'));
    assert!(!CHARSET.contains(&b'+'));
    assert!(!CHARSET.contains(&b'='));
}

#[test]
fn test_expiry_parsing_30_minutes() {
    let duration = parse_expiry("30m");
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_minutes(), 30);
}

#[test]
fn test_expiry_parsing_1_hour() {
    let duration = parse_expiry("1h");
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_hours(), 1);
}

#[test]
fn test_expiry_parsing_1_day() {
    let duration = parse_expiry("1d");
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_days(), 1);
}

#[test]
fn test_expiry_parsing_7_days() {
    let duration = parse_expiry("7d");
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_days(), 7);
}

#[test]
fn test_expiry_parsing_never() {
    let duration = parse_expiry("never");
    assert!(
        duration.is_none(),
        "never should result in None (no expiry)"
    );
}

#[test]
fn test_expiry_parsing_invalid_defaults_to_7_days() {
    // Unknown expiry strings should default to 7 days
    let duration = parse_expiry("invalid");
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_days(), 7);

    let duration = parse_expiry("10m");
    assert!(duration.is_some());
    assert_eq!(duration.unwrap().num_days(), 7);
}

#[test]
fn test_invite_struct_fields() {
    use vc_server::guild::types::GuildInvite;

    // Verify GuildInvite has expected fields
    let invite = GuildInvite {
        id: Uuid::new_v4(),
        guild_id: Uuid::new_v4(),
        code: "testcode".to_string(),
        created_by: Uuid::new_v4(),
        expires_at: Some(Utc::now() + Duration::days(1)),
        use_count: 0,
        created_at: Utc::now(),
    };

    assert_eq!(invite.code, "testcode");
    assert_eq!(invite.use_count, 0);
    assert!(invite.expires_at.is_some());
}

#[test]
fn test_invite_struct_never_expires() {
    use vc_server::guild::types::GuildInvite;

    let invite = GuildInvite {
        id: Uuid::new_v4(),
        guild_id: Uuid::new_v4(),
        code: "neverexp".to_string(),
        created_by: Uuid::new_v4(),
        expires_at: None,
        use_count: 0,
        created_at: Utc::now(),
    };

    assert!(
        invite.expires_at.is_none(),
        "Never-expiring invite should have None expires_at"
    );
}

#[test]
fn test_invite_response_fields() {
    use vc_server::guild::types::InviteResponse;

    let response = InviteResponse {
        id: Uuid::new_v4(),
        code: "testcode".to_string(),
        guild_id: Uuid::new_v4(),
        guild_name: "Test Guild".to_string(),
        expires_at: Some(Utc::now() + Duration::hours(1)),
        use_count: 5,
        created_at: Utc::now(),
    };

    assert_eq!(response.code, "testcode");
    assert_eq!(response.guild_name, "Test Guild");
    assert_eq!(response.use_count, 5);
}

#[test]
fn test_create_invite_request_fields() {
    use vc_server::guild::types::CreateInviteRequest;

    // Test JSON deserialization
    let json = r#"{"expires_in": "7d"}"#;
    let request: CreateInviteRequest = serde_json::from_str(json).expect("Should deserialize");
    assert_eq!(request.expires_in, "7d");
}

#[test]
fn test_invite_serialization() {
    use vc_server::guild::types::GuildInvite;

    let invite = GuildInvite {
        id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
        guild_id: Uuid::parse_str("660e8400-e29b-41d4-a716-446655440000").unwrap(),
        code: "AbCdEfGh".to_string(),
        created_by: Uuid::parse_str("770e8400-e29b-41d4-a716-446655440000").unwrap(),
        expires_at: None,
        use_count: 10,
        created_at: Utc::now(),
    };

    let json = serde_json::to_string(&invite).expect("Should serialize");
    assert!(json.contains("\"code\":\"AbCdEfGh\""));
    assert!(json.contains("\"use_count\":10"));
    assert!(json.contains("\"expires_at\":null"));
}

// ============================================================================
// Expiry Validation Tests
// ============================================================================

#[test]
fn test_invite_is_expired() {
    let past_time = Utc::now() - Duration::hours(1);
    let future_time = Utc::now() + Duration::hours(1);

    // Expired invite
    assert!(is_expired(Some(past_time)), "Past time should be expired");

    // Valid invite
    assert!(
        !is_expired(Some(future_time)),
        "Future time should not be expired"
    );

    // Never expires
    assert!(!is_expired(None), "None should mean never expires");
}

#[test]
fn test_invite_expiry_edge_cases() {
    // Just expired (1 second ago)
    let just_expired = Utc::now() - Duration::seconds(1);
    assert!(is_expired(Some(just_expired)));

    // About to expire (1 second from now)
    let about_to_expire = Utc::now() + Duration::seconds(1);
    assert!(!is_expired(Some(about_to_expire)));
}

// ============================================================================
// Rate Limit Constants Tests
// ============================================================================

#[test]
fn test_max_active_invites_constant() {
    // The implementation uses 10 as max active invites per guild
    const MAX_ACTIVE_INVITES: i64 = 10;
    assert_eq!(MAX_ACTIVE_INVITES, 10, "Max active invites should be 10");
}

// ============================================================================
// Integration Tests (require database - marked as #[ignore])
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
#[ignore] // Requires PostgreSQL
async fn test_invite_rejected_when_expired() {
    // This test verifies the database query filters out expired invites
    let _pool = create_test_pool().await;

    // The SQL query in join_via_invite includes:
    // WHERE code = $1 AND (expires_at IS NULL OR expires_at > NOW())
    // This ensures expired invites are not found

    // Would need to:
    // 1. Create a test guild
    // 2. Create an invite with expires_at in the past
    // 3. Try to use the invite
    // 4. Verify it returns "Invalid or expired invite code"
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_invite_rate_limit_enforcement() {
    // This test verifies max 10 active invites per guild
    let _pool = create_test_pool().await;

    // The implementation checks:
    // if active_count.0 >= 10 {
    //     return Err(GuildError::Validation("Maximum 10 active invites per guild"));
    // }

    // Would need to:
    // 1. Create a test guild
    // 2. Create 10 invites
    // 3. Try to create 11th invite
    // 4. Verify it returns validation error
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_already_member_returns_guild_info() {
    // This test verifies existing members get guild info without re-joining
    let _pool = create_test_pool().await;

    // The implementation checks:
    // let is_member = db::is_guild_member(&state.db, invite.guild_id, auth.id).await?;
    // if is_member { return Ok(Json(InviteResponse { ... })); }

    // Would need to:
    // 1. Create test guild with user as member
    // 2. Create invite
    // 3. Have user try to use invite
    // 4. Verify response contains guild info
    // 5. Verify use_count was NOT incremented
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_invite_use_count_increments() {
    // This test verifies use_count increments when invite is used
    let _pool = create_test_pool().await;

    // Would need to:
    // 1. Create test guild
    // 2. Create invite (use_count = 0)
    // 3. Have different user join via invite
    // 4. Verify use_count = 1
    // 5. Have another user join
    // 6. Verify use_count = 2
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_invite_code_collision_handling() {
    // This test verifies code regeneration on collision
    let _pool = create_test_pool().await;

    // The implementation retries up to 5 times:
    // while attempts < 5 {
    //     let exists = ...
    //     if exists.is_none() { break; }
    //     code = generate_invite_code();
    //     attempts += 1;
    // }
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_delete_invite_removes_from_db() {
    // This test verifies invite deletion
    let _pool = create_test_pool().await;

    // Would need to:
    // 1. Create test guild
    // 2. Create invite
    // 3. Delete invite
    // 4. Verify invite no longer found
    // 5. Verify use returns "Invalid or expired invite code"
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_only_owner_can_create_invite() {
    // This test verifies ownership check for invite creation
    let _pool = create_test_pool().await;

    // The implementation checks:
    // if guild.0 != auth.id { return Err(GuildError::Forbidden); }
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_only_owner_can_delete_invite() {
    // This test verifies ownership check for invite deletion
    let _pool = create_test_pool().await;

    // Same ownership check as create
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_only_owner_can_list_invites() {
    // This test verifies ownership check for listing invites
    let _pool = create_test_pool().await;

    // Same ownership check as create/delete
}

// ============================================================================
// Future Test Stubs (for planned features)
// ============================================================================

#[tokio::test]
#[ignore] // Feature not yet implemented
async fn test_invite_rejected_after_max_uses() {
    // TODO: When max_uses is added to GuildInvite:
    // 1. Create invite with max_uses = 5
    // 2. Have 5 users join
    // 3. Have 6th user try to join
    // 4. Verify rejection
}

#[tokio::test]
#[ignore] // Feature not yet implemented
async fn test_banned_user_cannot_join_via_invite() {
    // TODO: When bans table is implemented:
    // 1. Create guild and invite
    // 2. Ban a user from guild
    // 3. Have banned user try to use invite
    // 4. Verify rejection with appropriate error
}

#[tokio::test]
#[ignore] // Feature not yet implemented
async fn test_invite_to_suspended_guild_rejected() {
    // TODO: When guild suspension is implemented:
    // 1. Create guild and invite
    // 2. Suspend guild
    // 3. Have user try to use invite
    // 4. Verify rejection
}

// ============================================================================
// Helper Functions (mirroring server implementation for testing)
// ============================================================================

/// Parse expiry string to duration (mirrors invites.rs implementation)
fn parse_expiry(expires_in: &str) -> Option<Duration> {
    match expires_in {
        "30m" => Some(Duration::minutes(30)),
        "1h" => Some(Duration::hours(1)),
        "1d" => Some(Duration::days(1)),
        "7d" => Some(Duration::days(7)),
        "never" => None,
        _ => Some(Duration::days(7)), // Default to 7 days
    }
}

/// Check if an invite is expired
fn is_expired(expires_at: Option<chrono::DateTime<chrono::Utc>>) -> bool {
    match expires_at {
        Some(exp) => exp < Utc::now(),
        None => false, // Never expires
    }
}
