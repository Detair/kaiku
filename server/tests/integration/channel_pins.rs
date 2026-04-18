//! Integration tests for channel pins API.
//!
//! Run with: `cargo test --test integration channel_pins -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, body_to_json, create_channel, create_guild_with_default_role,
    create_test_user, generate_access_token, insert_deleted_message, insert_message, TestApp,
};

// ============================================================================
// Test Helpers
// ============================================================================

/// Pin a message via the API and return the raw response.
async fn pin_message(
    app: &TestApp,
    channel_id: Uuid,
    message_id: Uuid,
    token: &str,
) -> axum::http::Response<Body> {
    let req = TestApp::request(
        Method::PUT,
        &format!("/api/channels/{channel_id}/messages/{message_id}/pin"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    app.oneshot(req).await
}

/// Unpin a message via the API and return the raw response.
async fn unpin_message(
    app: &TestApp,
    channel_id: Uuid,
    message_id: Uuid,
    token: &str,
) -> axum::http::Response<Body> {
    let req = TestApp::request(
        Method::DELETE,
        &format!("/api/channels/{channel_id}/messages/{message_id}/pin"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    app.oneshot(req).await
}

/// List pinned messages in a channel.
async fn list_pins(app: &TestApp, channel_id: Uuid, token: &str) -> serde_json::Value {
    let req = TestApp::request(Method::GET, &format!("/api/channels/{channel_id}/pins"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

/// List messages in a channel via the messages API.
async fn list_messages(app: &TestApp, channel_id: Uuid, token: &str) -> serde_json::Value {
    let req = TestApp::request(Method::GET, &format!("/api/messages/channel/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    body_to_json(resp).await
}

/// Default permissions for pin tests: view + send + pin.
fn pin_perms() -> GuildPermissions {
    GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::SEND_MESSAGES
        | GuildPermissions::PIN_MESSAGES
}

// ============================================================================
// Tests
// ============================================================================

#[sqlx::test]
async fn test_pin_message_success(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-success-test").await;

    let msg_id = insert_message(&app.pool, channel_id, user_id, "Pin me!").await;

    // Pin the message
    let resp = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp.status(), 200, "Pin should return 200");

    // Verify it appears in the pins list
    let pins = list_pins(&app, channel_id, &token).await;
    let pins_arr = pins.as_array().expect("pins should be an array");
    assert_eq!(pins_arr.len(), 1, "Should have exactly one pin");
    assert_eq!(
        pins_arr[0]["message"]["id"].as_str().unwrap(),
        msg_id.to_string(),
        "Pinned message ID should match"
    );
    assert!(
        pins_arr[0]["pinned_by"].is_string(),
        "Should include pinned_by"
    );
    assert!(
        pins_arr[0]["pinned_at"].is_string(),
        "Should include pinned_at"
    );
}

#[sqlx::test]
async fn test_unpin_message_success(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "unpin-success-test").await;

    let msg_id = insert_message(&app.pool, channel_id, user_id, "Pin then unpin").await;

    // Pin the message
    let resp = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp.status(), 200);

    // Unpin the message
    let resp = unpin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp.status(), 204, "Unpin should return 204");

    // Verify pins list is empty
    let pins = list_pins(&app, channel_id, &token).await;
    let pins_arr = pins.as_array().expect("pins should be an array");
    assert!(pins_arr.is_empty(), "Pins list should be empty after unpin");
}

#[sqlx::test]
async fn test_pin_idempotent(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-idempotent-test").await;

    let msg_id = insert_message(&app.pool, channel_id, user_id, "Pin me twice").await;

    // Pin the same message twice
    let resp1 = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp1.status(), 200, "First pin should return 200");

    let resp2 = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp2.status(), 200, "Second pin should also return 200");

    // Verify only one pin entry exists
    let pins = list_pins(&app, channel_id, &token).await;
    let pins_arr = pins.as_array().expect("pins should be an array");
    assert_eq!(
        pins_arr.len(),
        1,
        "Should have exactly one pin despite pinning twice"
    );
}

#[sqlx::test]
async fn test_pin_limit_50(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-limit-test").await;

    // Create and pin 50 messages
    for i in 1..=50 {
        let msg_id = insert_message(&app.pool, channel_id, user_id, &format!("Message {i}")).await;
        let resp = pin_message(&app, channel_id, msg_id, &token).await;
        assert_eq!(resp.status(), 200, "Pin #{i} should succeed (limit is 50)");
    }

    // 51st pin should fail with 409 (PIN_LIMIT_REACHED)
    let msg_id = insert_message(&app.pool, channel_id, user_id, "Message 51").await;
    let resp = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(
        resp.status(),
        409,
        "51st pin should return 409 (PIN_LIMIT_REACHED)"
    );

    let body = body_to_json(resp).await;
    assert_eq!(
        body["error"].as_str().unwrap(),
        "PIN_LIMIT_REACHED",
        "Error code should be PIN_LIMIT_REACHED"
    );
}

#[sqlx::test]
async fn test_pin_forbidden_without_permission(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let owner_token = generate_access_token(&app.config, owner_id);
    let user_token = generate_access_token(&app.config, user_id);

    // Guild with only VIEW_CHANNEL + SEND_MESSAGES (no PIN_MESSAGES)
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = create_guild_with_default_role(&app.pool, owner_id, perms).await;
    add_guild_member(&app.pool, guild_id, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-forbidden-test").await;

    let msg_id = insert_message(&app.pool, channel_id, owner_id, "Can't pin this").await;

    // Second user (no PIN_MESSAGES permission) tries to pin
    let resp = pin_message(&app, channel_id, msg_id, &user_token).await;
    assert_eq!(
        resp.status(),
        403,
        "User without PIN_MESSAGES should get 403"
    );

    // Owner bypasses permission checks (GuildPermissions::all())
    let resp = pin_message(&app, channel_id, msg_id, &owner_token).await;
    assert_eq!(
        resp.status(),
        200,
        "Owner should succeed regardless of @everyone role permissions"
    );
}

#[sqlx::test]
async fn test_pin_message_not_in_channel(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_a = create_channel(&app.pool, guild_id, "pin-chan-a").await;
    let channel_b = create_channel(&app.pool, guild_id, "pin-chan-b").await;

    // Insert message in channel A
    let msg_id = insert_message(&app.pool, channel_a, user_id, "I belong to A").await;

    // Try to pin message in channel B (where it doesn't belong)
    let resp = pin_message(&app, channel_b, msg_id, &token).await;
    assert_eq!(
        resp.status(),
        404,
        "Pinning a message in the wrong channel should return 404"
    );
}

#[sqlx::test]
async fn test_pin_deleted_message(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-deleted-test").await;

    // Insert a soft-deleted message
    let msg_id = insert_deleted_message(&app.pool, channel_id, user_id, "Deleted message").await;

    // Try to pin the deleted message
    let resp = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(
        resp.status(),
        404,
        "Pinning a deleted message should return 404"
    );
}

#[sqlx::test]
async fn test_system_message_on_pin(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-sysmsg-test").await;

    let msg_id = insert_message(&app.pool, channel_id, user_id, "Pin for system msg").await;

    // Pin the message
    let resp = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp.status(), 200);

    // List messages and look for the system message
    let msgs = list_messages(&app, channel_id, &token).await;
    let items = msgs["items"].as_array().expect("items should be an array");

    let system_msg = items
        .iter()
        .find(|m| m["message_type"].as_str() == Some("system"));
    assert!(
        system_msg.is_some(),
        "Should find a system message after pinning"
    );

    let system_msg = system_msg.unwrap();
    let content = system_msg["content"].as_str().unwrap();
    assert!(
        content.contains("pinned a message"),
        "System message should contain 'pinned a message', got: {content}"
    );
}

#[sqlx::test]
async fn test_pinned_field_in_message_list(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-field-test").await;

    let pinned_msg_id = insert_message(&app.pool, channel_id, user_id, "I will be pinned").await;
    let _unpinned_msg_id =
        insert_message(&app.pool, channel_id, user_id, "I will not be pinned").await;

    // Pin the first message
    let resp = pin_message(&app, channel_id, pinned_msg_id, &token).await;
    assert_eq!(resp.status(), 200);

    // List messages and check pinned field
    let msgs = list_messages(&app, channel_id, &token).await;
    let items = msgs["items"].as_array().expect("items should be an array");

    for item in items {
        let msg_type = item["message_type"].as_str().unwrap_or("user");
        if msg_type == "system" {
            // Skip system messages for this check
            continue;
        }

        let id = item["id"].as_str().unwrap();
        let pinned = item["pinned"].as_bool().unwrap_or(false);

        if id == pinned_msg_id.to_string() {
            assert!(pinned, "Pinned message should have pinned=true");
        } else {
            assert!(!pinned, "Non-pinned message should have pinned=false");
        }
    }
}

#[sqlx::test]
async fn test_cascade_on_message_delete(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_default_role(&app.pool, user_id, pin_perms()).await;
    let channel_id = create_channel(&app.pool, guild_id, "pin-cascade-test").await;

    let msg_id = insert_message(&app.pool, channel_id, user_id, "Pin then delete").await;

    // Pin the message
    let resp = pin_message(&app, channel_id, msg_id, &token).await;
    assert_eq!(resp.status(), 200);

    // Verify pin exists
    let pins = list_pins(&app, channel_id, &token).await;
    let pins_arr = pins.as_array().unwrap();
    assert_eq!(pins_arr.len(), 1, "Should have one pin before delete");

    // Delete the message via API (soft delete)
    let req = TestApp::request(Method::DELETE, &format!("/api/messages/{msg_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Message delete should return 204");

    // Verify pins list is empty (soft-deleted messages are filtered out)
    let pins = list_pins(&app, channel_id, &token).await;
    let pins_arr = pins.as_array().unwrap();
    assert!(
        pins_arr.is_empty(),
        "Pins list should be empty after message deletion"
    );
}
