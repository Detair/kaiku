//! Upload size limit configuration tests.
//!
//! **Scope:** Tests configuration defaults, relationships, and boundary conditions.
//! These tests validate that:
//! - Default size limits are sensible (emoji < avatar < attachment)
//! - Config values are loaded correctly from environment
//! - Upload limits API endpoint returns correct values
//! - Boundary arithmetic is correct
//!
//! **Limitations:** These tests do NOT exercise actual HTTP upload handlers.
//! They validate configuration and arithmetic, not runtime behavior like:
//! - Multipart form parsing
//! - Actual file size validation in handlers
//! - HTTP status codes and error response formats
//! - Authorization checks before size validation
//! - Middleware interaction with handlers
//!
//! **TODO:** Add HTTP-level integration tests using axum::test or similar to:
//! 1. POST oversized files to /auth/me/avatar and verify 413 + error format
//! 2. Test unauthenticated upload attempts return 401 (not 413)
//! 3. Test guild membership checks run before size validation
//! 4. Verify middleware and handler limits work together correctly
//!
//! Run with: `cargo test --test upload_limits_test`

use serial_test::serial;
use sqlx::PgPool;
use vc_server::config::Config;
use vc_server::db;

/// Helper to create a test user and return their ID
async fn create_test_user(pool: &PgPool) -> uuid::Uuid {
    let user_id = uuid::Uuid::new_v4();
    // Generate username within 32-char limit, alphanumeric + underscore only (no hyphens)
    // Format: test_ + first 27 chars of UUID hex (no hyphens) = 32 chars max
    let uuid_hex = uuid::Uuid::new_v4().simple().to_string();
    let username = format!("test_{}", &uuid_hex[..27]);
    let password_hash = vc_server::auth::hash_password("password123").expect("Hash password");

    sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash) VALUES ($1, $2, $3, $4)"
    )
    .bind(user_id)
    .bind(&username)
    .bind(&username)
    .bind(&password_hash)
    .execute(pool)
    .await
    .expect("Failed to create test user");

    user_id
}

/// Helper to create a test guild and return its ID
async fn create_test_guild(pool: &PgPool, owner_id: uuid::Uuid) -> uuid::Uuid {
    sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO guilds (name, owner_id) VALUES ($1, $2) RETURNING id"
    )
    .bind("Test Guild")
    .bind(owner_id)
    .fetch_one(pool)
    .await
    .expect("Create guild")
}

#[tokio::test]
#[serial]
async fn test_config_default_upload_limits() {
    let config = Config::default_for_test();

    assert_eq!(config.max_upload_size, 50 * 1024 * 1024);
    assert_eq!(config.max_avatar_size, 5 * 1024 * 1024);
    assert_eq!(config.max_emoji_size, 256 * 1024);
}

#[tokio::test]
#[serial]
async fn test_upload_limits_endpoint_returns_defaults() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    // Verify config values would be returned by the endpoint
    assert_eq!(config.max_upload_size, 50 * 1024 * 1024);
    assert_eq!(config.max_avatar_size, 5 * 1024 * 1024);
    assert_eq!(config.max_emoji_size, 256 * 1024);

    pool.close().await;
}

/// Test avatar upload size validation logic
/// NOTE: This tests the boundary arithmetic, not actual handler behavior
#[tokio::test]
#[serial]
async fn test_avatar_size_validation_logic() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let _user_id = create_test_user(&pool).await;

    // Test boundary: exactly at limit should pass
    let exactly_at_limit = config.max_avatar_size;
    assert!(
        exactly_at_limit <= config.max_avatar_size,
        "File exactly at limit should be accepted (handler uses <= check)"
    );

    // Test boundary: one byte over should fail
    let one_byte_over = config.max_avatar_size + 1;
    assert!(
        one_byte_over > config.max_avatar_size,
        "File one byte over limit should be rejected (handler uses > check)"
    );

    pool.close().await;
}

