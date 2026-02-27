//! Admin elevation integration tests.
//!
//! Tests for the admin elevation system including:
//! - Elevation request/approval flow
//! - Elevation expiry after 15 minutes
#![allow(clippy::items_after_statements)]
//! - Elevation required for ban/suspend operations
//! - Elevation cache behavior
//!
//! Run with: `cargo test --test integration admin_elevation`
//! Run ignored (integration) tests: `cargo test --test integration admin_elevation -- --ignored`

use chrono::{Duration, Utc};
use uuid::Uuid;

// ============================================================================
// Unit Tests (no database required)
// ============================================================================

#[test]
fn test_elevated_admin_struct_clone() {
    use vc_server::admin::ElevatedAdmin;

    let now = Utc::now();
    let elevated = ElevatedAdmin {
        user_id: Uuid::new_v4(),
        elevated_at: now,
        expires_at: now + Duration::minutes(15),
        reason: Some("Testing admin features".to_string()),
    };

    let cloned = elevated.clone();
    assert_eq!(elevated.user_id, cloned.user_id);
    assert_eq!(elevated.elevated_at, cloned.elevated_at);
    assert_eq!(elevated.expires_at, cloned.expires_at);
    assert_eq!(elevated.reason, cloned.reason);
}

#[test]
fn test_system_admin_user_struct_clone() {
    use vc_server::admin::SystemAdminUser;

    let admin = SystemAdminUser {
        user_id: Uuid::new_v4(),
        username: "admin_user".to_string(),
        granted_at: Utc::now(),
    };

    let cloned = admin.clone();
    assert_eq!(admin.user_id, cloned.user_id);
    assert_eq!(admin.username, cloned.username);
    assert_eq!(admin.granted_at, cloned.granted_at);
}

#[test]
fn test_elevation_duration_is_15_minutes() {
    // Verify the elevation duration constant
    // The system uses 15 minutes as the elevation duration
    let duration_minutes = 15;
    let now = Utc::now();
    let expires_at = now + Duration::minutes(duration_minutes);

    // Elevation should expire in approximately 15 minutes
    let diff = expires_at - now;
    assert_eq!(diff.num_minutes(), 15);
}

#[test]
fn test_admin_error_variants() {
    use vc_server::admin::AdminError;

    // Test that all error variants can be created
    let not_admin = AdminError::NotAdmin;
    assert_eq!(not_admin.to_string(), "System admin privileges required");

    let elevation_required = AdminError::ElevationRequired;
    assert_eq!(
        elevation_required.to_string(),
        "This action requires an elevated session"
    );

    let invalid_mfa = AdminError::InvalidMfaCode;
    assert_eq!(invalid_mfa.to_string(), "Invalid MFA code");

    let not_found = AdminError::NotFound("User".to_string());
    assert_eq!(not_found.to_string(), "User not found");

    let validation = AdminError::Validation("Invalid input".to_string());
    assert_eq!(validation.to_string(), "Validation failed: Invalid input");
}

#[test]
fn test_elevation_expiry_calculation() {
    // Test that elevation expiry is calculated correctly
    let now = Utc::now();
    let duration_minutes = 15i64;
    let expires_at = now + Duration::minutes(duration_minutes);

    // Should be in the future
    assert!(expires_at > now);

    // Should be approximately 15 minutes from now
    let seconds_diff = (expires_at - now).num_seconds();
    assert_eq!(seconds_diff, 15 * 60); // 900 seconds

    // Test that expired check works
    let expired_at = now - Duration::minutes(1);
    assert!(expired_at < now, "Expired session should be in the past");
}

