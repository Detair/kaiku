//! HTTP Integration Tests for Direct Messages
//!
//! Tests DM creation (with idempotency), listing, access control,
//! and leave behavior.
//!
//! Run with: `cargo test --test integration dm_http -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use super::helpers::{body_to_json, create_test_user, generate_access_token, TestApp};
use uuid::Uuid;

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a DM via the API and return the full response JSON.
async fn create_dm_via_api(
    app: &TestApp,
    token: &str,
    participant_ids: &[Uuid],
) -> (u16, serde_json::Value) {
    let body = serde_json::json!({ "participant_ids": participant_ids });
    let req = TestApp::request(Method::POST, "/api/dm")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let json = body_to_json(resp).await;
    (status, json)
}

/// List all DMs for the authenticated user.
async fn list_dms(app: &TestApp, token: &str) -> serde_json::Value {
    let req = TestApp::request(Method::GET, "/api/dm")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

// ============================================================================
// DM CRUD
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_and_get_dm() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // Create DM between A and B
    let (status, dm) = create_dm_via_api(&app, &token_a, &[user_b]).await;
    assert_eq!(status, 201, "DM creation should return 201");
    let dm_id = dm["id"].as_str().expect("Response should have id");
    assert_eq!(dm["channel_type"], "dm");
    assert!(
        dm["participants"].is_array(),
        "Response should have participants"
    );

    let dm_uuid = Uuid::parse_str(dm_id).unwrap();
    guard.add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_uuid).await });

    // GET the DM
    let req = TestApp::request(Method::GET, &format!("/api/dm/{dm_id}"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "GET DM should return 200");
    let fetched = body_to_json(resp).await;
    assert_eq!(fetched["id"].as_str().unwrap(), dm_id);
    assert_eq!(fetched["channel_type"], "dm");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_dm_returns_existing() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // Create DM first time
    let (status1, dm1) = create_dm_via_api(&app, &token_a, &[user_b]).await;
    assert_eq!(status1, 201, "DM creation failed: {dm1}");
    let dm_id1 = dm1["id"].as_str().unwrap();

    let dm_uuid = Uuid::parse_str(dm_id1).unwrap();
    guard.add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_uuid).await });

    // Create DM second time → same channel_id (idempotent)
    let (status2, dm2) = create_dm_via_api(&app, &token_a, &[user_b]).await;
    assert_eq!(status2, 201);
    let dm_id2 = dm2["id"].as_str().unwrap();

    assert_eq!(
        dm_id1, dm_id2,
        "Creating DM twice with same participants should return same channel"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_list_dms() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let (user_c, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_a);
    guard.delete_user(user_b);
    guard.delete_user(user_c);

    // A creates DM with B
    let (_, dm_ab) = create_dm_via_api(&app, &token_a, &[user_b]).await;
    let dm_ab_uuid = Uuid::parse_str(dm_ab["id"].as_str().unwrap()).unwrap();
    guard.add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_ab_uuid).await });

    // A creates DM with C
    let (_, dm_ac) = create_dm_via_api(&app, &token_a, &[user_c]).await;
    let dm_ac_uuid = Uuid::parse_str(dm_ac["id"].as_str().unwrap()).unwrap();
    guard.add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_ac_uuid).await });

    // A lists → should have 2 DMs
    let dms_a = list_dms(&app, &token_a).await;
    let arr_a = dms_a.as_array().expect("DM list should be an array");
    assert_eq!(arr_a.len(), 2, "User A should see 2 DMs");

    // B lists → should have 1 DM (with A)
    let dms_b = list_dms(&app, &token_b).await;
    let arr_b = dms_b.as_array().expect("DM list should be an array");
    assert_eq!(arr_b.len(), 1, "User B should see 1 DM");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dm_non_participant_forbidden() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let (user_c, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_c = generate_access_token(&app.config, user_c);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_a);
    guard.delete_user(user_b);
    guard.delete_user(user_c);

    // A creates DM with B
    let (_, dm) = create_dm_via_api(&app, &token_a, &[user_b]).await;
    let dm_id = dm["id"].as_str().unwrap();
    let dm_uuid = Uuid::parse_str(dm_id).unwrap();
    guard.add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_uuid).await });

    // User C (non-participant) tries to GET the DM → 403
    let req = TestApp::request(Method::GET, &format!("/api/dm/{dm_id}"))
        .header("Authorization", format!("Bearer {token_c}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        403,
        "Non-participant should get 403 when accessing DM"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_leave_dm() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // A creates DM with B
    let (_, dm) = create_dm_via_api(&app, &token_a, &[user_b]).await;
    let dm_id = dm["id"].as_str().unwrap();
    let dm_uuid = Uuid::parse_str(dm_id).unwrap();
    guard.add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_uuid).await });

    // A leaves the DM → 204
    let req = TestApp::request(Method::POST, &format!("/api/dm/{dm_id}/leave"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Leaving DM should return 204");

    // A tries to GET the DM → should fail (no longer participant)
    let req = TestApp::request(Method::GET, &format!("/api/dm/{dm_id}"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert!(
        resp.status() == 403 || resp.status() == 404,
        "After leaving, GET DM should return 403 or 404, got {}",
        resp.status()
    );
}
