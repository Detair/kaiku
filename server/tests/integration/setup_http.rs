//! HTTP Integration Tests for Setup Endpoints
//!
//! Tests the setup API at the HTTP layer using `tower::ServiceExt::oneshot`.
//!
//! Each test runs against a fresh per-test database provisioned by
//! `#[sqlx::test]`, so mutations of the singleton `server_config.setup_complete`
//! row are isolated. Tests that depend on a specific initial value set it
//! explicitly; no cross-test state restoration is needed.
//!
//! Run with: `cargo test --test integration setup_http -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use tokio::time::{timeout, Duration};

use super::helpers::{body_to_json, create_test_user, generate_access_token, make_admin, TestApp};

// ============================================================================
// Database state helpers
// ============================================================================

/// Set `setup_complete` to the given value.
async fn set_setup_complete(pool: &sqlx::PgPool, complete: bool) {
    sqlx::query("UPDATE server_config SET value = $1::jsonb WHERE key = 'setup_complete'")
        .bind(serde_json::json!(complete))
        .execute(pool)
        .await
        .expect("Failed to set setup_complete");
}

// ============================================================================
// Tests
// ============================================================================

#[sqlx::test]
async fn test_status_returns_setup_state(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;

    let req = TestApp::request(Method::GET, "/api/setup/status")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    // The field must exist and be a boolean (value depends on DB state)
    assert!(
        json["setup_complete"].is_boolean(),
        "Expected setup_complete to be a boolean, got: {json}"
    );
}

#[sqlx::test]
async fn test_config_returns_values_when_setup_incomplete(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    set_setup_complete(&app.pool, false).await;

    let req = TestApp::request(Method::GET, "/api/setup/config")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert!(
        json["server_name"].is_string(),
        "Expected server_name string"
    );
    assert!(
        json["registration_policy"].is_string(),
        "Expected registration_policy string"
    );
    assert!(
        json["terms_url"].is_null() || json["terms_url"].is_string(),
        "Expected terms_url to be null or string"
    );
    assert!(
        json["privacy_url"].is_null() || json["privacy_url"].is_string(),
        "Expected privacy_url to be null or string"
    );
}

#[sqlx::test]
async fn test_config_returns_403_when_setup_complete(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    set_setup_complete(&app.pool, true).await;

    let req = TestApp::request(Method::GET, "/api/setup/config")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "SETUP_ALREADY_COMPLETE");
}

#[sqlx::test]
async fn test_complete_requires_auth(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;

    let req = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "server_name": "Test",
                "registration_policy": "open"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "MISSING_AUTH");
}

#[sqlx::test]
async fn test_complete_requires_admin(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    set_setup_complete(&app.pool, false).await;

    let req = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "server_name": "Test",
                "registration_policy": "open"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "FORBIDDEN");
}

#[sqlx::test]
async fn test_complete_succeeds_for_admin(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    set_setup_complete(&app.pool, false).await;

    let req = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "server_name": "My Test Server",
                "registration_policy": "invite_only",
                "terms_url": "https://example.com/terms",
                "privacy_url": "https://example.com/privacy"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204);

    // Verify DB state
    let setup_val: serde_json::Value =
        sqlx::query_scalar("SELECT value FROM server_config WHERE key = 'setup_complete'")
            .fetch_one(&app.pool)
            .await
            .expect("Failed to read setup_complete");
    assert_eq!(
        setup_val.as_bool(),
        Some(true),
        "setup_complete should be true after completion"
    );
}

#[sqlx::test]
async fn test_complete_rejects_invalid_body(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    set_setup_complete(&app.pool, false).await;

    let req = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "server_name": "Test",
                "registration_policy": "invalid"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "VALIDATION_ERROR");
}

#[sqlx::test]
async fn test_complete_already_done(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    set_setup_complete(&app.pool, true).await;

    let req = TestApp::request(Method::POST, "/api/setup/complete")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "server_name": "Test",
                "registration_policy": "open"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = timeout(Duration::from_secs(10), app.oneshot(req))
        .await
        .expect("setup/complete request timed out (possible deadlock)");
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "SETUP_ALREADY_COMPLETE");
}
