//! Data Governance Integration Tests
//!
//! Tests for data export and account deletion lifecycle.
//!
//! Run with: `cargo test --test governance_test -- --nocapture`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{body_to_json, create_test_user, generate_access_token, TestApp};
use serial_test::serial;
use vc_server::auth::hash_password;

// ============================================================================
// Helpers
// ============================================================================

/// Create a test user with a real Argon2 password hash.
async fn create_test_user_with_password(
    pool: &sqlx::PgPool,
    password: &str,
) -> (uuid::Uuid, String) {
    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("govtest_{test_id}");
    let hash = hash_password(password).expect("failed to hash password");

    let user = vc_server::db::create_user(pool, &username, "Gov Test User", None, &hash)
        .await
        .expect("failed to create test user");

    (user.id, username)
}

// ============================================================================
// Data Export Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_request_data_export_no_s3() {
    // Without S3 configured, export request should return 503
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::POST, "/api/me/data-export")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        503,
        "Should return 503 when S3 is not configured"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_get_export_status_none() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, "/api/me/data-export")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        404,
        "Should return 404 when no export job exists"
    );
}

// ============================================================================
// Account Deletion Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_request_deletion_requires_confirm() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Missing "DELETE" confirmation
    let body = serde_json::json!({
        "password": password,
        "confirm": "WRONG"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Should reject wrong confirmation string");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_request_deletion_requires_password() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // No password provided
    let body = serde_json::json!({
        "confirm": "DELETE"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Should require password for local auth");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_request_deletion_wrong_password() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let body = serde_json::json!({
        "password": "wrong_password",
        "confirm": "DELETE"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        401,
        "Should reject wrong password"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_request_deletion_success() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let body = serde_json::json!({
        "password": password,
        "confirm": "DELETE"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Should schedule deletion");

    let json = body_to_json(resp).await;
    assert!(
        json["deletion_scheduled_at"].is_string(),
        "Response should include deletion_scheduled_at"
    );
    assert!(
        json["message"].as_str().unwrap().contains("scheduled"),
        "Response should contain scheduling message"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_deletion_blocked_by_guild_ownership() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    // Create a guild owned by this user
    let guild_id = helpers::create_guild(&app.pool, user_id).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let body = serde_json::json!({
        "password": password,
        "confirm": "DELETE"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        409,
        "Should reject deletion when user owns guilds"
    );

    let json = body_to_json(resp).await;
    assert!(
        json["error"].as_str().unwrap().contains("guilds"),
        "Error should mention guilds"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_cancel_deletion() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // First, request deletion
    let body = serde_json::json!({
        "password": password,
        "confirm": "DELETE"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Deletion request should succeed");

    // Need a fresh token since sessions were invalidated
    let token = generate_access_token(&app.config, user_id);

    // Cancel deletion
    let req = TestApp::request(Method::POST, "/api/me/delete-account/cancel")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Cancellation should succeed");

    let json = body_to_json(resp).await;
    assert!(
        json["message"].as_str().unwrap().contains("cancelled"),
        "Response should confirm cancellation"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_cancel_deletion_when_not_pending() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::POST, "/api/me/delete-account/cancel")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        404,
        "Should return 404 when no deletion is pending"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_duplicate_deletion_request() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let body = serde_json::json!({
        "password": password,
        "confirm": "DELETE"
    });

    // First request
    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // Second request (with fresh token since sessions were invalidated)
    let token = generate_access_token(&app.config, user_id);
    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        409,
        "Should reject duplicate deletion request"
    );
}

// ============================================================================
// UserProfile includes deletion_scheduled_at
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_profile_shows_deletion_scheduled() {
    let app = helpers::fresh_test_app().await;
    let password = "test_password_123!";
    let (user_id, _) = create_test_user_with_password(&app.pool, password).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Request deletion
    let body = serde_json::json!({
        "password": password,
        "confirm": "DELETE"
    });

    let req = TestApp::request(Method::POST, "/api/me/delete-account")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // Check profile (need fresh token since sessions were cleared)
    let token = generate_access_token(&app.config, user_id);
    let req = TestApp::request(Method::GET, "/auth/me")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert!(
        json["deletion_scheduled_at"].is_string(),
        "Profile should include deletion_scheduled_at after deletion request"
    );
}
