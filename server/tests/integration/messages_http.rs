//! HTTP Integration Tests for Message CRUD
//!
//! Tests message creation, validation, pagination, editing, deletion,
//! and nonexistent channel handling.
//!
//! Run with: `cargo test --test integration messages_http -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    body_to_json, create_test_user, generate_access_token, insert_deleted_message,
    insert_message_at, TestApp,
};

// ============================================================================
// Test Helpers
// ============================================================================

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

// ============================================================================
// Message CRUD
// ============================================================================

#[sqlx::test]
async fn test_create_message_success(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "msg-create-test").await;

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

#[sqlx::test]
async fn test_create_message_validation_errors(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id =
        super::helpers::create_channel(&app.pool, guild_id, "msg-validation-test").await;

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

#[sqlx::test]
async fn test_list_messages_pagination(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id =
        super::helpers::create_channel(&app.pool, guild_id, "msg-pagination-test").await;

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

#[sqlx::test]
async fn test_edit_message_owner_only(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_a, perms).await;
    super::helpers::add_guild_member(&app.pool, guild_id, user_b).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "msg-edit-test").await;

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

#[sqlx::test]
async fn test_delete_message_owner_only(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_a, perms).await;
    super::helpers::add_guild_member(&app.pool, guild_id, user_b).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "msg-delete-test").await;

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

#[sqlx::test]
async fn test_create_message_nonexistent_channel(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let fake_channel = Uuid::now_v7();
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
    // Returns 404 (not 403 like channels) because the message handler checks
    // channel existence before permission checks — a different code path from
    // require_channel_access used by channel endpoints.
    assert_eq!(
        resp.status(),
        404,
        "Posting to nonexistent channel should return 404"
    );
}

// ============================================================================
// Catch-up (`after` cursor)
// ============================================================================

/// Set up a user, guild and channel; returns `(user_id, token, channel_id)`.
async fn catchup_setup(app: &TestApp, name: &str) -> (Uuid, String, Uuid) {
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, name).await;
    (user_id, token, channel_id)
}

fn ids(items: &serde_json::Value) -> Vec<String> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

async fn get_status(
    app: &TestApp,
    channel_id: Uuid,
    token: &str,
    qs: &str,
) -> (u16, serde_json::Value) {
    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/channel/{channel_id}?{qs}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

#[sqlx::test]
async fn test_list_messages_after_returns_ascending_page(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-page").await;
    let mut m = Vec::new();
    for i in 1..=5 {
        let t = format!("2026-01-01T00:00:0{i}Z");
        m.push(insert_message_at(&app.pool, channel_id, user_id, &format!("m{i}"), &t).await);
    }

    let page = list_messages(&app, channel_id, &token, &format!("after={}&limit=2", m[1])).await;
    assert_eq!(
        ids(&page["items"]),
        vec![m[2].to_string(), m[3].to_string()]
    );
    assert_eq!(page["has_more"], true);
    assert_eq!(page["next_cursor"], m[3].to_string());

    let page = list_messages(&app, channel_id, &token, &format!("after={}&limit=2", m[3])).await;
    assert_eq!(ids(&page["items"]), vec![m[4].to_string()]);
    assert_eq!(page["has_more"], false);
    assert!(page["next_cursor"].is_null());
}

#[sqlx::test]
async fn test_list_messages_after_excludes_deleted_and_thread_replies(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-filter").await;
    let m1 = insert_message_at(&app.pool, channel_id, user_id, "m1", "2026-01-01T00:00:01Z").await;
    let _deleted = insert_deleted_message(&app.pool, channel_id, user_id, "gone").await;
    let reply = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, parent_id, created_at) VALUES ($1, $2, $3, 'reply', $4, NOW())",
    )
    .bind(reply)
    .bind(channel_id)
    .bind(user_id)
    .bind(m1)
    .execute(&app.pool)
    .await
    .unwrap();
    let m3 = insert_message_at(&app.pool, channel_id, user_id, "m3", "2099-01-01T00:00:00Z").await;

    let page = list_messages(&app, channel_id, &token, &format!("after={m1}")).await;
    assert_eq!(ids(&page["items"]), vec![m3.to_string()]);
}

#[sqlx::test]
async fn test_list_messages_after_and_before_is_400(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-before").await;
    let m1 = insert_message_at(&app.pool, channel_id, user_id, "m1", "2026-01-01T00:00:01Z").await;
    for other in ["before", "around"] {
        let (status, body) = get_status(
            &app,
            channel_id,
            &token,
            &format!("after={m1}&{other}={m1}"),
        )
        .await;
        assert_eq!(status, 400, "after+{other}");
        assert_eq!(body["error"], "VALIDATION_ERROR");
    }
}

#[sqlx::test]
async fn test_list_messages_after_foreign_or_unknown_anchor_is_400(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-own").await;
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild2 = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let other_channel = super::helpers::create_channel(&app.pool, guild2, "after-other").await;
    let foreign = insert_message_at(
        &app.pool,
        other_channel,
        user_id,
        "x",
        "2026-01-01T00:00:01Z",
    )
    .await;

    for anchor in [foreign, Uuid::now_v7()] {
        let (status, body) = get_status(&app, channel_id, &token, &format!("after={anchor}")).await;
        assert_eq!(status, 400);
        assert_eq!(body["error"], "VALIDATION_ERROR");
    }
}

