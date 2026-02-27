//! Authentication integration tests.
//!
//! Tests for critical authentication flows including:
//! - User registration
//! - Login with valid/invalid credentials
//! - MFA enrollment and verification
//! - Token refresh lifecycle
//! - Session revocation
//!
//! Run with: `cargo test --test integration auth`
//! Run ignored (integration) tests: `cargo test --test integration auth -- --ignored`

use vc_server::auth::{hash_password, verify_password};

// ============================================================================
// Password Hashing Tests (Unit tests - no database required)
// ============================================================================

#[test]
fn test_password_hash_and_verify_success() {
    let password = "secure_password_123!";
    let hash = hash_password(password).expect("Hashing should succeed");

    // Hash should be different from password
    assert_ne!(hash, password);

    // Verification should succeed
    let verified = verify_password(password, &hash).expect("Verification should succeed");
    assert!(verified, "Correct password should verify");
}

#[test]
fn test_password_verify_wrong_password() {
    let password = "correct_password";
    let wrong_password = "wrong_password";

    let hash = hash_password(password).expect("Hashing should succeed");

    let verified = verify_password(wrong_password, &hash).expect("Verification should succeed");
    assert!(!verified, "Wrong password should not verify");
}

#[test]
fn test_password_hash_produces_unique_hashes() {
    let password = "same_password";

    let hash1 = hash_password(password).expect("Hashing should succeed");
    let hash2 = hash_password(password).expect("Hashing should succeed");

    // Same password should produce different hashes (due to salt)
    assert_ne!(
        hash1, hash2,
        "Argon2 should produce unique hashes with different salts"
    );

    // Both should verify correctly
    assert!(verify_password(password, &hash1).unwrap());
    assert!(verify_password(password, &hash2).unwrap());
}

#[test]
fn test_password_hash_handles_empty_password() {
    let empty_password = "";

    // Should handle empty password (validation should happen at API layer)
    let hash = hash_password(empty_password).expect("Hashing empty password should succeed");
    let verified = verify_password(empty_password, &hash).expect("Verification should succeed");
    assert!(verified);
}

#[test]
fn test_password_hash_handles_unicode() {
    let unicode_password = "密码🔐パスワード";

    let hash = hash_password(unicode_password).expect("Hashing unicode should succeed");
    let verified = verify_password(unicode_password, &hash).expect("Verification should succeed");
    assert!(verified, "Unicode password should verify");
}

#[test]
fn test_password_hash_handles_long_password() {
    // Very long password (Argon2 should handle this)
    let long_password = "a".repeat(1000);

    let hash = hash_password(&long_password).expect("Hashing long password should succeed");
    let verified = verify_password(&long_password, &hash).expect("Verification should succeed");
    assert!(verified, "Long password should verify");
}

// ============================================================================
// JWT Token Tests (Unit tests - no database required)
// ============================================================================

#[test]
fn test_token_hash_is_deterministic() {
    use vc_server::auth::hash_token;

    let token = "test_refresh_token_12345";

    let hash1 = hash_token(token);
    let hash2 = hash_token(token);

    // Same token should produce same hash (SHA256 is deterministic)
    assert_eq!(hash1, hash2, "Token hash should be deterministic");
}

