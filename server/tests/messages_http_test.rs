//! HTTP Integration Tests for Message CRUD
//!
//! Tests message creation, validation, pagination, editing, deletion,
//! and nonexistent channel handling.
//!
//! Run with: `cargo test --test messages_http_test -- --nocapture`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{body_to_json, create_test_user, generate_access_token, CleanupGuard, TestApp};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

// ============================================================================
// Permission bits (from server/src/permissions/guild.rs)
// ============================================================================

const VIEW_CHANNEL: i64 = 1 << 24;
const SEND_MESSAGES: i64 = 1 << 0;

// ============================================================================
// Test Helpers
// ============================================================================

async fn create_guild_with_owner(pool: &PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Msg Test Guild")
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to create test guild");

    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to add owner as guild member");

    // Create @everyone role with VIEW_CHANNEL + SEND_MESSAGES
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default) VALUES ($1, $2, '@everyone', $3, 0, true)",
    )
    .bind(Uuid::new_v4())
    .bind(guild_id)
    .bind(VIEW_CHANNEL | SEND_MESSAGES)
    .execute(pool)
    .await
    .expect("Failed to create @everyone role");

    guild_id
}

async fn add_guild_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) {
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to add guild member");
}

async fn create_channel(pool: &PgPool, guild_id: Uuid, name: &str) -> Uuid {
    let channel_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channels (id, name, channel_type, guild_id, position, max_screen_shares)
         VALUES ($1, $2, 'text', $3, 0, 5)",
    )
    .bind(channel_id)
    .bind(name)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("Failed to create test channel");
    channel_id
}

/// Send a message via the API and return the response JSON.
async fn send_message(
    app: &TestApp,
    channel_id: Uuid,
    token: &str,
    content: &str,
) -> serde_json::Value {
    let body = serde_json::json!({ "content": content });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "Expected 201 Created for message");
    body_to_json(resp).await
}

