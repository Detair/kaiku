//! Integration tests for channel-level permissions (`VIEW_CHANNEL`)
//!
//! These tests verify that the `VIEW_CHANNEL` permission is properly enforced across all endpoints.
//! They test guild owner bypass, role-based permissions, channel overrides, and DM access control.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use super::helpers::{create_test_user, generate_access_token, TestApp};

// ============================================================================
// Test Helpers
// ============================================================================

async fn create_guild_with_owner(pool: &PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Test Guild")
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to create test guild - check database schema and constraints");

    // Add owner as member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to add owner as guild member - check foreign key constraints");

    guild_id
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
    .expect("Failed to create test channel - check database schema and constraints");

    channel_id
}

async fn create_role(pool: &PgPool, guild_id: Uuid, name: &str, permissions: i64) -> Uuid {
    let role_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guild_roles (id, guild_id, name, permissions, position) VALUES ($1, $2, $3, $4, 0)")
        .bind(role_id)
        .bind(guild_id)
        .bind(name)
        .bind(permissions)
        .execute(pool)
        .await
        .expect("Failed to create test role - check database schema and constraints");

    role_id
}

async fn assign_role(pool: &PgPool, guild_id: Uuid, user_id: Uuid, role_id: Uuid) {
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("Failed to assign role to user - check foreign key constraints");
}

async fn create_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
    allow: i64,
    deny: i64,
) -> Uuid {
    let override_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(override_id)
    .bind(channel_id)
    .bind(role_id)
    .bind(allow)
    .bind(deny)
    .execute(pool)
    .await
    .expect("Failed to create channel override - check database schema and constraints");

    override_id
}

async fn add_guild_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) {
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to add guild member - check foreign key constraints");
}

async fn create_dm_channel(pool: &PgPool, user1: Uuid, user2: Uuid) -> Uuid {
    let channel_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channels (id, name, channel_type, position, max_screen_shares)
         VALUES ($1, 'DM', 'dm', 0, 5)",
    )
    .bind(channel_id)
    .execute(pool)
    .await
    .expect("Failed to create DM channel - check database schema and constraints");

    // Add both users as participants
    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2), ($1, $3)")
        .bind(channel_id)
        .bind(user1)
        .bind(user2)
        .execute(pool)
        .await
        .expect("Failed to add DM participants - check foreign key constraints");

    channel_id
}

// Permission bits (from server/src/permissions/guild.rs)
const VIEW_CHANNEL: i64 = 1 << 24;
const SEND_MESSAGES: i64 = 1 << 3;

// ============================================================================
// Tests: Guild Owner Bypass
// ============================================================================

#[tokio::test]
async fn test_guild_owner_can_view_any_channel() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_guild_owner_bypasses_channel_overrides() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "restricted").await;

    // Create @everyone role without VIEW_CHANNEL
    let everyone_role = create_role(&app.pool, guild_id, "@everyone", 0).await;

    // Create channel override denying VIEW_CHANNEL
    create_channel_override(&app.pool, channel_id, everyone_role, 0, VIEW_CHANNEL).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// Tests: VIEW_CHANNEL Permission
// ============================================================================

#[tokio::test]
async fn test_user_with_view_channel_can_access() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    // Add user to guild and give VIEW_CHANNEL permission
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Member", VIEW_CHANNEL).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_user_without_view_channel_cannot_access() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "restricted").await;

    // Add user to guild WITHOUT VIEW_CHANNEL
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Limited", SEND_MESSAGES).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_non_guild_member_cannot_access_channel() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (outsider_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, outsider_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Tests: Channel Overrides
// ============================================================================

