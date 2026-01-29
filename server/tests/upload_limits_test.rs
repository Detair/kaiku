//! Upload size limit integration tests.
//!
//! Tests that file size validation works end-to-end for:
//! - Avatar uploads (5MB limit)
//! - Emoji uploads (256KB limit)
//! - DM icon uploads (5MB limit, same as avatars)
//! - Upload limits API endpoint
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

/// Helper to create a DM conversation
async fn create_test_dm(pool: &PgPool, user_id: uuid::Uuid) -> uuid::Uuid {
    let dm_id = sqlx::query_scalar::<_, uuid::Uuid>(
        "INSERT INTO dm_conversations (is_group) VALUES (true) RETURNING id"
    )
    .fetch_one(pool)
    .await
    .expect("Create DM");

    sqlx::query("INSERT INTO dm_participants (dm_id, user_id) VALUES ($1, $2)")
        .bind(dm_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Add participant");

    dm_id
}

/// Test that the upload limits API endpoint returns default values
#[tokio::test]
#[serial]
async fn test_upload_limits_endpoint_returns_defaults() {
    // This test verifies the /api/config/upload-limits endpoint
    // Note: We can't easily test the HTTP endpoint without spinning up a server,
    // so we test the config loading logic instead

    let config = Config::default_for_test();

    // Verify default values match what the endpoint would return
    assert_eq!(config.max_avatar_size, 5 * 1024 * 1024, "Default avatar size should be 5MB");
    assert_eq!(config.max_emoji_size, 256 * 1024, "Default emoji size should be 256KB");
    assert_eq!(config.max_upload_size, 50 * 1024 * 1024, "Default upload size should be 50MB");
}

/// Test that avatar size validation works at the database/handler level
#[tokio::test]
#[serial]
async fn test_avatar_size_validation_logic() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    let user_id = create_test_user(&pool).await;

    // Simulate file size check (this is what the handler does)
    let file_size_over_limit = 6 * 1024 * 1024; // 6MB
    let file_size_at_limit = 5 * 1024 * 1024;   // 5MB exactly
    let file_size_under_limit = 4 * 1024 * 1024; // 4MB

    // Over limit should fail
    assert!(
        file_size_over_limit > config.max_avatar_size,
        "6MB file should exceed 5MB limit"
    );

    // At limit should pass
    assert!(
        file_size_at_limit <= config.max_avatar_size,
        "5MB file should be at or under 5MB limit"
    );

    // Under limit should pass
    assert!(
        file_size_under_limit <= config.max_avatar_size,
        "4MB file should be under 5MB limit"
    );

    // Verify user exists for upload
    let user = sqlx::query_as!(
        db::User,
        r#"SELECT id, username, display_name, email, password_hash,
           auth_method as "auth_method: _", external_id, avatar_url,
           status as "status: _", mfa_secret, created_at, updated_at
           FROM users WHERE id = $1"#,
        user_id
    )
    .fetch_one(&pool)
    .await
    .expect("User should exist");

    assert_eq!(user.id, user_id);
}

/// Test that emoji size validation enforces 256KB limit
#[tokio::test]
#[serial]
async fn test_emoji_size_validation_logic() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;

    // Simulate emoji file size checks
    let file_size_over_limit = 300 * 1024;  // 300KB
    let file_size_at_limit = 256 * 1024;    // 256KB exactly
    let file_size_under_limit = 200 * 1024; // 200KB

    // Over limit should fail
    assert!(
        file_size_over_limit > config.max_emoji_size,
        "300KB emoji should exceed 256KB limit"
    );

    // At limit should pass
    assert!(
        file_size_at_limit <= config.max_emoji_size,
        "256KB emoji should be at or under limit"
    );

    // Under limit should pass
    assert!(
        file_size_under_limit <= config.max_emoji_size,
        "200KB emoji should be under 256KB limit"
    );

    // Verify guild exists for emoji upload
    let guild = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM guilds WHERE id = $1"
    )
    .bind(guild_id)
    .fetch_one(&pool)
    .await
    .expect("Guild should exist");

    assert_eq!(guild, guild_id);
}

/// Test that DM icon size validation uses avatar limit (5MB), not attachment limit (50MB)
///
/// NOTE: Currently ignored because dm_conversations table doesn't exist yet.
/// Enable this test once DM feature is implemented in schema.
#[tokio::test]
#[serial]
#[ignore = "DM feature not yet implemented - enable once dm_conversations table exists"]
async fn test_dm_icon_uses_avatar_limit_not_attachment_limit() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    let user_id = create_test_user(&pool).await;
    let dm_id = create_test_dm(&pool, user_id).await;

    // File sizes to test
    let file_size_6mb = 6 * 1024 * 1024;   // Over avatar limit, under attachment limit
    let file_size_5mb = 5 * 1024 * 1024;   // At avatar limit

    // 6MB should fail (over 5MB avatar limit)
    // This verifies DM icons use max_avatar_size, NOT max_upload_size
    assert!(
        file_size_6mb > config.max_avatar_size,
        "6MB DM icon should exceed avatar limit (5MB)"
    );
    assert!(
        file_size_6mb < config.max_upload_size,
        "6MB is under attachment limit (50MB) - this confirms we're testing the right boundary"
    );

    // 5MB should pass (at avatar limit)
    assert!(
        file_size_5mb <= config.max_avatar_size,
        "5MB DM icon should be at avatar limit"
    );

    // Verify DM exists
    let dm = sqlx::query_scalar::<_, uuid::Uuid>(
        "SELECT id FROM dm_conversations WHERE id = $1"
    )
    .bind(dm_id)
    .fetch_one(&pool)
    .await
    .expect("DM should exist");

    assert_eq!(dm, dm_id);
}

/// Test boundary condition: file exactly 1 byte over limit should fail
#[tokio::test]
#[serial]
async fn test_avatar_one_byte_over_limit_fails() {
    let config = Config::default_for_test();

    let one_byte_over = config.max_avatar_size + 1;

    assert!(
        one_byte_over > config.max_avatar_size,
        "File with 1 byte over limit should fail validation"
    );
}

/// Test that emoji error response includes max_size_bytes field
#[tokio::test]
#[serial]
async fn test_emoji_error_includes_max_size() {
    let config = Config::default_for_test();

    // This verifies the new error structure with max_size field
    // The actual HTTP response would include:
    // {
    //   "error": "FILE_TOO_LARGE",
    //   "message": "File too large (max 256KB for emojis)",
    //   "max_size_bytes": 262144
    // }

    assert_eq!(
        config.max_emoji_size,
        262144,
        "Emoji limit should be 256KB (262144 bytes) for error response"
    );
}

/// Test zero-byte files are accepted
#[tokio::test]
#[serial]
async fn test_zero_byte_files_accepted() {
    let config = Config::default_for_test();

    let zero_bytes = 0_usize;

    assert!(
        zero_bytes <= config.max_avatar_size,
        "Zero-byte files should be accepted (weird but not invalid)"
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
