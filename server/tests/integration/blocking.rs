//! HTTP Integration Tests for Blocking Endpoints
//!
//! Tests the block/unblock API at the HTTP layer using `tower::ServiceExt::oneshot`.
//! Each test creates its own users and cleans up via `delete_user` (CASCADE).
//!
//! Run with: `cargo test --test integration blocking -- --nocapture`

use axum::body::Body;
use axum::http::Method;

use super::helpers::{
    body_to_json, create_dm_channel, create_test_user, delete_user, generate_access_token, TestApp,
};

// ============================================================================
// Block / Unblock Tests
// ============================================================================

#[tokio::test]
async fn test_block_user_success() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_a);

    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["status"], "Blocked");
    assert_eq!(json["requester_id"], user_a.to_string());
    assert_eq!(json["addressee_id"], user_b.to_string());

    // Verify DB state
    let row = sqlx::query_scalar::<_, String>(
        "SELECT status::text FROM friendships WHERE requester_id = $1 AND addressee_id = $2",
    )
    .bind(user_a)
    .bind(user_b)
    .fetch_one(&app.pool)
    .await
    .expect("Friendship row should exist");
    assert_eq!(row, "blocked");

    // Cleanup
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

#[tokio::test]
async fn test_block_user_requires_auth() {
    let app = TestApp::new().await;
    let (user_b, _) = create_test_user(&app.pool).await;

    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);

    // Cleanup
    delete_user(&app.pool, user_b).await;
}

#[tokio::test]
async fn test_block_self_fails() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_a);

    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_a}/block"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "SELF_FRIEND_REQUEST");

    // Cleanup
    delete_user(&app.pool, user_a).await;
}

#[tokio::test]
async fn test_unblock_user_success() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_a);

    // First, block user_b
    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // Now unblock
    let req = TestApp::request(Method::DELETE, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // Verify friendship row is deleted
    let exists: bool = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM friendships WHERE requester_id = $1 AND addressee_id = $2)",
    )
    .bind(user_a)
    .bind(user_b)
    .fetch_one(&app.pool)
    .await
    .expect("Query should succeed");
    assert!(!exists, "Friendship row should be deleted after unblock");

    // Cleanup
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

#[tokio::test]
async fn test_unblock_nonexistent_fails() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_a);

    // Unblock someone we haven't blocked
    let req = TestApp::request(Method::DELETE, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "FRIENDSHIP_NOT_FOUND");

    // Cleanup
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

#[tokio::test]
async fn test_block_prevents_friend_request() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    // A blocks B
    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // B tries to send friend request to A — need A's username
    let username_a: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user_a)
        .fetch_one(&app.pool)
        .await
        .expect("User should exist");

    let req = TestApp::request(Method::POST, "/api/friends/request")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token_b}"))
        .body(Body::from(
            serde_json::json!({ "username": username_a }).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "BLOCKED");

    // Cleanup
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

#[tokio::test]
async fn test_block_prevents_dm_creation() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    // A blocks B
    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // B tries to create DM with A
    let req = TestApp::request(Method::POST, "/api/dm")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token_b}"))
        .body(Body::from(
            serde_json::json!({ "participant_ids": [user_a] }).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "VALIDATION_ERROR");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("Cannot create DM"),
        "Expected block-related validation message, got: {}",
        json["message"]
    );

    // Cleanup
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

#[tokio::test]
async fn test_block_prevents_message_in_dm() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    // Create a DM channel between A and B (directly in DB)
    let channel_id = create_dm_channel(&app.pool, user_a, user_b).await;

    // A blocks B
    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    // B tries to send a message in the DM
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token_b}"))
        .body(Body::from(
            serde_json::json!({ "content": "Hello!" }).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "BLOCKED");

    // Cleanup
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}
