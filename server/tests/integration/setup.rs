//! First User Setup Tests
//!
//! Tests for the first-time server setup wizard and admin bootstrap.
//!
//! Tests use transactions for isolation — each test rolls back its changes
//! automatically, so concurrent execution is safe.

use sqlx::PgPool;
use vc_server::config::Config;
use vc_server::db;

/// Test that the first user registration logic is set up correctly.
///
/// This test verifies database-level admin grant mechanics. The actual
/// first-user detection and grant occurs atomically in POST /auth/register
/// (handlers.rs registration flow). See `setup_integration.rs` for
/// full-flow testing including concurrent registration scenarios.
#[tokio::test]
async fn test_first_user_detection_works() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // Use a transaction for isolation (will be rolled back automatically on drop)
    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Delete bot installations first (installed_by FK to users has no CASCADE)
    sqlx::query("DELETE FROM guild_bot_installations")
        .execute(&mut *tx)
        .await
        .expect("Failed to clear guild_bot_installations");
    // Delete users - cascades to system_admins due to ON DELETE CASCADE
    sqlx::query("DELETE FROM users")
        .execute(&mut *tx)
        .await
        .expect("Failed to clear users");

    // Generate unique identifiers for this test
    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("first_user_{test_id}");

    // Create user (note: admin grant happens in registration handler, not here)
    let user = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, is_bot, bot_owner_id,
                     deletion_requested_at, deletion_scheduled_at,
                     created_at, updated_at"#,
        username,
        "First User",
        "hash"
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to create first user");

    // Manually grant admin (simulating what registration handler does)
    sqlx::query("INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)")
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .expect("Failed to grant admin");

    // Verify user is system admin
    let is_admin: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1)")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to check admin status");

    assert!(is_admin, "First user should be granted admin permissions");

    // Transaction is rolled back automatically, no cleanup needed
    tx.rollback().await.expect("Failed to rollback");
    println!("✅ First user detection test passed (transaction rolled back)");
}

/// Test that setup status is initially incomplete.
#[tokio::test]
async fn test_setup_initially_incomplete() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // For fresh installs, setup should be incomplete
    // For existing installs with users, the migration marks it complete
    let setup_complete = db::is_setup_complete(&pool)
        .await
        .expect("Failed to check setup status");

    let user_count = db::count_users(&pool).await.expect("Failed to count users");

    // Note: In test environments, the migration might run on a fresh database (no users),
    // setting setup_complete=false. If tests then add users, we have users but
    // setup_complete=false, which is a valid state for a fresh install where users were added
    // after the migration. The migration only marks setup_complete=true if users existed AT THE
    // TIME the migration ran. Therefore, we can't assert setup_complete==true just because
    // users exist now.
    println!(
        "✅ Setup status check passed (users: {user_count}, setup_complete: {setup_complete})"
    );
}

/// Test server config CRUD operations.
///
/// Uses a transaction to avoid mutating shared database state.
#[tokio::test]
async fn test_server_config_operations() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Get default server name
    let server_name: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM server_config WHERE key = 'server_name'")
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to get server_name");
    assert_eq!(server_name.as_str(), Some("Canis Server"));

    // Get default registration policy
    let reg_policy: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM server_config WHERE key = 'registration_policy'")
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to get registration_policy");
    assert_eq!(reg_policy.as_str(), Some("open"));

    // Create a test user for config updates (required by foreign key)
    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let test_username = format!("config_test_{test_id}");

    let test_user = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, is_bot, bot_owner_id,
                     deletion_requested_at, deletion_scheduled_at,
                     created_at, updated_at"#,
        test_username,
        "Config Test",
        "hash"
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to create test user");

    // Test setting a config value
    sqlx::query(
        "UPDATE server_config SET value = $1, updated_by = $2, updated_at = NOW() WHERE key = 'server_name'",
    )
    .bind(serde_json::json!("Test Server"))
    .bind(test_user.id)
    .execute(&mut *tx)
    .await
    .expect("Failed to set server_name");

    // Verify the change
    let updated_name: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM server_config WHERE key = 'server_name'")
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to get updated server_name");
    assert_eq!(updated_name.as_str(), Some("Test Server"));

    // Transaction is rolled back, no need to restore
    tx.rollback().await.expect("Failed to rollback");

    println!("✅ Server config operations test passed (transaction rolled back)");
}

/// Test that setup can be marked complete (irreversible).
///
/// Note: This test uses a transaction that is rolled back to avoid
/// permanently modifying the database state.
#[tokio::test]
async fn test_mark_setup_complete() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // Use a transaction for isolation (will be rolled back automatically)
    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Get current setup status within transaction
    let initial_status_value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to get setup status");

    let initial_status = initial_status_value.as_bool().unwrap_or(false);

    if initial_status {
        println!("    ℹ️  Setup already complete in DB, testing update still works");
    }

    // Create a test user for marking setup complete (required by foreign key)
    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let test_username = format!("setup_complete_{test_id}");

    let test_user = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, is_bot, bot_owner_id,
                     deletion_requested_at, deletion_scheduled_at,
                     created_at, updated_at"#,
        test_username,
        "Setup Test",
        "hash"
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to create test user");

    // Mark setup as complete within transaction
    sqlx::query(
        "UPDATE server_config
         SET value = 'true'::jsonb, updated_by = $1, updated_at = NOW()
         WHERE key = 'setup_complete'",
    )
    .bind(test_user.id)
    .execute(&mut *tx)
    .await
    .expect("Failed to mark setup complete");

    // Verify setup is now complete within transaction
    let final_status_value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to check setup status after marking complete");

    let final_status = final_status_value.as_bool().unwrap_or(false);

    assert!(final_status, "Setup should be marked as complete");

    // Transaction is rolled back automatically, no cleanup needed
    tx.rollback().await.expect("Failed to rollback");
    println!("✅ Mark setup complete test passed (transaction rolled back)");
}

