//! HTTP Integration Tests for Admin Report Management Endpoints
//!
//! Tests admin report endpoints under `/api/admin/reports` at the HTTP layer.
//! These require system admin + elevated session (inserted directly in DB).
//!
//! Run with: `cargo test --test integration admin_reports -- --nocapture`

use axum::body::Body;
use axum::http::Method;

use super::helpers::{
    body_to_json, create_elevated_session, create_test_report, create_test_user, delete_user,
    generate_access_token, make_admin, TestApp,
};

// ============================================================================
// Access Control Tests
// ============================================================================

#[tokio::test]
async fn test_list_reports_requires_admin() {
    let app = TestApp::new().await;
    let (user, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user);

    let req = TestApp::request(Method::GET, "/api/admin/reports")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "not_admin");

    // Cleanup
    delete_user(&app.pool, user).await;
}

#[tokio::test]
async fn test_list_reports_requires_elevation() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    // Admin but NOT elevated
    let req = TestApp::request(Method::GET, "/api/admin/reports")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "elevation_required");

    // Cleanup
    delete_user(&app.pool, admin).await;
}

// ============================================================================
// List Reports Tests
// ============================================================================

#[tokio::test]
async fn test_list_reports_success() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let _report_id = create_test_report(&app.pool, reporter, target).await;

    let req = TestApp::request(Method::GET, "/api/admin/reports")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert!(json["items"].is_array());
    assert!(json["total"].as_i64().unwrap() >= 1);

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

#[tokio::test]
async fn test_list_reports_filter_by_status() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let _report_id = create_test_report(&app.pool, reporter, target).await;

    let req = TestApp::request(Method::GET, "/api/admin/reports?status=pending")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    let items = json["items"].as_array().expect("items should be an array");
    for item in items {
        assert_eq!(item["status"], "pending");
    }

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

// ============================================================================
// Get Report Tests
// ============================================================================

#[tokio::test]
async fn test_get_report_success() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let report_id = create_test_report(&app.pool, reporter, target).await;

    let req = TestApp::request(Method::GET, &format!("/api/admin/reports/{report_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["id"], report_id.to_string());
    assert_eq!(json["reporter_id"], reporter.to_string());
    assert_eq!(json["target_user_id"], target.to_string());

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

#[tokio::test]
async fn test_get_report_not_found() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let fake_id = uuid::Uuid::new_v4();
    let req = TestApp::request(Method::GET, &format!("/api/admin/reports/{fake_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "REPORT_NOT_FOUND");

    // Cleanup
    delete_user(&app.pool, admin).await;
}

// ============================================================================
// Claim Report Tests
// ============================================================================

#[tokio::test]
async fn test_claim_report_success() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let report_id = create_test_report(&app.pool, reporter, target).await;

    let req = TestApp::request(
        Method::POST,
        &format!("/api/admin/reports/{report_id}/claim"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["status"], "reviewing");
    assert_eq!(json["assigned_admin_id"], admin.to_string());

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

#[tokio::test]
async fn test_claim_already_reviewing_fails() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let report_id = create_test_report(&app.pool, reporter, target).await;

    // Claim once (succeeds)
    let req = TestApp::request(
        Method::POST,
        &format!("/api/admin/reports/{report_id}/claim"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // Claim again (should fail — status is now 'reviewing', not 'pending')
    let req = TestApp::request(
        Method::POST,
        &format!("/api/admin/reports/{report_id}/claim"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "REPORT_NOT_FOUND");

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

// ============================================================================
// Resolve Report Tests
// ============================================================================

#[tokio::test]
async fn test_resolve_report_success() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let report_id = create_test_report(&app.pool, reporter, target).await;

    let req = TestApp::request(
        Method::POST,
        &format!("/api/admin/reports/{report_id}/resolve"),
    )
    .header("Content-Type", "application/json")
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::from(
        serde_json::json!({
            "resolution_action": "warned",
            "resolution_note": "First offense warning"
        })
        .to_string(),
    ))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["status"], "resolved");
    assert_eq!(json["resolution_action"], "warned");

    // Cleanup
    delete_user(&app.pool, reporter).await;
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

#[tokio::test]
async fn test_resolve_invalid_action() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let (reporter, _) = create_test_user(&app.pool).await;
    let (target, _) = create_test_user(&app.pool).await;
    let report_id = create_test_report(&app.pool, reporter, target).await;

    let req = TestApp::request(
        Method::POST,
        &format!("/api/admin/reports/{report_id}/resolve"),
    )
    .header("Content-Type", "application/json")
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::from(
        serde_json::json!({
            "resolution_action": "invalid_action"
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
    delete_user(&app.pool, target).await;
    delete_user(&app.pool, admin).await;
}

// ============================================================================
// Report Stats Tests
// ============================================================================

#[tokio::test]
async fn test_report_stats_success() {
    let app = TestApp::new().await;
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    create_elevated_session(&app.pool, admin).await;
    let token = generate_access_token(&app.config, admin);

    let req = TestApp::request(Method::GET, "/api/admin/reports/stats")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert!(json["pending"].is_number(), "pending should be a number");
    assert!(
        json["reviewing"].is_number(),
        "reviewing should be a number"
    );
    assert!(json["resolved"].is_number(), "resolved should be a number");
    assert!(
        json["dismissed"].is_number(),
        "dismissed should be a number"
    );

    // Cleanup
    delete_user(&app.pool, admin).await;
}
