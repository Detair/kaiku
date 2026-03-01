//! HTTP Integration Tests for Content Filters
//!
//! Tests filter configuration CRUD, custom patterns, message blocking,
//! moderation log, and edge cases (encrypted, DM, cache invalidation).
//!
//! Run with: `cargo test --test integration filters_http -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{body_to_json, create_test_user, generate_access_token, TestApp};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a guild with `MANAGE_GUILD` + `SEND_MESSAGES` + `VIEW_CHANNEL`.
async fn setup_guild_with_filters(app: &TestApp) -> (Uuid, Uuid, Uuid, String) {
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::SEND_MESSAGES
        | GuildPermissions::MANAGE_GUILD;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "filter-test").await;
    (user_id, guild_id, channel_id, token)
}

/// Enable a filter category via the API.
async fn enable_filter_category(
    app: &TestApp,
    guild_id: Uuid,
    token: &str,
    category: &str,
    action: &str,
) {
    let body = serde_json::json!({
        "configs": [{
            "category": category,
            "enabled": true,
            "action": action,
        }]
    });
    let req = TestApp::request(Method::PUT, &format!("/api/guilds/{guild_id}/filters"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Expected 200 for filter config update");
}

/// Create a custom pattern via the API.
async fn create_pattern(
    app: &TestApp,
    guild_id: Uuid,
    token: &str,
    pattern: &str,
    is_regex: bool,
) -> serde_json::Value {
    let body = serde_json::json!({
        "pattern": pattern,
        "is_regex": is_regex,
    });
    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild_id}/filters/patterns"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "Expected 201 for pattern creation");
    body_to_json(resp).await
}

/// Send a message and return (`status_code`, `response_json`).
async fn send_message_raw(
    app: &TestApp,
    channel_id: Uuid,
    token: &str,
    content: &str,
) -> (u16, serde_json::Value) {
    let body = serde_json::json!({ "content": content });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let json = body_to_json(resp).await;
    (status, json)
}

/// Send an encrypted message.
async fn send_encrypted_message(
    app: &TestApp,
    channel_id: Uuid,
    token: &str,
    content: &str,
) -> (u16, serde_json::Value) {
    let body = serde_json::json!({
        "content": content,
        "encrypted": true,
        "nonce": "dGVzdF9ub25jZQ==",
    });
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let json = body_to_json(resp).await;
    (status, json)
}

/// Edit a message and return (`status_code`, `response_json`).
async fn edit_message_raw(
    app: &TestApp,
    message_id: &str,
    token: &str,
    content: &str,
) -> (u16, serde_json::Value) {
    let body = serde_json::json!({ "content": content });
    let req = TestApp::request(Method::PATCH, &format!("/api/messages/{message_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let json = body_to_json(resp).await;
    (status, json)
}

// ============================================================================
// Filter Config CRUD
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_list_filter_configs_empty() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/filters"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert!(json.as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_enable_and_list_filter_category() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    enable_filter_category(&app, guild_id, &token, "spam", "block").await;

    let req = TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/filters"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    let configs = json.as_array().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["category"], "spam");
    assert_eq!(configs[0]["enabled"], true);
    assert_eq!(configs[0]["action"], "block");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_disable_filter_category() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Enable then disable
    enable_filter_category(&app, guild_id, &token, "spam", "block").await;

    let body = serde_json::json!({
        "configs": [{ "category": "spam", "enabled": false, "action": "block" }]
    });
    let req = TestApp::request(Method::PUT, &format!("/api/guilds/{guild_id}/filters"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert_eq!(json[0]["enabled"], false);
}

// ============================================================================
// Custom Patterns
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_custom_keyword_pattern() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let pattern = create_pattern(&app, guild_id, &token, "badword", false).await;
    assert_eq!(pattern["pattern"], "badword");
    assert_eq!(pattern["is_regex"], false);
    assert_eq!(pattern["enabled"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_custom_regex_pattern() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let pattern = create_pattern(&app, guild_id, &token, r"(?i)bad\s+word", true).await;
    assert_eq!(pattern["is_regex"], true);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_invalid_regex_rejected() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let body = serde_json::json!({
        "pattern": "[invalid",
        "is_regex": true,
    });
    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild_id}/filters/patterns"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Invalid regex should be rejected");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_delete_custom_pattern() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let pattern = create_pattern(&app, guild_id, &token, "deleteme", false).await;
    let pattern_id = pattern["id"].as_str().unwrap();

    let req = TestApp::request(
        Method::DELETE,
        &format!("/api/guilds/{guild_id}/filters/patterns/{pattern_id}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Expected 204 No Content for delete");
}

// ============================================================================
// Message Blocking
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_message_blocked_by_custom_keyword() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Add custom keyword
    create_pattern(&app, guild_id, &token, "forbidden", false).await;

    // Send message containing the keyword
    let (status, json) =
        send_message_raw(&app, channel_id, &token, "this is forbidden content").await;
    assert_eq!(status, 403, "Blocked message should return 403");
    assert_eq!(json["error"], "CONTENT_FILTERED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_clean_message_allowed() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Add custom keyword
    create_pattern(&app, guild_id, &token, "forbidden", false).await;

    // Send clean message
    let (status, _) = send_message_raw(&app, channel_id, &token, "this is perfectly fine").await;
    assert_eq!(status, 201, "Clean message should be allowed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_edit_blocked_by_filter() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Create clean message first
    let (status, msg) = send_message_raw(&app, channel_id, &token, "clean message").await;
    assert_eq!(status, 201);
    let msg_id = msg["id"].as_str().unwrap();

    // Add filter
    create_pattern(&app, guild_id, &token, "forbidden", false).await;

    // Edit to add blocked word
    let (status, json) = edit_message_raw(&app, msg_id, &token, "now this is forbidden").await;
    assert_eq!(
        status, 403,
        "Edited message with blocked word should return 403"
    );
    assert_eq!(json["error"], "CONTENT_FILTERED");
}

// ============================================================================
// Encrypted & DM Skip
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_encrypted_message_not_filtered() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Add filter
    create_pattern(&app, guild_id, &token, "forbidden", false).await;

    // Send encrypted message with blocked word
    let (status, _) = send_encrypted_message(&app, channel_id, &token, "forbidden content").await;
    assert_eq!(
        status, 201,
        "Encrypted message should bypass content filter"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_dm_message_not_filtered() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let dm_channel = super::helpers::create_dm_channel(&app.pool, user_a, user_b).await;

    let mut guard = app.cleanup_guard();
    guard
        .add(move |pool| async move { super::helpers::delete_dm_channel(&pool, dm_channel).await });
    guard.delete_user(user_a);
    guard.delete_user(user_b);

    // DMs have no guild_id, so filters don't apply
    let (status, _) = send_message_raw(&app, dm_channel, &token_a, "anything goes in DMs").await;
    assert_eq!(status, 201, "DM messages should not be filtered");
}

// ============================================================================
// Log Action (non-block)
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_log_action_allows_message_but_creates_log() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Enable spam category with "log" action
    enable_filter_category(&app, guild_id, &token, "spam", "log").await;

    // Send message that matches spam pattern
    let (status, _) = send_message_raw(
        &app,
        channel_id,
        &token,
        "click here to claim your free prize!",
    )
    .await;
    assert_eq!(status, 201, "Log action should allow the message through");

    // Check moderation log has an entry
    let req = TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/filters/log"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert!(
        json["total"].as_i64().unwrap() > 0,
        "Moderation log should have entries from log action"
    );
}

// ============================================================================
// Moderation Log
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_moderation_log_pagination() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Add filter and trigger some blocks
    create_pattern(&app, guild_id, &token, "forbidden", false).await;
    for _ in 0..3 {
        let _ = send_message_raw(&app, channel_id, &token, "this is forbidden").await;
    }

    let req = TestApp::request(
        Method::GET,
        &format!("/api/guilds/{guild_id}/filters/log?limit=2&offset=0"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert_eq!(json["items"].as_array().unwrap().len(), 2);
    assert!(json["total"].as_i64().unwrap() >= 3);
}

// ============================================================================
// Permission Checks
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_admin_cannot_modify_filters() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;
    let member_token = generate_access_token(&app.config, member_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, owner_id, perms).await;
    super::helpers::add_guild_member(&app.pool, guild_id, member_id).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(owner_id);
    guard.delete_user(member_id);

    // Try to list configs as non-admin
    let req = TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/filters"))
        .header("Authorization", format!("Bearer {member_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403, "Non-admin should get 403");
}

// ============================================================================
// Test Endpoint
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_filter_dry_run() {
    let app = TestApp::new().await;
    let (user_id, guild_id, _, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Add a keyword
    create_pattern(&app, guild_id, &token, "testblock", false).await;

    // Test matching content
    let body = serde_json::json!({ "content": "this contains testblock" });
    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild_id}/filters/test"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert_eq!(json["blocked"], true);
    assert!(!json["matches"].as_array().unwrap().is_empty());

    // Test non-matching content
    let body = serde_json::json!({ "content": "this is clean" });
    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild_id}/filters/test"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(serde_json::to_string(&body).unwrap()))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert_eq!(json["blocked"], false);
    assert!(json["matches"].as_array().unwrap().is_empty());
}

// ============================================================================
// Cache Invalidation
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cache_invalidation_on_config_change() {
    let app = TestApp::new().await;
    let (user_id, guild_id, channel_id, token) = setup_guild_with_filters(&app).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Message should pass with no filters
    let (status, _) = send_message_raw(&app, channel_id, &token, "forbidden content").await;
    assert_eq!(status, 201);

    // Add filter
    create_pattern(&app, guild_id, &token, "forbidden", false).await;

    // Same message should now be blocked
    let (status, json) = send_message_raw(&app, channel_id, &token, "forbidden content").await;
    assert_eq!(
        status, 403,
        "After adding filter, message should be blocked"
    );
    assert_eq!(json["error"], "CONTENT_FILTERED");
}