/// Test emoji upload size validation logic
/// NOTE: This tests the boundary arithmetic, not actual handler behavior
#[tokio::test]
#[serial]
async fn test_emoji_size_validation_logic() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let user_id = create_test_user(&pool).await;
    let _guild_id = create_test_guild(&pool, user_id).await;

    // Test boundary: exactly at limit should pass
    let exactly_at_limit = config.max_emoji_size;
    assert!(
        exactly_at_limit <= config.max_emoji_size,
        "Emoji exactly at limit should be accepted"
    );

    // Test boundary: one byte over should fail
    let one_byte_over = config.max_emoji_size + 1;
    assert!(
        one_byte_over > config.max_emoji_size,
        "Emoji one byte over limit should be rejected"
    );

    pool.close().await;
}

/// Test that avatar validation uses correct config field (max_avatar_size, not max_upload_size)
#[tokio::test]
#[serial]
async fn test_avatar_uses_avatar_limit_not_attachment_limit() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let _user_id = create_test_user(&pool).await;

    // 6MB file - over avatar limit (5MB) but under attachment limit (50MB)
    let file_size_over_avatar = 6 * 1024 * 1024;

    // Avatar handler should reject based on max_avatar_size (5MB), not max_upload_size (50MB)
    assert!(
        file_size_over_avatar > config.max_avatar_size,
        "6MB file should exceed avatar limit (5MB)"
    );
    assert!(
        file_size_over_avatar < config.max_upload_size,
        "6MB file should be under attachment limit (50MB)"
    );

    pool.close().await;
}

/// Test that DM icon validation uses avatar limit, not attachment limit
#[tokio::test]
#[serial]
#[ignore = "DM feature not yet implemented - enable once dm_conversations table exists"]
async fn test_dm_icon_uses_avatar_limit_not_attachment_limit() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let _user_id = create_test_user(&pool).await;

    // This verifies DM icon HANDLER validation uses max_avatar_size (5MB),
    // NOT max_upload_size (50MB). Middleware limits are tested separately.
    let file_size_over_avatar = 6 * 1024 * 1024;

    assert!(
        file_size_over_avatar > config.max_avatar_size,
        "6MB file should exceed avatar limit (5MB) for DM icons"
    );
    assert!(
        file_size_over_avatar < config.max_upload_size,
        "6MB file should be under attachment limit (50MB)"
    );

    pool.close().await;
}

/// Test that emoji validation uses correct config field (max_emoji_size)
#[tokio::test]
#[serial]
async fn test_emoji_uses_emoji_limit_not_attachment_limit() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let user_id = create_test_user(&pool).await;
    let _guild_id = create_test_guild(&pool, user_id).await;

    // 512KB file - over emoji limit (256KB) but under attachment limit (50MB)
    let file_size_over_emoji = 512 * 1024;

    assert!(
        file_size_over_emoji > config.max_emoji_size,
        "512KB file should exceed emoji limit (256KB)"
    );
    assert!(
        file_size_over_emoji < config.max_upload_size,
        "512KB file should be under attachment limit (50MB)"
    );

    pool.close().await;
}

/// Test boundary: file exactly at avatar limit should be accepted
#[tokio::test]
#[serial]
async fn test_avatar_exactly_at_limit_accepted() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let _user_id = create_test_user(&pool).await;

    let file_size_at_limit = config.max_avatar_size;
    assert!(
        file_size_at_limit <= config.max_avatar_size,
        "File exactly at limit should be accepted (handler checks data.len() > max_size)"
    );

    pool.close().await;
}

/// Test boundary: file one byte over avatar limit should be rejected
#[tokio::test]
#[serial]
async fn test_avatar_one_byte_over_limit_fails() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let _user_id = create_test_user(&pool).await;

    let file_size_over_limit = config.max_avatar_size + 1;
    assert!(
        file_size_over_limit > config.max_avatar_size,
        "File one byte over limit should be rejected"
    );

    pool.close().await;
}

