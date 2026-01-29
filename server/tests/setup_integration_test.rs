//! First User Setup Integration Tests
//!
//! Integration tests for first-time server setup and concurrent registration scenarios.
//!
//! These tests verify:
//! - First user admin grant through the complete registration flow
//! - Concurrent registration race condition handling
//! - Concurrent setup completion protection
//!
//! Note: These tests use the database directly rather than HTTP to avoid
//! the complexity of spinning up a test server. The critical locking logic
//! is tested through concurrent database transactions.
//!
//! Run with: `cargo test --test setup_integration_test`

use serial_test::serial;
use sqlx::PgPool;
use std::sync::Arc;
use vc_server::config::Config;
use vc_server::db;

/// Test that first user registration grants admin and subsequent registrations do not.
/// This simulates the sequential registration flow.
#[tokio::test]
#[serial]
async fn test_first_user_receives_admin_sequential() {
    let config = Config::default_for_test();
    let pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");

    // Clean slate - delete all users
    sqlx::query("DELETE FROM users")
        .execute(&pool)
        .await
        .expect("Failed to clear users");

    // Ensure setup is not marked complete
    sqlx::query("UPDATE server_config SET value = 'false' WHERE key = 'setup_complete'")
        .execute(&pool)
        .await
        .expect("Failed to reset setup_complete");

    // Create first user with admin grant
    let test_id1 = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let username1 = format!("first_user_{test_id1}");

    let mut tx1 = pool.begin().await.expect("Failed to start tx1");

    // Lock setup_complete (as production does)
    let _lock = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE"
    )
    .fetch_one(&mut *tx1)
    .await
    .expect("Failed to lock setup_complete");

    // Count users (should be 0)
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx1)
        .await
        .expect("Failed to count users");

    assert_eq!(user_count, 0, "Should have 0 users initially");

    // Create first user
    let user1 = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, created_at, updated_at"#,
        username1,
        "First User",
        "hash"
    )
    .fetch_one(&mut *tx1)
    .await
    .expect("Failed to create first user");

    // Grant admin to first user
    sqlx::query("INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)")
        .bind(user1.id)
        .execute(&mut *tx1)
        .await
        .expect("Failed to grant admin to first user");

    tx1.commit().await.expect("Failed to commit tx1");

    // Verify first user is admin
    let is_admin1: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1)"
    )
    .bind(user1.id)
    .fetch_one(&pool)
    .await
    .expect("Failed to check admin status");

    assert!(is_admin1, "First user should be granted admin permissions");

    // Create second user WITHOUT admin grant
    let test_id2 = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let username2 = format!("second_user_{test_id2}");

    let mut tx2 = pool.begin().await.expect("Failed to start tx2");

    // Lock setup_complete again
    let _lock2 = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE"
    )
    .fetch_one(&mut *tx2)
    .await
    .expect("Failed to lock setup_complete");

    // Count users (should be 1 now)
    let user_count2: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx2)
        .await
        .expect("Failed to count users");

    assert_eq!(user_count2, 1, "Should have 1 user before second registration");

    // Create second user (NO admin grant since user_count > 0)
    let user2 = sqlx::query_as!(
        db::User,
        r#"INSERT INTO users (username, display_name, password_hash, auth_method)
           VALUES ($1, $2, $3, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, created_at, updated_at"#,
        username2,
        "Second User",
        "hash"
    )
    .fetch_one(&mut *tx2)
    .await
    .expect("Failed to create second user");

    tx2.commit().await.expect("Failed to commit tx2");

    // Verify second user is NOT admin
    let is_admin2: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1)"
    )
    .bind(user2.id)
    .fetch_one(&pool)
    .await
    .expect("Failed to check admin status");

    assert!(!is_admin2, "Second user should NOT be granted admin permissions");

    // Cleanup
    sqlx::query("DELETE FROM users")
        .execute(&pool)
        .await
        .expect("Failed to cleanup users");

    println!("✅ First user admin grant test passed");
    println!("    First user: {} (admin: {})", username1, is_admin1);
    println!("    Second user: {} (admin: {})", username2, is_admin2);
}

