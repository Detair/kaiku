//! HTTP-Level Concurrent Setup Completion Test
//!
//! Tests that concurrent HTTP requests to `POST /api/setup/complete`
//! result in exactly one success (204) while others receive 403.
//!
//! This extends the database-level concurrency tests in
//! `setup_integration_test.rs` by exercising the full HTTP stack:
//! auth middleware, JSON deserialization, validation, compare-and-swap,
//! and response serialization — all under concurrent load.
//!
//! Run with: `cargo test --test setup_concurrent_http_test -- --nocapture`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{create_test_user, generate_access_token, make_admin, TestApp};
use serial_test::serial;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tower::ServiceExt;
use vc_server::api::{create_router, AppState};
use vc_server::voice::sfu::SfuServer;

/// Create a TestApp with a fresh pool (not shared via OnceCell).
///
/// The shared test pool can hold stale connections after prior test runtimes
/// shut down, which intermittently causes PoolTimedOut in this suite.
async fn setup_test_app() -> TestApp {
    let config = helpers::shared_config().await.clone();
    let pool = vc_server::db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to test DB");
    let redis = vc_server::db::create_redis_client(&config.redis_url)
        .await
        .expect("Failed to connect to test Redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

    let state = AppState::new(
        pool.clone(),
        redis,
        config.clone(),
        None,
        sfu,
        None,
        None,
        None,
    );
    let router = create_router(state);

    TestApp {
        router,
        pool,
        config: Arc::new(config),
    }
}

/// Set `setup_complete` to the given value and return the previous value.
async fn set_setup_complete(pool: &sqlx::PgPool, complete: bool) -> bool {
    let prev: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM server_config WHERE key = 'setup_complete'")
            .fetch_one(pool)
            .await
            .expect("Failed to read setup_complete");

    sqlx::query("UPDATE server_config SET value = $1::jsonb WHERE key = 'setup_complete'")
        .bind(serde_json::json!(complete))
        .execute(pool)
        .await
        .expect("Failed to set setup_complete");

    prev.as_bool().unwrap_or_else(|| {
        panic!("setup_complete has invalid type in database, expected boolean, got: {prev:?}")
    })
}

/// Test that concurrent HTTP setup completion requests result in exactly one 204.
///
/// Two admin users simultaneously POST to `/api/setup/complete`.
/// The compare-and-swap pattern ensures exactly one succeeds (204),
/// while the other gets 403 (SETUP_ALREADY_COMPLETE).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_concurrent_http_setup_completion() {
    let app = setup_test_app().await;

    // Create two separate admin users
    let (admin1_id, _) = create_test_user(&app.pool).await;
    let (admin2_id, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin1_id).await;
    make_admin(&app.pool, admin2_id).await;

    let token1 = generate_access_token(&app.config, admin1_id);
    let token2 = generate_access_token(&app.config, admin2_id);

    // Reset setup to incomplete
    let prev = set_setup_complete(&app.pool, false).await;

    // Guard ensures cleanup even if assertions panic
    let mut guard = app.cleanup_guard();
    guard.restore_config_defaults();
    guard.restore_setup_complete(prev);
    guard.delete_user(admin1_id);
    guard.delete_user(admin2_id);

    // Build two concurrent completion requests
    let req1 = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token1}"))
        .body(Body::from(
            serde_json::json!({
                "server_name": "Server from Admin 1",
                "registration_policy": "open"
            })
            .to_string(),
        ))
        .unwrap();

    let req2 = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token2}"))
        .body(Body::from(
            serde_json::json!({
                "server_name": "Server from Admin 2",
                "registration_policy": "invite_only"
            })
            .to_string(),
        ))
        .unwrap();

    // Send both requests concurrently through the full HTTP stack
    let router1 = app.router.clone();
    let router2 = app.router.clone();
    let (resp1, resp2) = timeout(Duration::from_secs(30), async {
        tokio::join!(router1.oneshot(req1), router2.oneshot(req2),)
    })
    .await
    .expect("Concurrent setup completion requests timed out");

    let s1 = resp1.expect("Request 1 failed").status();
    let s2 = resp2.expect("Request 2 failed").status();

    // Exactly one should succeed (204), the other should get 403
    assert!(
        (s1 == 204 && s2 == 403) || (s1 == 403 && s2 == 204),
        "Expected one 204 and one 403, got {s1} and {s2}"
    );

    // Verify setup is marked complete
    let setup_val: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM server_config WHERE key = 'setup_complete'")
            .fetch_one(&app.pool)
            .await
            .expect("Failed to read setup_complete");
    assert_eq!(
        setup_val.as_bool(),
        Some(true),
        "setup_complete should be true after concurrent completion"
    );
}

/// Test that concurrent completion with 5 admins still results in exactly one success.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_concurrent_http_setup_five_admins() {
    let app = setup_test_app().await;

    let num_admins = 5;
    let mut admin_ids = Vec::new();
    let mut tokens = Vec::new();

    for _ in 0..num_admins {
        let (user_id, _) = create_test_user(&app.pool).await;
        make_admin(&app.pool, user_id).await;
        let token = generate_access_token(&app.config, user_id);
        admin_ids.push(user_id);
        tokens.push(token);
    }

    let prev = set_setup_complete(&app.pool, false).await;

    let mut guard = app.cleanup_guard();
    guard.restore_config_defaults();
    guard.restore_setup_complete(prev);
    for &id in &admin_ids {
        guard.delete_user(id);
    }

    // Spawn concurrent requests
    let mut handles = Vec::new();
    for (i, token) in tokens.into_iter().enumerate() {
        let router = app.router.clone();
        handles.push(tokio::spawn(async move {
            let req = TestApp::request(Method::POST, "/api/setup/complete")
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(
                    serde_json::json!({
                        "server_name": format!("Server from Admin {}", i),
                        "registration_policy": "open"
                    })
                    .to_string(),
                ))
                .unwrap();

            router.oneshot(req).await.expect("Request failed").status()
        }));
    }

    let mut success_count = 0;
    let mut forbidden_count = 0;

    for handle in handles {
        let status = timeout(Duration::from_secs(30), handle)
            .await
            .expect("Concurrent setup completion task timed out")
            .expect("Task panicked");
        match status.as_u16() {
            204 => success_count += 1,
            403 => forbidden_count += 1,
            other => panic!("Unexpected status: {other}"),
        }
    }

    assert_eq!(
        success_count, 1,
        "Exactly one admin should succeed, got {success_count}"
    );
    assert_eq!(
        forbidden_count,
        num_admins - 1,
        "All other admins should get 403"
    );
}