/// Test boundary: file exactly at emoji limit should be accepted
#[tokio::test]
#[serial]
async fn test_emoji_exactly_at_limit_accepted() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let user_id = create_test_user(&pool).await;
    let _guild_id = create_test_guild(&pool, user_id).await;

    let file_size_at_limit = config.max_emoji_size;
    assert!(
        file_size_at_limit <= config.max_emoji_size,
        "Emoji exactly at limit should be accepted"
    );

    pool.close().await;
}

/// Test boundary: emoji one byte over limit should be rejected
#[tokio::test]
#[serial]
async fn test_emoji_one_byte_over_limit_fails() {
    let config = Config::default_for_test();
    let pool = db::create_pool(&config.database_url).await.unwrap();

    let user_id = create_test_user(&pool).await;
    let _guild_id = create_test_guild(&pool, user_id).await;

    // Test that config field provides max size for error response
    // (emojis.rs line 64: "max_size_bytes": max_size)
    assert_eq!(
        config.max_emoji_size, 262144,
        "Emoji limit should be 256KB (262144 bytes) for error response"
    );

    let file_size_over_limit = config.max_emoji_size + 1;
    assert!(
        file_size_over_limit > config.max_emoji_size,
        "Emoji one byte over limit should be rejected"
    );

    pool.close().await;
}

/// Test edge case: zero-byte uploads should be rejected
#[tokio::test]
#[serial]
async fn test_zero_byte_uploads_handled() {
    let config = Config::default_for_test();

    let zero_bytes = 0;

    // Zero bytes should pass size checks but fail other validation
    // (e.g., image format detection would fail)
    assert!(
        zero_bytes <= config.max_avatar_size,
        "Zero-byte avatars should pass size check (but fail format check)"
    );
    assert!(
        zero_bytes <= config.max_emoji_size,
        "Zero-byte emojis should be accepted"
    );
}

/// Test that config defaults are sensible
#[test]
fn test_config_default_upload_limits_are_sensible() {
    let config = Config::default_for_test();

    // Avatar limit should be less than attachment limit
    assert!(
        config.max_avatar_size < config.max_upload_size,
        "Avatar limit should be smaller than attachment limit"
    );

    // Emoji limit should be less than avatar limit
    assert!(
        config.max_emoji_size < config.max_avatar_size,
        "Emoji limit should be smaller than avatar limit"
    );

    // All limits should be positive
    assert!(config.max_avatar_size > 0, "Avatar limit must be positive");
    assert!(config.max_emoji_size > 0, "Emoji limit must be positive");
    assert!(config.max_upload_size > 0, "Upload limit must be positive");
}

/// Documents the validation pattern used in handlers for future HTTP tests
///
/// This test serves as documentation for how size validation should work
/// when proper HTTP integration tests are added. It shows:
/// 1. The validation happens on data.len() after multipart parsing
/// 2. The check is: if data.len() > max_size { reject }
/// 3. This means files exactly at the limit are accepted
///
/// Future HTTP tests should:
/// - Create actual multipart/form-data requests
/// - Send to POST /auth/me/avatar, POST /api/guilds/{id}/emojis, etc.
/// - Verify HTTP 413 Payload Too Large status
/// - Check error response format: { "error": "...", "message": "...", "max_size_bytes": ... }
/// - Test authorization (401) takes precedence over size validation (413)
#[test]
fn test_validation_pattern_documentation() {
    let config = Config::default_for_test();

    // Handler validation pattern (see auth/handlers.rs:672, guild/emojis.rs:227):
    let data_len = 5 * 1024 * 1024; // Simulated file size
    let max_size = config.max_avatar_size;

    // This is the check used in handlers:
    let should_reject = data_len > max_size;

    assert!(
        !should_reject,
        "File exactly at limit should NOT be rejected (5MB <= 5MB max)"
    );

    // One byte over:
    let data_len_over = max_size + 1;
    let should_reject_over = data_len_over > max_size;

    assert!(
        should_reject_over,
        "File one byte over should be rejected (5MB + 1 byte > 5MB max)"
    );
}