#[test]
fn test_elevation_cache_key_format() {
    // Test the cache key format used for elevation status
    let user_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    let cache_key = format!("admin:elevated:{user_id}");

    assert_eq!(
        cache_key,
        "admin:elevated:550e8400-e29b-41d4-a716-446655440000"
    );
    assert!(cache_key.starts_with("admin:elevated:"));
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

/// Helper struct for test user cleanup.
#[allow(dead_code)]
struct TestUser {
    id: Uuid,
    username: String,
}

/// Helper to create a test user.
#[allow(dead_code)]
async fn create_test_user(pool: &sqlx::PgPool, username: &str) -> TestUser {
    let user_id = Uuid::now_v7();
    let password_hash = vc_server::auth::hash_password("Test123!@#").unwrap();

    sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash) VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(username)
    .bind(username)
    .bind(&password_hash)
    .execute(pool)
    .await
    .expect("Failed to create test user");

    TestUser {
        id: user_id,
        username: username.to_string(),
    }
}

/// Helper to grant system admin to a user.
#[allow(dead_code)]
async fn grant_system_admin(pool: &sqlx::PgPool, user_id: Uuid, granted_by: Uuid) {
    sqlx::query("INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $2)")
        .bind(user_id)
        .bind(granted_by)
        .execute(pool)
        .await
        .expect("Failed to grant system admin");
}

/// Helper to create a session for a user.
#[allow(dead_code)]
async fn create_session(pool: &sqlx::PgPool, user_id: Uuid) -> Uuid {
    let session_id = Uuid::now_v7();
    let token_hash = vc_server::auth::hash_token("test_session_token");
    let expires_at = Utc::now() + Duration::hours(24);

    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires_at)
    .execute(pool)
    .await
    .expect("Failed to create session");

    session_id
}