/// List messages in a channel, returning the full paginated response.
async fn list_messages(
    app: &TestApp,
    channel_id: Uuid,
    token: &str,
    query_string: &str,
) -> serde_json::Value {
    let url = if query_string.is_empty() {
        format!("/api/messages/channel/{channel_id}")
    } else {
        format!("/api/messages/channel/{channel_id}?{query_string}")
    };
    let req = TestApp::request(Method::GET, &url)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

fn register_guild_cleanup(guard: &mut CleanupGuard, guild_id: Uuid) {
    guard.add(move |pool| async move {
        let _ = sqlx::query("DELETE FROM guilds WHERE id = $1")
            .bind(guild_id)
            .execute(&pool)
            .await;
    });
}

// ============================================================================
// Message CRUD
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_create_message_success() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "msg-create-test").await;

    let mut guard = app.cleanup_guard();
    register_guild_cleanup(&mut guard, guild_id);
    guard.delete_user(user_id);

    let msg = send_message(&app, channel_id, &token, "Hello, world!").await;

    assert!(msg["id"].is_string(), "Response should have an id");
    assert_eq!(msg["content"], "Hello, world!");
    assert_eq!(msg["channel_id"], channel_id.to_string());
    assert_eq!(msg["encrypted"], false);
    assert!(msg["author"].is_object(), "Response should have author");
    assert!(
        msg["created_at"].is_string(),
        "Response should have created_at"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_create_message_validation_errors() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "msg-validation-test").await;

    let mut guard = app.cleanup_guard();
    register_guild_cleanup(&mut guard, guild_id);
    guard.delete_user(user_id);

    // Empty content → 400
    let body = serde_json::json!({ "content": "" });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Empty content should return 400");

    // Content too long (4001 chars) → 400
    let long_content = "a".repeat(4001);
    let body = serde_json::json!({ "content": long_content });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "4001-char content should return 400");

    // Encrypted without nonce → 400
    let body = serde_json::json!({ "content": "encrypted msg", "encrypted": true });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        400,
        "Encrypted message without nonce should return 400"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_list_messages_pagination() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "msg-pagination-test").await;

    let mut guard = app.cleanup_guard();
    register_guild_cleanup(&mut guard, guild_id);
    guard.delete_user(user_id);

    // Create 5 messages with small delays to ensure distinct ordering
    for i in 1..=5 {
        send_message(&app, channel_id, &token, &format!("Message {i}")).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Fetch first page (limit=2) — messages are newest-first
    let page1 = list_messages(&app, channel_id, &token, "limit=2").await;
    let items1 = page1["items"].as_array().expect("items should be an array");
    assert_eq!(items1.len(), 2, "First page should have 2 messages");
    assert!(
        page1["has_more"].as_bool().unwrap(),
        "Should indicate more messages exist"
    );
    assert!(
        page1["next_cursor"].is_string(),
        "Should provide next_cursor"
    );

    // Fetch second page using cursor
    let cursor = page1["next_cursor"].as_str().unwrap();
    let page2 = list_messages(
        &app,
        channel_id,
        &token,
        &format!("limit=2&before={cursor}"),
    )
    .await;
    let items2 = page2["items"].as_array().expect("items should be an array");
    assert_eq!(items2.len(), 2, "Second page should have 2 messages");

    // Verify no overlap between pages
    let ids1: Vec<&str> = items1.iter().map(|m| m["id"].as_str().unwrap()).collect();
    let ids2: Vec<&str> = items2.iter().map(|m| m["id"].as_str().unwrap()).collect();
    for id in &ids2 {
        assert!(
            !ids1.contains(id),
            "Page 2 should not contain items from page 1"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_edit_message_owner_only() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let guild_id = create_guild_with_owner(&app.pool, user_a).await;
    add_guild_member(&app.pool, guild_id, user_b).await;
    let channel_id = create_channel(&app.pool, guild_id, "msg-edit-test").await;

    let mut guard = app.cleanup_guard();
    register_guild_cleanup(&mut guard, guild_id);
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // User A creates a message
    let msg = send_message(&app, channel_id, &token_a, "Original content").await;
    let msg_id = msg["id"].as_str().unwrap();

    // User B tries to edit User A's message → should fail
    let body = serde_json::json!({ "content": "Edited by B" });
    let req = TestApp::request(Method::PATCH, &format!("/api/messages/{msg_id}"))
        .header("Authorization", format!("Bearer {token_b}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert!(
        resp.status() == 403 || resp.status() == 404,
        "Non-owner edit should return 403 or 404, got {}",
        resp.status()
    );

    // User A edits their own message → 200
    let body = serde_json::json!({ "content": "Edited by A" });
    let req = TestApp::request(Method::PATCH, &format!("/api/messages/{msg_id}"))
        .header("Authorization", format!("Bearer {token_a}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Owner should be able to edit");
    let edited = body_to_json(resp).await;
    assert_eq!(edited["content"], "Edited by A");
    assert!(
        edited["edited_at"].is_string(),
        "edited_at should be set after edit"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_delete_message_owner_only() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let guild_id = create_guild_with_owner(&app.pool, user_a).await;
    add_guild_member(&app.pool, guild_id, user_b).await;
    let channel_id = create_channel(&app.pool, guild_id, "msg-delete-test").await;

    let mut guard = app.cleanup_guard();
    register_guild_cleanup(&mut guard, guild_id);
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // User A creates a message
    let msg = send_message(&app, channel_id, &token_a, "To be deleted").await;
    let msg_id = msg["id"].as_str().unwrap();

    // User B tries to delete User A's message → 403
    let req = TestApp::request(Method::DELETE, &format!("/api/messages/{msg_id}"))
        .header("Authorization", format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403, "Non-owner delete should return 403");

    // User A deletes their own message → 204
    let req = TestApp::request(Method::DELETE, &format!("/api/messages/{msg_id}"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Owner should be able to delete");

    // Verify message is gone from listing (soft deleted)
    let msgs = list_messages(&app, channel_id, &token_a, "").await;
    let items = msgs["items"].as_array().unwrap();
    let found = items.iter().any(|m| m["id"].as_str() == Some(msg_id));
    assert!(!found, "Deleted message should not appear in listing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_create_message_nonexistent_channel() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let fake_channel = Uuid::new_v4();
    let body = serde_json::json!({ "content": "Hello" });
    let req = TestApp::request(
        Method::POST,
        &format!("/api/messages/channel/{fake_channel}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        404,
        "Posting to nonexistent channel should return 404"
    );
}