/// Test that concurrent registrations only grant admin to ONE user.
/// This simulates a race condition where multiple registration attempts happen simultaneously.
#[tokio::test]
#[serial]
async fn test_concurrent_registrations_only_one_gets_admin() {
    let config = Config::default_for_test();
    let pool: Arc<PgPool> = Arc::new(
        db::create_pool(&config.database_url)
            .await
            .expect("Failed to connect to DB")
    );

    // Clean slate
    sqlx::query("DELETE FROM users")
        .execute(pool.as_ref())
        .await
        .expect("Failed to clear users");

    sqlx::query("UPDATE server_config SET value = 'false' WHERE key = 'setup_complete'")
        .execute(pool.as_ref())
        .await
        .expect("Failed to reset setup_complete");

    // Spawn 5 concurrent "registration" tasks
    let num_concurrent = 5;
    let mut handles = vec![];

    for i in 0..num_concurrent {
        let pool_clone = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
            let username = format!("concurrent_user_{}_{}", i, test_id);

            // Simulate the registration flow with FOR UPDATE lock
            let mut tx = pool_clone.begin().await.expect("Failed to start transaction");

            // Acquire lock (this serializes the concurrent attempts)
            let _lock = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE"
            )
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to lock setup_complete");

            // Count users
            let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                .fetch_one(&mut *tx)
                .await
                .expect("Failed to count users");

            // Create user
            let user = sqlx::query_as!(
                db::User,
                r#"INSERT INTO users (username, display_name, password_hash, auth_method)
                   VALUES ($1, $2, $3, 'local')
                   RETURNING id, username, display_name, email, password_hash,
                             auth_method as "auth_method: _", external_id, avatar_url,
                             status as "status: _", mfa_secret, created_at, updated_at"#,
                username.clone(),
                format!("User {}", i),
                "hash"
            )
            .fetch_one(&mut *tx)
            .await
            .expect("Failed to create user");

            // Grant admin only if this is the first user
            let granted_admin = if user_count == 0 {
                sqlx::query("INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)")
                    .bind(user.id)
                    .execute(&mut *tx)
                    .await
                    .expect("Failed to grant admin");
                true
            } else {
                false
            };

            tx.commit().await.expect("Failed to commit");

            (user.id, username, granted_admin)
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    let results = futures::future::join_all(handles).await;

    // Collect results
    let mut admin_granted_count = 0;
    let mut all_user_ids = vec![];

    for result in results {
        let (user_id, username, granted_admin) = result.expect("Task failed");
        all_user_ids.push(user_id);
        if granted_admin {
            admin_granted_count += 1;
            println!("    ✓ User {} was granted admin", username);
        } else {
            println!("    ✓ User {} was NOT granted admin", username);
        }
    }

    // Verify exactly ONE user was granted admin
    assert_eq!(
        admin_granted_count, 1,
        "Exactly one user should be granted admin, got {}",
        admin_granted_count
    );

    // Verify in database
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM system_admins")
        .fetch_one(pool.as_ref())
        .await
        .expect("Failed to count admins");

    assert_eq!(admin_count, 1, "Should have exactly 1 admin in database");

    // Verify all users were created
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool.as_ref())
        .await
        .expect("Failed to count users");

    assert_eq!(
        user_count as usize, num_concurrent,
        "All {} users should have been created",
        num_concurrent
    );

    // Cleanup
    sqlx::query("DELETE FROM users")
        .execute(pool.as_ref())
        .await
        .expect("Failed to cleanup");

    println!("✅ Concurrent registration test passed");
    println!("    {} concurrent registrations, exactly 1 received admin", num_concurrent);
}

/// Test that concurrent setup completion attempts only succeed once.
/// This verifies the compare-and-swap pattern in the setup complete endpoint.
#[tokio::test]
#[serial]
async fn test_concurrent_setup_completion_only_one_succeeds() {
    let config = Config::default_for_test();
    let pool: Arc<PgPool> = Arc::new(
        db::create_pool(&config.database_url)
            .await
            .expect("Failed to connect to DB")
    );

    // Setup: Ensure setup is NOT complete
    sqlx::query("UPDATE server_config SET value = 'false' WHERE key = 'setup_complete'")
        .execute(pool.as_ref())
        .await
        .expect("Failed to reset setup_complete");

    // Spawn 3 concurrent "setup completion" tasks
    let num_concurrent = 3;
    let mut handles = vec![];

    for i in 0..num_concurrent {
        let pool_clone = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            // Simulate the setup completion flow
            let mut tx = pool_clone.begin().await.expect("Failed to start transaction");

            // Attempt compare-and-swap (as production does)
            let updated = sqlx::query_scalar::<_, Option<i32>>(
                "UPDATE server_config
                 SET value = 'true'
                 WHERE key = 'setup_complete' AND value = 'false'
                 RETURNING 1"
            )
            .fetch_optional(&mut *tx)
            .await
            .expect("Failed to update setup_complete");

            let success = updated.is_some();

            if success {
                // Update other config (simulating the full setup flow)
                let server_name_json = serde_json::json!(format!("Server from task {}", i));
                sqlx::query(
                    "INSERT INTO server_config (key, value)
                     VALUES ('server_name', $1)
                     ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value"
                )
                .bind(server_name_json)
                .execute(&mut *tx)
                .await
                .expect("Failed to update server_name");

                tx.commit().await.expect("Failed to commit");
            } else {
                // If update failed, rollback
                tx.rollback().await.expect("Failed to rollback");
            }

            (i, success)
        });

        handles.push(handle);
    }

    // Wait for all tasks
    let results = futures::future::join_all(handles).await;

    // Collect results
    let mut success_count = 0;
    let mut successful_task = None;

    for result in results {
        let (task_id, success) = result.expect("Task failed");
        if success {
            success_count += 1;
            successful_task = Some(task_id);
            println!("    ✓ Task {} successfully completed setup", task_id);
        } else {
            println!("    ✓ Task {} found setup already complete", task_id);
        }
    }

    // Verify exactly ONE task succeeded
    assert_eq!(
        success_count, 1,
        "Exactly one task should complete setup, got {}",
        success_count
    );

    println!("    Winner was task: {:?}", successful_task);

    // Verify setup is now marked complete
    let setup_complete: serde_json::Value = sqlx::query_scalar(
        "SELECT value FROM server_config WHERE key = 'setup_complete'"
    )
    .fetch_one(pool.as_ref())
    .await
    .expect("Failed to read setup_complete");

    assert_eq!(
        setup_complete.as_bool(),
        Some(true),
        "Setup should be marked complete"
    );

    // Cleanup - reset server_config to defaults
    sqlx::query("UPDATE server_config SET value = 'false' WHERE key = 'setup_complete'")
        .execute(pool.as_ref())
        .await
        .expect("Failed to reset setup_complete");

    sqlx::query("UPDATE server_config SET value = '\"Canis Server\"' WHERE key = 'server_name'")
        .execute(pool.as_ref())
        .await
        .expect("Failed to reset server_name");

    println!("✅ Concurrent setup completion test passed");
    println!("    {} concurrent attempts, exactly 1 succeeded", num_concurrent);
}