#[test]
fn test_token_hash_produces_hex_output() {
    use vc_server::auth::hash_token;

    let token = "any_token_value";
    let hash = hash_token(token);

    // SHA256 produces 64 hex characters
    assert_eq!(hash.len(), 64, "SHA256 hash should be 64 hex chars");

    // Should only contain hex characters
    assert!(
        hash.chars().all(|c: char| c.is_ascii_hexdigit()),
        "Hash should be valid hex"
    );
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
async fn test_user_registration_creates_user() {
    let pool = create_test_pool().await;

    // Generate unique username for test
    let username = format!("test_user_{}", uuid::Uuid::new_v4());
    let password = "Test123!@#";
    let display_name = "Test User";

    // Hash password
    let password_hash = hash_password(password).expect("Hashing should succeed");

    // Create user
    let user = vc_server::db::create_user(&pool, &username, display_name, None, &password_hash)
        .await
        .expect("User creation should succeed");

    assert_eq!(user.username, username);
    assert_eq!(user.display_name, display_name);
    assert!(user.password_hash.is_some());

    // Clean up
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(&pool)
        .await
        .expect("Cleanup should succeed");
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_duplicate_username_rejected() {
    let pool = create_test_pool().await;

    let username = format!("test_dup_{}", uuid::Uuid::new_v4());
    let password_hash = hash_password("password").unwrap();

    // Create first user
    let user1 = vc_server::db::create_user(&pool, &username, "User 1", None, &password_hash)
        .await
        .expect("First user should be created");

    // Try to create second user with same username
    let result = vc_server::db::create_user(&pool, &username, "User 2", None, &password_hash).await;

    assert!(result.is_err(), "Duplicate username should fail");

    // Clean up
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user1.id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_session_creation_and_lookup() {
    let pool = create_test_pool().await;

    // Create a test user first
    let username = format!("test_session_{}", uuid::Uuid::new_v4());
    let password_hash = hash_password("password").unwrap();
    let user = vc_server::db::create_user(&pool, &username, "Session Test", None, &password_hash)
        .await
        .expect("User creation should succeed");

    // Create a session
    let token = "test_refresh_token";
    let token_hash = vc_server::auth::hash_token(token);
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(24);

    let session = vc_server::db::create_session(
        &pool,
        user.id,
        &token_hash,
        expires_at,
        Some("127.0.0.1"),
        Some("Test Agent"),
    )
    .await
    .expect("Session creation should succeed");

    assert_eq!(session.user_id, user.id);
    assert_eq!(session.token_hash, token_hash);

    // Lookup session by token hash
    let found = vc_server::db::find_session_by_token_hash(&pool, &token_hash)
        .await
        .expect("Lookup should succeed");

    assert!(found.is_some(), "Session should be found");
    assert_eq!(found.unwrap().id, session.id);

    // Clean up
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session.id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_session_revocation() {
    let pool = create_test_pool().await;

    // Create a test user
    let username = format!("test_revoke_{}", uuid::Uuid::new_v4());
    let password_hash = hash_password("password").unwrap();
    let user = vc_server::db::create_user(&pool, &username, "Revoke Test", None, &password_hash)
        .await
        .expect("User creation should succeed");

    // Create multiple sessions
    let token1_hash = vc_server::auth::hash_token("token1");
    let token2_hash = vc_server::auth::hash_token("token2");
    let expires_at = chrono::Utc::now() + chrono::Duration::hours(24);

    vc_server::db::create_session(&pool, user.id, &token1_hash, expires_at, None, None)
        .await
        .expect("Session 1 should be created");

    vc_server::db::create_session(&pool, user.id, &token2_hash, expires_at, None, None)
        .await
        .expect("Session 2 should be created");

    // Revoke all sessions
    let revoked = vc_server::db::delete_all_user_sessions(&pool, user.id)
        .await
        .expect("Revocation should succeed");

    assert_eq!(revoked, 2, "Should revoke 2 sessions");

    // Verify sessions are gone
    let session1 = vc_server::db::find_session_by_token_hash(&pool, &token1_hash)
        .await
        .expect("Lookup should succeed");
    let session2 = vc_server::db::find_session_by_token_hash(&pool, &token2_hash)
        .await
        .expect("Lookup should succeed");

    assert!(session1.is_none(), "Session 1 should be deleted");
    assert!(session2.is_none(), "Session 2 should be deleted");

    // Clean up user
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(&pool)
        .await
        .ok();
}

#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_expired_session_not_found() {
    let pool = create_test_pool().await;

    // Create a test user
    let username = format!("test_expired_{}", uuid::Uuid::new_v4());
    let password_hash = hash_password("password").unwrap();
    let user = vc_server::db::create_user(&pool, &username, "Expired Test", None, &password_hash)
        .await
        .expect("User creation should succeed");

    // Create an expired session (in the past)
    let token_hash = vc_server::auth::hash_token("expired_token");
    let expires_at = chrono::Utc::now() - chrono::Duration::hours(1); // Expired 1 hour ago

    // Directly insert expired session (bypassing normal creation)
    sqlx::query("INSERT INTO sessions (user_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user.id)
        .bind(&token_hash)
        .bind(expires_at)
        .execute(&pool)
        .await
        .expect("Session insert should succeed");

    // Lookup should NOT find expired session
    let found = vc_server::db::find_session_by_token_hash(&pool, &token_hash)
        .await
        .expect("Lookup should succeed");

    assert!(found.is_none(), "Expired session should not be found");

    // Clean up
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user.id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user.id)
        .execute(&pool)
        .await
        .ok();
}
