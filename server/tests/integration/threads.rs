//! HTTP Integration Tests for Message Threads
//!
//! Tests thread reply creation, listing, pagination, counter updates,
//! nested-thread prevention, and thread read state.
//!
//! Run with: `cargo test --test integration threads -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use super::helpers::{body_to_json, create_test_user, delete_user, generate_access_token, TestApp};
use sqlx::PgPool;
use uuid::Uuid;

// ============================================================================
// Test Helpers
// ============================================================================

// Permission bits (from server/src/permissions/guild.rs)
const VIEW_CHANNEL: i64 = 1 << 24;
const SEND_MESSAGES: i64 = 1 << 0;

async fn create_guild_with_owner(pool: &PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Thread Test Guild")
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

    // Create @everyone role with VIEW_CHANNEL + SEND_MESSAGES so non-owner members have access
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
    parent_id: Option<Uuid>,
) -> serde_json::Value {
    let body = if let Some(pid) = parent_id {
        serde_json::json!({ "content": content, "parent_id": pid })
    } else {
        serde_json::json!({ "content": content })
    };

    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "Expected 201 Created for message");
    body_to_json(resp).await
}

/// List messages in a channel via the API.
async fn list_channel_messages(app: &TestApp, channel_id: Uuid, token: &str) -> serde_json::Value {
    let req = TestApp::request(Method::GET, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

/// List thread replies via the API.
async fn list_thread_replies(
    app: &TestApp,
    parent_id: &str,
    token: &str,
    after: Option<&str>,
) -> serde_json::Value {
    let url = if let Some(cursor) = after {
        format!("/api/messages/{parent_id}/thread?after={cursor}")
    } else {
        format!("/api/messages/{parent_id}/thread")
    };

    let req = TestApp::request(Method::GET, &url)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

/// Clean up: delete guild (cascades to channels, members, messages).
async fn cleanup_guild(pool: &PgPool, guild_id: Uuid) {
    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .expect("Failed to delete guild");
}

// ============================================================================
// Thread Reply CRUD
// ============================================================================

#[tokio::test]
async fn test_create_thread_reply() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-test").await;

    // Create parent message
    let parent = send_message(&app, channel_id, &token, "Parent message", None).await;
    let parent_id = parent["id"].as_str().unwrap();

    // Create thread reply
    let reply = send_message(
        &app,
        channel_id,
        &token,
        "Thread reply 1",
        Some(Uuid::parse_str(parent_id).unwrap()),
    )
    .await;

    assert_eq!(reply["content"], "Thread reply 1");
    assert_eq!(reply["parent_id"], parent_id);

    // Verify parent's thread counters were updated
    let row = sqlx::query_as::<_, (i32, bool)>(
        "SELECT thread_reply_count, thread_last_reply_at IS NOT NULL FROM messages WHERE id = $1",
    )
    .bind(Uuid::parse_str(parent_id).unwrap())
    .fetch_one(&app.pool)
    .await
    .expect("Parent message should exist");

    assert_eq!(row.0, 1, "thread_reply_count should be 1");
    assert!(row.1, "thread_last_reply_at should be set");

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
async fn test_thread_replies_not_in_channel_feed() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-feed-test").await;

    // Create parent + reply
    let parent = send_message(&app, channel_id, &token, "Parent", None).await;
    let parent_id = parent["id"].as_str().unwrap();

    send_message(
        &app,
        channel_id,
        &token,
        "Thread reply",
        Some(Uuid::parse_str(parent_id).unwrap()),
    )
    .await;

    // List channel messages — should only show the parent, not the reply
    let channel_msgs = list_channel_messages(&app, channel_id, &token).await;
    let items = channel_msgs["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        1,
        "Channel feed should only have the parent message"
    );
    assert_eq!(items[0]["id"], parent_id);

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
async fn test_list_thread_replies() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-list-test").await;

    // Create parent + 3 replies
    let parent = send_message(&app, channel_id, &token, "Parent message", None).await;
    let parent_id_str = parent["id"].as_str().unwrap();
    let parent_id = Uuid::parse_str(parent_id_str).unwrap();

    for i in 1..=3 {
        send_message(
            &app,
            channel_id,
            &token,
            &format!("Reply {i}"),
            Some(parent_id),
        )
        .await;
    }

    // List thread replies
    let thread = list_thread_replies(&app, parent_id_str, &token, None).await;
    let items = thread["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "Should have 3 thread replies");

    // Verify chronological order (ASC)
    assert_eq!(items[0]["content"], "Reply 1");
    assert_eq!(items[1]["content"], "Reply 2");
    assert_eq!(items[2]["content"], "Reply 3");

    // All replies should have parent_id set
    for item in items {
        assert_eq!(item["parent_id"], parent_id_str);
    }

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
async fn test_thread_replies_pagination() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-pagination-test").await;

    // Create parent + 5 replies
    let parent = send_message(&app, channel_id, &token, "Parent", None).await;
    let parent_id_str = parent["id"].as_str().unwrap();
    let parent_id = Uuid::parse_str(parent_id_str).unwrap();

    for i in 1..=5 {
        send_message(
            &app,
            channel_id,
            &token,
            &format!("Reply {i}"),
            Some(parent_id),
        )
        .await;
    }

    // Fetch first page (limit=2)
    let url = format!("/api/messages/{parent_id_str}/thread?limit=2");
    let req = TestApp::request(Method::GET, &url)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let page1 = body_to_json(resp).await;

    let items1 = page1["items"].as_array().unwrap();
    assert_eq!(items1.len(), 2);
    assert_eq!(items1[0]["content"], "Reply 1");
    assert_eq!(items1[1]["content"], "Reply 2");
    assert!(
        page1["has_more"].as_bool().unwrap(),
        "Should have more pages"
    );

    // Fetch second page using cursor
    let cursor = page1["next_cursor"].as_str().unwrap();
    let url2 = format!("/api/messages/{parent_id_str}/thread?limit=2&after={cursor}");
    let req2 = TestApp::request(Method::GET, &url2)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp2 = app.oneshot(req2).await;
    assert_eq!(resp2.status(), 200);
    let page2 = body_to_json(resp2).await;

    let items2 = page2["items"].as_array().unwrap();
    assert_eq!(items2.len(), 2);
    assert_eq!(items2[0]["content"], "Reply 3");
    assert_eq!(items2[1]["content"], "Reply 4");
    assert!(page2["has_more"].as_bool().unwrap());

    // Fetch last page
    let cursor2 = page2["next_cursor"].as_str().unwrap();
    let url3 = format!("/api/messages/{parent_id_str}/thread?limit=2&after={cursor2}");
    let req3 = TestApp::request(Method::GET, &url3)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp3 = app.oneshot(req3).await;
    assert_eq!(resp3.status(), 200);
    let page3 = body_to_json(resp3).await;

    let items3 = page3["items"].as_array().unwrap();
    assert_eq!(items3.len(), 1);
    assert_eq!(items3[0]["content"], "Reply 5");
    assert!(!page3["has_more"].as_bool().unwrap(), "No more pages");

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Nested Thread Prevention
// ============================================================================

#[tokio::test]
async fn test_cannot_nest_threads() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-nest-test").await;

    // Create parent + reply
    let parent = send_message(&app, channel_id, &token, "Parent", None).await;
    let parent_id = Uuid::parse_str(parent["id"].as_str().unwrap()).unwrap();

    let reply = send_message(&app, channel_id, &token, "Reply", Some(parent_id)).await;
    let reply_id = reply["id"].as_str().unwrap();

    // Try to create a nested reply (reply to a reply) — should fail
    let body = serde_json::json!({
        "content": "Nested reply",
        "parent_id": reply_id,
    });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_ne!(resp.status(), 201, "Nested threads should be rejected");

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Thread Counter Updates on Delete
// ============================================================================

#[tokio::test]
async fn test_delete_thread_reply_decrements_counter() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-delete-test").await;

    // Create parent + 2 replies
    let parent = send_message(&app, channel_id, &token, "Parent", None).await;
    let parent_id = Uuid::parse_str(parent["id"].as_str().unwrap()).unwrap();

    let reply1 = send_message(&app, channel_id, &token, "Reply 1", Some(parent_id)).await;
    send_message(&app, channel_id, &token, "Reply 2", Some(parent_id)).await;

    // Verify counter is 2
    let count: i32 = sqlx::query_scalar("SELECT thread_reply_count FROM messages WHERE id = $1")
        .bind(parent_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // Delete reply 1
    let reply1_id = reply1["id"].as_str().unwrap();
    let req = TestApp::request(Method::DELETE, &format!("/api/messages/{reply1_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert!(
        resp.status().is_success(),
        "Delete should succeed, got {}",
        resp.status()
    );

    // Verify counter decremented to 1
    let count_after: i32 =
        sqlx::query_scalar("SELECT thread_reply_count FROM messages WHERE id = $1")
            .bind(parent_id)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        count_after, 1,
        "Counter should decrement after reply deletion"
    );

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Thread Read State
// ============================================================================

#[tokio::test]
async fn test_mark_thread_read() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-read-test").await;

    // Create parent + reply
    let parent = send_message(&app, channel_id, &token, "Parent", None).await;
    let parent_id_str = parent["id"].as_str().unwrap();
    let parent_id = Uuid::parse_str(parent_id_str).unwrap();

    let reply = send_message(&app, channel_id, &token, "Reply", Some(parent_id)).await;
    let reply_id = Uuid::parse_str(reply["id"].as_str().unwrap()).unwrap();

    // Mark thread as read
    let req = TestApp::request(
        Method::POST,
        &format!("/api/messages/{parent_id_str}/thread/read"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        204,
        "Mark thread read should return 204 No Content"
    );

    // Verify read state in DB
    let last_read: Option<Uuid> = sqlx::query_scalar(
        "SELECT last_read_message_id FROM thread_read_state WHERE user_id = $1 AND thread_parent_id = $2",
    )
    .bind(user_id)
    .bind(parent_id)
    .fetch_optional(&app.pool)
    .await
    .expect("Query should succeed")
    .flatten();

    assert_eq!(
        last_read,
        Some(reply_id),
        "last_read_message_id should point to the latest reply"
    );

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Permission Checks
// ============================================================================

#[tokio::test]
async fn test_thread_replies_require_auth() {
    let app = TestApp::new().await;
    let parent_id = Uuid::new_v4();

    let req = TestApp::request(Method::GET, &format!("/api/messages/{parent_id}/thread"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401, "Thread listing should require auth");
}

#[tokio::test]
async fn test_thread_replies_nonexistent_parent() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let fake_parent = Uuid::new_v4();

    let req = TestApp::request(Method::GET, &format!("/api/messages/{fake_parent}/thread"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404, "Should 404 for nonexistent parent");

    // Cleanup
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
async fn test_thread_reply_from_second_user() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let guild_id = create_guild_with_owner(&app.pool, user_a).await;
    add_guild_member(&app.pool, guild_id, user_b).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-multiuser-test").await;

    // User A creates parent
    let parent = send_message(&app, channel_id, &token_a, "Parent from A", None).await;
    let parent_id = Uuid::parse_str(parent["id"].as_str().unwrap()).unwrap();

    // User B replies in thread
    let reply = send_message(&app, channel_id, &token_b, "Reply from B", Some(parent_id)).await;
    assert_eq!(reply["content"], "Reply from B");
    assert_eq!(reply["parent_id"], parent["id"]);

    // Both users can list the thread
    let thread_a = list_thread_replies(&app, parent["id"].as_str().unwrap(), &token_a, None).await;
    let thread_b = list_thread_replies(&app, parent["id"].as_str().unwrap(), &token_b, None).await;
    assert_eq!(thread_a["items"].as_array().unwrap().len(), 1);
    assert_eq!(thread_b["items"].as_array().unwrap().len(), 1);

    // Parent counter updated
    let count: i32 = sqlx::query_scalar("SELECT thread_reply_count FROM messages WHERE id = $1")
        .bind(parent_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Thread Info in Parent Response
// ============================================================================

#[tokio::test]
async fn test_parent_message_includes_thread_info() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "threads-info-test").await;

    // Create parent + reply
    let parent = send_message(&app, channel_id, &token, "Parent", None).await;
    let parent_id = Uuid::parse_str(parent["id"].as_str().unwrap()).unwrap();

    send_message(&app, channel_id, &token, "Reply 1", Some(parent_id)).await;
    send_message(&app, channel_id, &token, "Reply 2", Some(parent_id)).await;

    // List channel messages — parent should have thread_reply_count = 2
    let msgs = list_channel_messages(&app, channel_id, &token).await;
    let items = msgs["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);

    let parent_msg = &items[0];
    assert_eq!(parent_msg["thread_reply_count"], 2);
    assert!(
        parent_msg["thread_last_reply_at"].is_string(),
        "thread_last_reply_at should be set"
    );

    // Cleanup
    cleanup_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}