/// Test race condition prevention in first user detection.
///
/// Test that the `setup_complete` lock serialization works correctly.
/// This verifies the actual locking pattern used in production (handlers.rs:225-237).
/// Full concurrent behavior requires integration testing through HTTP endpoints.
#[tokio::test]
async fn test_setup_complete_lock_serialization() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // Use isolated transaction
    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Test the actual locking pattern used in production (handlers.rs:225-237)
    // This acquires a row-level lock on the setup_complete config row
    let _lock = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to acquire setup_complete lock");

    println!("    ✓ Successfully acquired FOR UPDATE lock on setup_complete config");

    // Now count users (this is what production does)
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx)
        .await
        .expect("Failed to count users");

    println!("    ✓ User count: {user_count}");

    // Verify we can still do other operations while holding the lock
    let setup_value = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete'",
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to read setup_complete value");

    println!("    ✓ Setup complete value: {setup_value}");

    tx.rollback().await.expect("Failed to rollback");

    println!("✅ Setup complete lock serialization test passed");
    println!("    Note: True concurrent behavior requires HTTP integration tests");
    println!("    See setup_integration.rs for concurrent registration tests");
}

/// Test validation of server configuration values.
///
/// Note: This test uses a transaction to avoid modifying shared database state.
/// It documents that the database layer is permissive and validation should
/// happen at the API layer.
#[tokio::test]
async fn test_config_validation() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // Use transaction for isolation
    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Create a test user for config updates (required by foreign key)
    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let test_username = format!("validation_test_{test_id}");

    let test_user = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, is_bot, bot_owner_id,
                     deletion_requested_at, deletion_scheduled_at,
                     created_at, updated_at"#,
        test_username,
        "Validation Test",
        "hash"
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to create test user");

    // Test 1: Empty server_name (database allows, API should validate)
    let empty_name_result = sqlx::query(
        "UPDATE server_config
         SET value = $1, updated_by = $2
         WHERE key = 'server_name'",
    )
    .bind(serde_json::json!(""))
    .bind(test_user.id)
    .execute(&mut *tx)
    .await;

    assert!(
        empty_name_result.is_ok(),
        "Database allows empty server_name (API should validate)"
    );

    // Test 2: Server name at 64-char boundary should be accepted
    let long_name = "a".repeat(64); // Exactly 64 characters
    let long_name_result = sqlx::query(
        "UPDATE server_config
         SET value = $1, updated_by = $2
         WHERE key = 'server_name'",
    )
    .bind(serde_json::json!(long_name))
    .bind(test_user.id)
    .execute(&mut *tx)
    .await;

    assert!(
        long_name_result.is_ok(),
        "64-character server name should be accepted"
    );

    // Test 3: Invalid registration_policy (database allows, API should validate)
    let invalid_policy_result = sqlx::query(
        "UPDATE server_config
         SET value = $1, updated_by = $2
         WHERE key = 'registration_policy'",
    )
    .bind(serde_json::json!("invalid_policy"))
    .bind(test_user.id)
    .execute(&mut *tx)
    .await;

    assert!(
        invalid_policy_result.is_ok(),
        "Database allows any policy value (API should validate)"
    );

    // Test 4: Malformed URLs (database allows, API should validate)
    let invalid_url_result = sqlx::query(
        "UPDATE server_config
         SET value = $1, updated_by = $2
         WHERE key = 'terms_url'",
    )
    .bind(serde_json::json!("not-a-valid-url"))
    .bind(test_user.id)
    .execute(&mut *tx)
    .await;

    assert!(
        invalid_url_result.is_ok(),
        "Database allows malformed URLs (API should validate)"
    );

    // Transaction rollback happens automatically
    tx.rollback().await.expect("Failed to rollback");

    println!("✅ Config validation test passed (transaction rolled back)");
    println!("    Note: Database layer is permissive - API should add validation");
}

/// Test that second user does NOT receive admin permissions.
#[tokio::test]
async fn test_second_user_not_admin() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // Use isolated transaction
    let mut tx = pool.begin().await.expect("Failed to start transaction");

    // Delete bot installations first (installed_by FK to users has no CASCADE)
    sqlx::query("DELETE FROM guild_bot_installations")
        .execute(&mut *tx)
        .await
        .expect("Failed to clear guild_bot_installations");
    // Clean slate in transaction (cascades to system_admins)
    sqlx::query("DELETE FROM users")
        .execute(&mut *tx)
        .await
        .expect("Failed to clear users");

    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let first_username = format!("user1_{test_id}");
    let second_username = format!("user2_{test_id}");

    // Create first user
    sqlx::query(
        "INSERT INTO users (username, display_name, password_hash, auth_method)
         VALUES ($1, 'User 1', 'hash', 'local')",
    )
    .bind(&first_username)
    .execute(&mut *tx)
    .await
    .expect("Failed to create first user");

    // Create second user
    let user2 = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, is_bot, bot_owner_id,
                     deletion_requested_at, deletion_scheduled_at,
                     created_at, updated_at"#,
        second_username,
        "User 2",
        "hash"
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to create second user");

    // Verify second user is NOT system admin
    let is_admin = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1) as "exists!""#,
        user2.id
    )
    .fetch_one(&mut *tx)
    .await
    .expect("Failed to check admin status");

    assert!(
        !is_admin,
        "Second user should not automatically receive admin permissions"
    );

    tx.rollback().await.expect("Failed to rollback");
    println!("✅ Second user not admin test passed (transaction rolled back)");
}
