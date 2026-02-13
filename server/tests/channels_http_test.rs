//! HTTP Integration Tests for Channel CRUD
//!
//! Tests channel creation, validation, permission-gated update/delete,
//! and not-found handling.
//!
//! Run with: `cargo test --test channels_http_test -- --nocapture`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{body_to_json, create_test_user, generate_access_token, TestApp};
use serial_test::serial;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

// ============================================================================
// Channel CRUD
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_create_channel_success() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let body = serde_json::json!({
        "name": "new-channel",
        "channel_type": "text",
        "guild_id": guild_id,
    });
    let req = TestApp::request(Method::POST, "/api/channels")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "Channel creation should return 201");

    let json = body_to_json(resp).await;
    assert!(json["id"].is_string(), "Response should have id");
    assert_eq!(json["name"], "new-channel");
    assert_eq!(json["channel_type"], "text");
    assert_eq!(json["guild_id"], guild_id.to_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_create_channel_validation_errors() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Empty name → 400
    let body = serde_json::json!({
        "name": "",
        "channel_type": "text",
        "guild_id": guild_id,
    });
    let req = TestApp::request(Method::POST, "/api/channels")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Empty name should return 400");

    // Invalid channel_type → 400
    let body = serde_json::json!({
        "name": "valid-name",
        "channel_type": "invalid_type",
        "guild_id": guild_id,
    });
    let req = TestApp::request(Method::POST, "/api/channels")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Invalid channel type should return 400");

    // Voice channel with user_limit=100 → 400 (max is 99)
    let body = serde_json::json!({
        "name": "voice-channel",
        "channel_type": "voice",
        "guild_id": guild_id,
        "user_limit": 100,
    });
    let req = TestApp::request(Method::POST, "/api/channels")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        400,
        "Voice channel with user_limit=100 should return 400"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_update_channel_requires_manage_channels() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;
    let token_owner = generate_access_token(&app.config, owner_id);
    let token_member = generate_access_token(&app.config, member_id);

    // @everyone has VIEW_CHANNEL + SEND_MESSAGES but NOT MANAGE_CHANNELS
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = helpers::create_guild_with_default_role(&app.pool, owner_id, perms).await;
    helpers::add_guild_member(&app.pool, guild_id, member_id).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "perm-update-test").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(owner_id);
    guard.delete_user(member_id);

    let body = serde_json::json!({ "name": "renamed-by-member" });

    // Member without MANAGE_CHANNELS → 403
    let req = TestApp::request(Method::PATCH, &format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token_member}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        403,
        "Member without MANAGE_CHANNELS should get 403"
    );

    // Owner can always update (owner bypass)
    let body = serde_json::json!({ "name": "renamed-by-owner" });
    let req = TestApp::request(Method::PATCH, &format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token_owner}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Owner should be able to update");

    let json = body_to_json(resp).await;
    assert_eq!(json["name"], "renamed-by-owner");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_delete_channel_requires_manage_channels() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;
    let token_owner = generate_access_token(&app.config, owner_id);
    let token_member = generate_access_token(&app.config, member_id);

    // @everyone has VIEW_CHANNEL + SEND_MESSAGES but NOT MANAGE_CHANNELS
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = helpers::create_guild_with_default_role(&app.pool, owner_id, perms).await;
    helpers::add_guild_member(&app.pool, guild_id, member_id).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "perm-delete-test").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(owner_id);
    guard.delete_user(member_id);

    // Member without MANAGE_CHANNELS → 403
    let req = TestApp::request(Method::DELETE, &format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token_member}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        403,
        "Member without MANAGE_CHANNELS should get 403"
    );

    // Owner can always delete (owner bypass)
    let req = TestApp::request(Method::DELETE, &format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token_owner}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Owner should be able to delete");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_get_channel_not_found() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let fake_id = Uuid::now_v7();
    let req = TestApp::request(Method::GET, &format!("/api/channels/{fake_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    // require_channel_access returns Forbidden for non-existent channels
    // (deliberate: avoids leaking channel existence)
    assert!(
        resp.status() == 403 || resp.status() == 404,
        "Non-existent channel should return 403 or 404, got {}",
        resp.status()
    );
}
