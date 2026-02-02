//! HTTP Integration Tests for Setup Endpoints
//!
//! Tests the setup API at the HTTP layer using `tower::ServiceExt::oneshot`.
//! Each test that modifies shared state (`setup_complete`, `system_admins`)
//! uses `#[serial]` and restores the original state on completion.
//!
//! Run with: `cargo test --test setup_http_test -- --nocapture`

mod helpers;

use axum::{body::Body, http::Method};
use helpers::{body_to_json, create_test_user, generate_access_token, make_admin, TestApp};
use serial_test::serial;

// ============================================================================
// Database state helpers
// ============================================================================

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

/// Delete a user by ID (cascades to `system_admins`).
async fn delete_user(pool: &sqlx::PgPool, user_id: uuid::Uuid) {
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to delete test user");
}

/// Restore default config values that `complete` may have changed.
async fn restore_config_defaults(pool: &sqlx::PgPool) {
    for (key, val) in [
        ("server_name", serde_json::json!("Canis Server")),
        ("registration_policy", serde_json::json!("open")),
        ("terms_url", serde_json::Value::Null),
        ("privacy_url", serde_json::Value::Null),
    ] {
        sqlx::query("UPDATE server_config SET value = $1, updated_by = NULL WHERE key = $2")
            .bind(val)
            .bind(key)
            .execute(pool)
            .await
            .expect("Failed to restore config default");
    }
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
#[serial]
async fn test_status_returns_setup_state() {
    let app = TestApp::new().await;

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

#[tokio::test]
#[serial]
async fn test_config_returns_values_when_setup_incomplete() {
    let app = TestApp::new().await;
    let prev = set_setup_complete(&app.pool, false).await;

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
    // terms_url and privacy_url may be null or string
    assert!(
        json["terms_url"].is_null() || json["terms_url"].is_string(),
        "Expected terms_url to be null or string"
    );
    assert!(
        json["privacy_url"].is_null() || json["privacy_url"].is_string(),
        "Expected privacy_url to be null or string"
    );

    // Restore
    set_setup_complete(&app.pool, prev).await;
}

#[tokio::test]
#[serial]
async fn test_config_returns_403_when_setup_complete() {
    let app = TestApp::new().await;
    let prev = set_setup_complete(&app.pool, true).await;

    let req = TestApp::request(Method::GET, "/api/setup/config")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "SETUP_ALREADY_COMPLETE");

    // Restore
    set_setup_complete(&app.pool, prev).await;
}

#[tokio::test]
#[serial]
async fn test_complete_requires_auth() {
    let app = TestApp::new().await;

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

#[tokio::test]
#[serial]
async fn test_complete_requires_admin() {
    let app = TestApp::new().await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    // NOT making this user an admin
    let token = generate_access_token(&app.config, user_id);

    let prev = set_setup_complete(&app.pool, false).await;

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

    // Cleanup
    set_setup_complete(&app.pool, prev).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
#[serial]
async fn test_complete_succeeds_for_admin() {
    let app = TestApp::new().await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let prev = set_setup_complete(&app.pool, false).await;

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

    // Cleanup — restore config defaults and original setup_complete
    restore_config_defaults(&app.pool).await;
    set_setup_complete(&app.pool, prev).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
#[serial]
async fn test_complete_rejects_invalid_body() {
    let app = TestApp::new().await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let prev = set_setup_complete(&app.pool, false).await;

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

    // Cleanup
    set_setup_complete(&app.pool, prev).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
#[serial]
async fn test_complete_already_done() {
    let app = TestApp::new().await;
    let (user_id, _username) = create_test_user(&app.pool).await;
    make_admin(&app.pool, user_id).await;
    let token = generate_access_token(&app.config, user_id);

    let prev = set_setup_complete(&app.pool, true).await;

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
    assert_eq!(json["error"], "SETUP_ALREADY_COMPLETE");

    // Cleanup
    set_setup_complete(&app.pool, prev).await;
    delete_user(&app.pool, user_id).await;
}