#[sqlx::test]
async fn test_list_messages_after_cursor_advances_past_fully_blocked_page(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-blocked").await;
    let (blocked_id, _) = create_test_user(&app.pool).await;
    sqlx::query(
        "INSERT INTO friendships (id, requester_id, addressee_id, status) VALUES ($1, $2, $3, 'blocked')",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(blocked_id)
    .execute(&app.pool)
    .await
    .unwrap();

    let m1 = insert_message_at(&app.pool, channel_id, user_id, "m1", "2026-01-01T00:00:01Z").await;
    let b = insert_message_at(
        &app.pool,
        channel_id,
        blocked_id,
        "b",
        "2026-01-01T00:00:02Z",
    )
    .await;
    let m3 = insert_message_at(&app.pool, channel_id, user_id, "m3", "2026-01-01T00:00:03Z").await;

    let page = list_messages(&app, channel_id, &token, &format!("after={m1}&limit=1")).await;
    assert!(page["items"].as_array().unwrap().is_empty());
    assert_eq!(page["has_more"], true);
    assert_eq!(page["next_cursor"], b.to_string());

    let page = list_messages(&app, channel_id, &token, &format!("after={b}&limit=1")).await;
    assert_eq!(ids(&page["items"]), vec![m3.to_string()]);

    // `before` path: same rule (oldest fetched row, pre-filter).
    let page = list_messages(&app, channel_id, &token, &format!("before={m3}&limit=1")).await;
    assert!(page["items"].as_array().unwrap().is_empty());
    assert_eq!(page["next_cursor"], b.to_string());
}

#[sqlx::test]
async fn test_list_messages_after_soft_deleted_anchor_is_valid(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-deleted-anchor").await;
    let m1 = insert_message_at(&app.pool, channel_id, user_id, "m1", "2026-01-01T00:00:01Z").await;
    let m2 = insert_message_at(&app.pool, channel_id, user_id, "m2", "2026-01-01T00:00:02Z").await;
    sqlx::query("UPDATE messages SET deleted_at = NOW() WHERE id = $1")
        .bind(m1)
        .execute(&app.pool)
        .await
        .unwrap();

    let page = list_messages(&app, channel_id, &token, &format!("after={m1}")).await;
    assert_eq!(ids(&page["items"]), vec![m2.to_string()]);
}

#[sqlx::test]
async fn test_list_messages_after_created_at_tie_broken_by_id(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "after-tie").await;
    let t = "2026-01-01T00:00:01Z";
    let mut m = Vec::new();
    for i in 0..3 {
        m.push(insert_message_at(&app.pool, channel_id, user_id, &format!("t{i}"), t).await);
    }
    m.sort(); // v7 ids; ties order by id

    let page = list_messages(&app, channel_id, &token, &format!("after={}", m[0])).await;
    assert_eq!(
        ids(&page["items"]),
        vec![m[1].to_string(), m[2].to_string()]
    );
}

// ============================================================================
// Jump to a message (`around` cursor)
// ============================================================================

#[sqlx::test]
async fn test_list_messages_around_reports_older_history(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, token, channel_id) = catchup_setup(&app, "around-older").await;
    let mut msgs = Vec::new();
    for i in 0..60 {
        let at = format!("2026-01-01T00:{i:02}:00Z");
        msgs.push(insert_message_at(&app.pool, channel_id, user_id, &format!("m{i}"), &at).await);
    }
    let around = |anchor: Uuid| format!("around={anchor}&limit=50");

    // Few newer messages after the anchor (the usual "a few unread" case):
    // older history exists, so the client must be able to page back.
    let page = list_messages(&app, channel_id, &token, &around(msgs[56])).await;
    assert_eq!(page["has_more"], true);
    let got = ids(&page["items"]);
    assert_eq!(got.len(), 26 + 3, "half + 1 up to the anchor, then the 3 newer ones");
    assert_eq!(got.first().unwrap(), &msgs[31].to_string());
    assert_eq!(got.last().unwrap(), &msgs[59].to_string());

    // Exactly half + 1 messages up to the anchor: nothing older.
    let page = list_messages(&app, channel_id, &token, &around(msgs[25])).await;
    assert_eq!(page["has_more"], false);
    assert_eq!(ids(&page["items"]).first().unwrap(), &msgs[0].to_string());

    // One more: the oldest message is outside the window.
    let page = list_messages(&app, channel_id, &token, &around(msgs[26])).await;
    assert_eq!(page["has_more"], true);
    assert_eq!(ids(&page["items"]).first().unwrap(), &msgs[1].to_string());

    // Near the start of the channel.
    let page = list_messages(&app, channel_id, &token, &around(msgs[3])).await;
    assert_eq!(page["has_more"], false);
}