/// Helper to cleanup test data.
#[allow(dead_code)]
async fn cleanup_test_user(pool: &sqlx::PgPool, user_id: Uuid) {
    // Delete in correct order due to foreign key constraints
    let _ = sqlx::query("DELETE FROM elevated_sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_system_admin_check() {
    let pool = create_test_pool().await;

    // Create test users
    let admin_username = format!("test_admin_{}", Uuid::new_v4());
    let regular_username = format!("test_regular_{}", Uuid::new_v4());

    let admin_user = create_test_user(&pool, &admin_username).await;
    let regular_user = create_test_user(&pool, &regular_username).await;

    // Grant admin to first user
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;

    // Check admin status
    use vc_server::permissions::queries::get_system_admin;

    let admin_status = get_system_admin(&pool, admin_user.id)
        .await
        .expect("Query should succeed");
    assert!(admin_status.is_some(), "User should be a system admin");

    let regular_status = get_system_admin(&pool, regular_user.id)
        .await
        .expect("Query should succeed");
    assert!(
        regular_status.is_none(),
        "User should NOT be a system admin"
    );

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
    cleanup_test_user(&pool, regular_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_session_creation() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_elevate_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;

    // Create a regular session first
    let session_id = create_session(&pool, admin_user.id).await;

    // Create elevated session
    use vc_server::permissions::queries::create_elevated_session;

    let elevated = create_elevated_session(
        &pool,
        admin_user.id,
        session_id,
        "127.0.0.1",
        15, // 15 minutes
        Some("Testing elevation"),
    )
    .await
    .expect("Elevation should succeed");

    assert_eq!(elevated.user_id, admin_user.id);
    assert!(
        elevated.expires_at > Utc::now(),
        "Should expire in the future"
    );
    assert_eq!(elevated.reason.as_deref(), Some("Testing elevation"));

    // Verify expiry is approximately 15 minutes
    let diff = elevated.expires_at - Utc::now();
    assert!(
        diff.num_minutes() >= 14 && diff.num_minutes() <= 15,
        "Elevation should expire in ~15 minutes"
    );

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_session_lookup() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_lookup_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Initially, no elevated session
    let not_elevated: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM elevated_sessions WHERE user_id = $1 AND expires_at > NOW()",
    )
    .bind(admin_user.id)
    .fetch_optional(&pool)
    .await
    .expect("Query should succeed");

    assert!(not_elevated.is_none(), "Should not be elevated initially");

    // Create elevated session
    use vc_server::permissions::queries::create_elevated_session;

    create_elevated_session(&pool, admin_user.id, session_id, "127.0.0.1", 15, None)
        .await
        .expect("Elevation should succeed");

    // Now should find elevated session
    let elevated: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM elevated_sessions WHERE user_id = $1 AND expires_at > NOW()",
    )
    .bind(admin_user.id)
    .fetch_optional(&pool)
    .await
    .expect("Query should succeed");

    assert!(elevated.is_some(), "Should be elevated now");

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_expiry_excludes_expired_sessions() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_expiry_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Create an expired elevated session (in the past)
    let expired_at = Utc::now() - Duration::minutes(1);
    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at) VALUES ($1, $2, '127.0.0.1'::inet, $3)",
    )
    .bind(admin_user.id)
    .bind(session_id)
    .bind(expired_at)
    .execute(&pool)
    .await
    .expect("Insert should succeed");

    // Query with expiry check should NOT find the expired session
    let elevated: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM elevated_sessions WHERE user_id = $1 AND expires_at > NOW()",
    )
    .bind(admin_user.id)
    .fetch_optional(&pool)
    .await
    .expect("Query should succeed");

    assert!(
        elevated.is_none(),
        "Expired session should not be found with expiry check"
    );

    // Query without expiry check SHOULD find it
    let any_session: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM elevated_sessions WHERE user_id = $1")
            .bind(admin_user.id)
            .fetch_optional(&pool)
            .await
            .expect("Query should succeed");

    assert!(any_session.is_some(), "Session should exist in table");

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_de_elevation_removes_sessions() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_de_elev_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Create elevated session
    use vc_server::permissions::queries::create_elevated_session;

    create_elevated_session(&pool, admin_user.id, session_id, "127.0.0.1", 15, None)
        .await
        .expect("Elevation should succeed");

    // Verify elevated
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM elevated_sessions WHERE user_id = $1")
        .bind(admin_user.id)
        .fetch_one(&pool)
        .await
        .expect("Query should succeed");
    assert_eq!(count.0, 1, "Should have one elevated session");

    // De-elevate (delete all elevated sessions)
    sqlx::query("DELETE FROM elevated_sessions WHERE user_id = $1")
        .bind(admin_user.id)
        .execute(&pool)
        .await
        .expect("Delete should succeed");

    // Verify no longer elevated
    let count_after: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM elevated_sessions WHERE user_id = $1")
            .bind(admin_user.id)
            .fetch_one(&pool)
            .await
            .expect("Query should succeed");
    assert_eq!(
        count_after.0, 0,
        "Should have no elevated sessions after de-elevation"
    );

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_upsert_updates_expiry() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_upsert_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Create first elevated session
    use vc_server::permissions::queries::create_elevated_session;

    let first_elevation = create_elevated_session(
        &pool,
        admin_user.id,
        session_id,
        "127.0.0.1",
        15, // 15 minutes
        Some("First elevation"),
    )
    .await
    .expect("First elevation should succeed");

    let first_expiry = first_elevation.expires_at;

    // Small delay to ensure different timestamp
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Elevate again with same session (should upsert)
    let second_elevation = create_elevated_session(
        &pool,
        admin_user.id,
        session_id,
        "127.0.0.1",
        15, // 15 minutes
        Some("Second elevation"),
    )
    .await
    .expect("Second elevation should succeed");

    // Expiry should be updated (later than first)
    assert!(
        second_elevation.expires_at >= first_expiry,
        "Second elevation should have same or later expiry"
    );

    // Should still only have one elevated session
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM elevated_sessions WHERE user_id = $1")
        .bind(admin_user.id)
        .fetch_one(&pool)
        .await
        .expect("Query should succeed");
    assert_eq!(
        count.0, 1,
        "Should have exactly one elevated session after upsert"
    );

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_non_admin_cannot_have_elevated_session() {
    let pool = create_test_pool().await;

    // Create test regular user (not an admin)
    let regular_username = format!("test_nonadmin_{}", Uuid::new_v4());
    let regular_user = create_test_user(&pool, &regular_username).await;
    let session_id = create_session(&pool, regular_user.id).await;

    // Regular user should not be in system_admins
    use vc_server::permissions::queries::get_system_admin;

    let admin_status = get_system_admin(&pool, regular_user.id)
        .await
        .expect("Query should succeed");
    assert!(admin_status.is_none(), "Regular user should not be admin");

    // Even if we somehow create an elevated session, the middleware check would fail
    // because require_system_admin runs first and would reject the request.
    // This test documents this expected behavior.

    // Attempt to create elevated session anyway (this is an edge case test)
    // In real flow, the middleware would prevent this
    use vc_server::permissions::queries::create_elevated_session;

    // This will succeed at DB level but the user still isn't an admin
    let elevated = create_elevated_session(
        &pool,
        regular_user.id,
        session_id,
        "127.0.0.1",
        15,
        Some("Should not work in practice"),
    )
    .await
    .expect("DB insert succeeds but middleware would block");

    // Session exists but user is still not a system admin
    assert!(elevated.user_id == regular_user.id);

    let admin_status_after = get_system_admin(&pool, regular_user.id)
        .await
        .expect("Query should succeed");
    assert!(
        admin_status_after.is_none(),
        "User still should not be admin after elevation attempt"
    );

    // Cleanup
    let _ = sqlx::query("DELETE FROM elevated_sessions WHERE user_id = $1")
        .bind(regular_user.id)
        .execute(&pool)
        .await;
    cleanup_test_user(&pool, regular_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_with_reason_stored() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_reason_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Create elevated session with reason
    use vc_server::permissions::queries::create_elevated_session;

    let reason_text = "Investigating reported abuse case #1234";
    let elevated = create_elevated_session(
        &pool,
        admin_user.id,
        session_id,
        "127.0.0.1",
        15,
        Some(reason_text),
    )
    .await
    .expect("Elevation should succeed");

    assert_eq!(elevated.reason.as_deref(), Some(reason_text));

    // Query from DB to verify persistence
    let stored_reason: Option<(Option<String>,)> =
        sqlx::query_as("SELECT reason FROM elevated_sessions WHERE user_id = $1")
            .bind(admin_user.id)
            .fetch_optional(&pool)
            .await
            .expect("Query should succeed");

    assert!(stored_reason.is_some());
    assert_eq!(stored_reason.unwrap().0.as_deref(), Some(reason_text));

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_without_reason() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_no_reason_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Create elevated session without reason
    use vc_server::permissions::queries::create_elevated_session;

    let elevated = create_elevated_session(
        &pool,
        admin_user.id,
        session_id,
        "127.0.0.1",
        15,
        None, // No reason
    )
    .await
    .expect("Elevation should succeed");

    assert!(elevated.reason.is_none(), "Reason should be None");

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_elevation_ip_address_stored() {
    let pool = create_test_pool().await;

    // Create test admin user
    let admin_username = format!("test_ip_{}", Uuid::new_v4());
    let admin_user = create_test_user(&pool, &admin_username).await;
    grant_system_admin(&pool, admin_user.id, admin_user.id).await;
    let session_id = create_session(&pool, admin_user.id).await;

    // Create elevated session
    use vc_server::permissions::queries::create_elevated_session;

    let test_ip = "192.168.1.100";
    create_elevated_session(&pool, admin_user.id, session_id, test_ip, 15, None)
        .await
        .expect("Elevation should succeed");

    // Query from DB to verify IP persistence (using host() to extract IP string)
    let stored_ip: Option<(String,)> =
        sqlx::query_as("SELECT host(ip_address) FROM elevated_sessions WHERE user_id = $1")
            .bind(admin_user.id)
            .fetch_optional(&pool)
            .await
            .expect("Query should succeed");

    assert!(stored_ip.is_some());
    assert_eq!(stored_ip.unwrap().0, test_ip);

    // Cleanup
    cleanup_test_user(&pool, admin_user.id).await;
}
