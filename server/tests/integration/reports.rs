//! HTTP Integration Tests for User-Facing Report Endpoint
//!
//! Tests `POST /api/reports` at the HTTP layer using `tower::ServiceExt::oneshot`.
//! Each test creates its own users and cleans up via `delete_user` (CASCADE).
//!
//! Run with: `cargo test --test integration reports -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use super::helpers::{body_to_json, create_test_user, delete_user, generate_access_token, TestApp};

// ============================================================================
// Report Creation Tests
// ============================================================================

#[tokio::test]
async fn test_create_report_success() {
    let app = TestApp::new().await;
    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, reporter);

    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": target,
                "category": "harassment",
                "description": "Test report description"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["reporter_id"], reporter.to_string());
    assert_eq!(json["target_user_id"], target.to_string());
    assert_eq!(json["status"], "pending");
    assert_eq!(json["category"], "harassment");

    // Verify DB state
    let status: String = sqlx::query_scalar::<_, String>(
        "SELECT status::text FROM user_reports WHERE reporter_id = $1 AND target_user_id = $2",
    )
    .bind(reporter)
    .bind(target)
    .fetch_one(&app.pool)
    .await
    .expect("Report row should exist");
    assert_eq!(status, "pending");

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
}

#[tokio::test]
async fn test_create_report_requires_auth() {
    let app = TestApp::new().await;
    let (target, _) = create_test_user(&app.pool).await;

    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": target,
                "category": "spam"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);

    // Cleanup
    delete_user(&app.pool, target).await;
}

#[tokio::test]
async fn test_create_report_self_fails() {
    let app = TestApp::new().await;
    let (user, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user);

    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": user,
                "category": "harassment"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "VALIDATION_ERROR");

    // Cleanup
    delete_user(&app.pool, user).await;
}

#[tokio::test]
async fn test_create_report_invalid_target() {
    let app = TestApp::new().await;
    let (reporter, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, reporter);
    let fake_id = uuid::Uuid::new_v4();

    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": fake_id,
                "category": "spam"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "VALIDATION_ERROR");

    // Cleanup
    delete_user(&app.pool, reporter).await;
}

#[tokio::test]
async fn test_create_report_invalid_category() {
    let app = TestApp::new().await;
    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, reporter);

    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": target,
                "category": "nonexistent_category"
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 422);

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
}

#[tokio::test]
async fn test_create_report_duplicate_active() {
    let app = TestApp::new().await;
    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, reporter);

    // Create first report
    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": target,
                "category": "harassment"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // Try to create duplicate report
    let req = TestApp::request(Method::POST, "/api/reports")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({
                "target_type": "user",
                "target_user_id": target,
                "category": "spam"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 409);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "DUPLICATE_REPORT");

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
}