#[tokio::test]
async fn test_channel_override_deny_blocks_access() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "secret").await;

    // Add user with VIEW_CHANNEL role
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Member", VIEW_CHANNEL).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    // Create channel override DENYING VIEW_CHANNEL (deny wins)
    create_channel_override(&app.pool, channel_id, role_id, 0, VIEW_CHANNEL).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_channel_override_allow_grants_access() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "special").await;

    // Add user WITHOUT VIEW_CHANNEL
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Limited", 0).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    // Create channel override ALLOWING VIEW_CHANNEL
    create_channel_override(&app.pool, channel_id, role_id, VIEW_CHANNEL, 0).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_channel_override_deny_wins_over_allow() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "conflicted").await;

    // Add user to guild
    add_guild_member(&app.pool, guild_id, user_id).await;

    // Create two roles: one allows, one denies
    let allow_role = create_role(&app.pool, guild_id, "AllowRole", 0).await;
    let deny_role = create_role(&app.pool, guild_id, "DenyRole", 0).await;

    assign_role(&app.pool, guild_id, user_id, allow_role).await;
    assign_role(&app.pool, guild_id, user_id, deny_role).await;

    // Override 1: Allow VIEW_CHANNEL
    create_channel_override(&app.pool, channel_id, allow_role, VIEW_CHANNEL, 0).await;

    // Override 2: Deny VIEW_CHANNEL (deny wins)
    create_channel_override(&app.pool, channel_id, deny_role, 0, VIEW_CHANNEL).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Tests: DM Channels
// ============================================================================

#[tokio::test]
async fn test_dm_participant_can_access() {
    let app = TestApp::new().await;
    let (user1_id, _) = create_test_user(&app.pool).await;
    let (user2_id, _) = create_test_user(&app.pool).await;
    let token1 = generate_access_token(&app.config, user1_id);
    let token2 = generate_access_token(&app.config, user2_id);
    let dm_channel = create_dm_channel(&app.pool, user1_id, user2_id).await;

    // Both participants should be able to access
    let request1 = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{dm_channel}"))
        .header("Authorization", format!("Bearer {token1}"))
        .body(Body::empty())
        .unwrap();

    let request2 = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{dm_channel}"))
        .header("Authorization", format!("Bearer {token2}"))
        .body(Body::empty())
        .unwrap();

    let response1 = app.oneshot(request1).await;
    let response2 = app.oneshot(request2).await;

    assert_eq!(response1.status(), StatusCode::OK);
    assert_eq!(response2.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_non_dm_participant_cannot_access() {
    let app = TestApp::new().await;
    let (user1_id, _) = create_test_user(&app.pool).await;
    let (user2_id, _) = create_test_user(&app.pool).await;
    let (outsider_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, outsider_id);
    let dm_channel = create_dm_channel(&app.pool, user1_id, user2_id).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{dm_channel}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Tests: Message Operations
// ============================================================================

#[tokio::test]
async fn test_cannot_send_message_without_view_channel() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    // Add user with SEND_MESSAGES but NO VIEW_CHANNEL
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Member", SEND_MESSAGES).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    let body_json = json!({
        "content": "Hello",
        "nonce": Uuid::new_v4().to_string()
    });

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/channels/{channel_id}/messages"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_string(&body_json).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await;
    // Returns 404 to avoid leaking channel existence to unauthorized users
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_cannot_read_messages_without_view_channel() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    // Add user WITHOUT VIEW_CHANNEL
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Member", 0).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    let request = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/channels/{channel_id}/messages"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    // Returns 404 to avoid leaking channel existence to unauthorized users
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ============================================================================
// Tests: Favorites Endpoint Security Fix
// ============================================================================

#[tokio::test]
async fn test_cannot_favorite_channel_without_view_permission() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "restricted").await;

    // Add user to guild WITHOUT VIEW_CHANNEL
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Member", 0).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/me/favorites/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND); // Generic error to avoid info leakage
}

#[tokio::test]
async fn test_can_favorite_channel_with_view_permission() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    // Add user WITH VIEW_CHANNEL
    add_guild_member(&app.pool, guild_id, user_id).await;
    let role_id = create_role(&app.pool, guild_id, "Member", VIEW_CHANNEL).await;
    assign_role(&app.pool, guild_id, user_id, role_id).await;

    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/me/favorites/{channel_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await;
    assert_eq!(response.status(), StatusCode::OK); // Endpoint returns 200 OK, not 201 CREATED
}
