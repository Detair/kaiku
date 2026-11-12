//! HTTP Integration Tests for Blocking Endpoints
//!
//! Tests the block/unblock API at the HTTP layer using `tower::ServiceExt::oneshot`.
//! Each test runs against a fresh per-test database provisioned by `#[sqlx::test]`,
//! so no manual row cleanup is required.
//!
//! Run with: `cargo test --test integration blocking -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;

use super::helpers::{
    body_to_json, create_dm_channel, create_test_user, generate_access_token, TestApp,
};

// ============================================================================
// Block / Unblock Tests
// ============================================================================

#[sqlx::test]
async fn test_block_user_success(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

#[sqlx::test]
async fn test_block_user_requires_auth(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_b, _) = create_test_user(&app.pool).await;

    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);
}

#[sqlx::test]
async fn test_block_self_fails(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

#[sqlx::test]
async fn test_unblock_user_success(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

#[sqlx::test]
async fn test_unblock_nonexistent_fails(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

#[sqlx::test]
async fn test_block_prevents_friend_request(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

#[sqlx::test]
async fn test_block_prevents_dm_creation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

#[sqlx::test]
async fn test_block_prevents_message_in_dm(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

/// The Redis block cache expires; a block that exists only in Postgres must still
/// refuse DM sends in both directions.
#[sqlx::test]
async fn test_db_only_block_refuses_dm_send(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);
    let channel_id = create_dm_channel(&app.pool, user_a, user_b).await;

    // A blocks B directly in the DB: no Redis cache entry exists.
    sqlx::query(
        "INSERT INTO friendships (requester_id, addressee_id, status) VALUES ($1, $2, 'blocked')",
    )
    .bind(user_a)
    .bind(user_b)
    .execute(&app.pool)
    .await
    .unwrap();

    for token in [&token_b, &token_a] {
        let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::from(
                serde_json::json!({ "content": "Hello!" }).to_string(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await;
        assert_eq!(resp.status(), 403);
        assert_eq!(body_to_json(resp).await["error"], "BLOCKED");
    }
}

// ============================================================================
// Group DMs follow the same block rule as 1:1 DMs
// ============================================================================

async fn db_block(pool: &PgPool, blocker: uuid::Uuid, blocked: uuid::Uuid) {
    sqlx::query(
        "INSERT INTO friendships (requester_id, addressee_id, status) VALUES ($1, $2, 'blocked')",
    )
    .bind(blocker)
    .bind(blocked)
    .execute(pool)
    .await
    .unwrap();
}

async fn create_dm(
    app: &TestApp,
    token: &str,
    participants: &[uuid::Uuid],
) -> (u16, serde_json::Value) {
    let req = TestApp::request(Method::POST, "/api/dm")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::json!({ "participant_ids": participants }).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// Inviting someone who blocked you into a group DM is refused like a 1:1 DM.
#[sqlx::test]
async fn group_dm_with_user_who_blocked_caller_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (caller, _) = create_test_user(&pool).await;
    let (blocker, _) = create_test_user(&pool).await;
    let (friend, _) = create_test_user(&pool).await;
    db_block(&pool, blocker, caller).await;
    let token = generate_access_token(&app.config, caller);

    let (status, json) = create_dm(&app, &token, &[blocker, friend]).await;
    assert_eq!(status, 400);
    assert!(json["message"]
        .as_str()
        .unwrap_or("")
        .contains("Cannot create DM"));
    let groups: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM channels WHERE channel_type = 'dm'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(groups, 0);
}

/// Inviting someone you blocked is refused too.
#[sqlx::test]
async fn group_dm_with_user_caller_blocked_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (caller, _) = create_test_user(&pool).await;
    let (blocked, _) = create_test_user(&pool).await;
    let (friend, _) = create_test_user(&pool).await;
    db_block(&pool, caller, blocked).await;
    let token = generate_access_token(&app.config, caller);

    let (status, _) = create_dm(&app, &token, &[friend, blocked]).await;
    assert_eq!(status, 400);
}

/// A block between two invited people is theirs: the caller may still
/// create the group (it would reveal a third party's block otherwise).
#[sqlx::test]
async fn group_dm_ignores_blocks_between_other_participants(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (caller, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let (c, _) = create_test_user(&pool).await;
    db_block(&pool, b, c).await;
    let token = generate_access_token(&app.config, caller);

    let (status, _) = create_dm(&app, &token, &[b, c]).await;
    assert_eq!(status, 201);
}

/// A 1:1 DM is also refused when only Postgres knows the block.
#[sqlx::test]
async fn dm_with_db_only_block_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (caller, _) = create_test_user(&pool).await;
    let (blocker, _) = create_test_user(&pool).await;
    db_block(&pool, blocker, caller).await;
    let token = generate_access_token(&app.config, caller);

    let (status, _) = create_dm(&app, &token, &[blocker]).await;
    assert_eq!(status, 400);
}
