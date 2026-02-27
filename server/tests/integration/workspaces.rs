//! Personal Workspaces Integration Tests
//!
//! Run with: `cargo test --test integration workspaces -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use super::helpers::{
    body_to_json, create_channel, create_guild, create_test_user, generate_access_token, TestApp,
};

// ============================================================================
// Workspace CRUD Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_workspace() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "name": "Gaming Setup",
                "icon": "🎮"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "Should create workspace");

    let json = body_to_json(resp).await;
    assert_eq!(json["name"], "Gaming Setup");
    assert_eq!(json["icon"], "🎮");
    assert!(json["id"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_workspace_name_too_long() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let long_name = "a".repeat(101);
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": long_name })).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Should reject name > 100 chars");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_workspace_unicode_name_length() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // 100 CJK characters (300 bytes in UTF-8) should be accepted
    let cjk_name: String = "你".repeat(100);
    assert_eq!(cjk_name.chars().count(), 100);
    assert_eq!(cjk_name.len(), 300); // 3 bytes per char

    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": cjk_name })).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "100 Unicode chars should be accepted");

    // 101 CJK characters should be rejected
    let long_cjk: String = "你".repeat(101);
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": long_cjk })).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "101 Unicode chars should be rejected");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_workspace_empty_name() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "   " })).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400, "Should reject empty/whitespace name");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_create_workspace_limit_exceeded() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Fill up to the limit via direct DB inserts
    let limit = app.config.max_workspaces_per_user;
    for i in 0..limit {
        sqlx::query("INSERT INTO workspaces (owner_user_id, name, sort_order) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(format!("WS-{i}"))
            .bind(i as i32)
            .execute(&app.pool)
            .await
            .unwrap();
    }

    // Next creation should fail
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "One Too Many" })).unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403, "Should reject when limit reached");

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "LIMIT_EXCEEDED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_list_workspaces() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create 3 workspaces
    for name in &["Alpha", "Beta", "Gamma"] {
        let req = TestApp::request(Method::POST, "/api/me/workspaces")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({ "name": name })).unwrap(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await;
        assert_eq!(resp.status(), 201);
    }

    // List
    let req = TestApp::request(Method::GET, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    let arr = json.as_array().expect("Should be an array");
    assert_eq!(arr.len(), 3, "Should have 3 workspaces");
    assert!(arr[0]["entry_count"].is_number());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_list_workspaces_empty() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let req = TestApp::request(Method::GET, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    let arr = json.as_array().expect("Should be an array");
    assert!(arr.is_empty(), "Should be empty");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_workspace_with_entries() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Test WS" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // Add entry
    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "guild_id": guild_id,
                "channel_id": channel_id
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201);

    // Get workspace detail
    let req = TestApp::request(Method::GET, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["name"], "Test WS");
    let entries = json["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["channel_name"], "general");
    assert!(entries[0]["guild_name"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_workspace_not_found() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let fake_id = uuid::Uuid::new_v4();
    let req = TestApp::request(Method::GET, &format!("/api/me/workspaces/{fake_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_workspace_not_owner() {
    let app = TestApp::new().await;
    let (user1_id, _) = create_test_user(&app.pool).await;
    let (user2_id, _) = create_test_user(&app.pool).await;
    let token1 = generate_access_token(&app.config, user1_id);
    let token2 = generate_access_token(&app.config, user2_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user1_id);
    guard.delete_user(user2_id);

    // User1 creates a workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token1}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Private" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // User2 tries to access it
    let req = TestApp::request(Method::GET, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token2}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404, "Other user should get 404");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_update_workspace() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Old Name", "icon": "🔧" }))
                .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // Update
    let req = TestApp::request(Method::PATCH, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "New Name", "icon": "🎯" }))
                .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["name"], "New Name");
    assert_eq!(json["icon"], "🎯");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_update_workspace_clear_icon() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create with icon
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Icon Test", "icon": "🎮" }))
                .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();
    assert_eq!(ws_json["icon"], "🎮");

    // Clear icon by sending null
    let req = TestApp::request(Method::PATCH, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(r#"{"icon": null}"#))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert!(json["icon"].is_null(), "Icon should be cleared to null");

    // Omitting icon should NOT change it (stays null)
    // First set it again
    let req = TestApp::request(Method::PATCH, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "icon": "🔧" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json = body_to_json(resp).await;
    assert_eq!(json["icon"], "🔧");

    // Omit icon entirely — should remain "🔧"
    let req = TestApp::request(Method::PATCH, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Renamed" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["name"], "Renamed");
    assert_eq!(json["icon"], "🔧", "Icon should be unchanged when omitted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_delete_workspace() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Temp" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // Delete
    let req = TestApp::request(Method::DELETE, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Should delete workspace");

    // Verify gone
    let req = TestApp::request(Method::GET, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404, "Should be gone");
}

// ============================================================================
// Entry Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_add_entry() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "raids").await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "WS" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // Add entry
    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "guild_id": guild_id,
                "channel_id": channel_id
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201, "Should add entry");

    let json = body_to_json(resp).await;
    assert_eq!(json["channel_name"], "raids");
    assert!(json["guild_name"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_add_entry_duplicate() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "dup-test").await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Dup" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    let body = serde_json::to_string(&serde_json::json!({
        "guild_id": guild_id,
        "channel_id": channel_id
    }))
    .unwrap();

    // Add first time
    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.clone()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201);

    // Add again → duplicate
    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 409, "Should reject duplicate entry");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_add_entry_no_guild_membership() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let (other_user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    // Other user's guild — user_id is NOT a member
    let guild_id = create_guild(&app.pool, other_user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "secret").await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);
    guard.delete_user(other_user_id);

    // Create workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Test" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // Try to add entry from non-member guild
    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "guild_id": guild_id,
                "channel_id": channel_id
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404, "Should reject non-member");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_add_entry_limit_exceeded() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild(&app.pool, user_id).await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Full WS" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();
    let ws_uuid: uuid::Uuid = ws_id.parse().unwrap();

    // Fill 50 entries via direct DB inserts (creating channels for each)
    for i in 0..50 {
        let ch_id = create_channel(&app.pool, guild_id, &format!("ch-{i}")).await;
        sqlx::query(
            "INSERT INTO workspace_entries (workspace_id, guild_id, channel_id, position) VALUES ($1, $2, $3, $4)",
        )
        .bind(ws_uuid)
        .bind(guild_id)
        .bind(ch_id)
        .bind(i)
        .execute(&app.pool)
        .await
        .unwrap();
    }

    // 51st entry via API should fail
    let extra_ch = create_channel(&app.pool, guild_id, "ch-overflow").await;
    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "guild_id": guild_id,
                "channel_id": extra_ch
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403, "Should reject when entry limit reached");

    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "LIMIT_EXCEEDED");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_remove_entry() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "removable").await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create workspace + add entry
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "WS" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({
                "guild_id": guild_id,
                "channel_id": channel_id
            }))
            .unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201);
    let entry_json = body_to_json(resp).await;
    let entry_id = entry_json["id"].as_str().unwrap();

    // Remove entry
    let req = TestApp::request(
        Method::DELETE,
        &format!("/api/me/workspaces/{ws_id}/entries/{entry_id}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Should remove entry");

    // Verify gone (workspace should have 0 entries)
    let req = TestApp::request(Method::GET, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let json = body_to_json(resp).await;
    let entries = json["entries"].as_array().unwrap();
    assert!(entries.is_empty(), "Should have no entries after removal");
}

// ============================================================================
// Reorder Tests
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reorder_entries() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild(&app.pool, user_id).await;
    let ch1 = create_channel(&app.pool, guild_id, "ch-one").await;
    let ch2 = create_channel(&app.pool, guild_id, "ch-two").await;

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create workspace
    let req = TestApp::request(Method::POST, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "name": "Reorder" })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let ws_json = body_to_json(resp).await;
    let ws_id = ws_json["id"].as_str().unwrap();

    // Add two entries
    let mut entry_ids = Vec::new();
    for ch in [ch1, ch2] {
        let req = TestApp::request(Method::POST, &format!("/api/me/workspaces/{ws_id}/entries"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({
                    "guild_id": guild_id,
                    "channel_id": ch
                }))
                .unwrap(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await;
        assert_eq!(resp.status(), 201);
        let json = body_to_json(resp).await;
        entry_ids.push(json["id"].as_str().unwrap().to_string());
    }

    // Reverse order
    entry_ids.reverse();
    let req = TestApp::request(
        Method::PATCH,
        &format!("/api/me/workspaces/{ws_id}/reorder"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::to_string(&serde_json::json!({ "entry_ids": entry_ids })).unwrap(),
    ))
    .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Should reorder entries");

    // Verify new order
    let req = TestApp::request(Method::GET, &format!("/api/me/workspaces/{ws_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let json = body_to_json(resp).await;
    let entries = json["entries"].as_array().unwrap();
    assert_eq!(entries[0]["channel_name"], "ch-two");
    assert_eq!(entries[1]["channel_name"], "ch-one");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reorder_workspaces() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create 3 workspaces
    let mut ws_ids = Vec::new();
    for name in &["First", "Second", "Third"] {
        let req = TestApp::request(Method::POST, "/api/me/workspaces")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({ "name": name })).unwrap(),
            ))
            .unwrap();
        let resp = app.oneshot(req).await;
        let json = body_to_json(resp).await;
        ws_ids.push(json["id"].as_str().unwrap().to_string());
    }

    // Reverse order
    ws_ids.reverse();
    let req = TestApp::request(Method::POST, "/api/me/workspaces/reorder")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_string(&serde_json::json!({ "workspace_ids": ws_ids })).unwrap(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 204, "Should reorder workspaces");

    // Verify new order
    let req = TestApp::request(Method::GET, "/api/me/workspaces")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let json = body_to_json(resp).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr[0]["name"], "Third");
    assert_eq!(arr[1]["name"], "Second");
    assert_eq!(arr[2]["name"], "First");
}
