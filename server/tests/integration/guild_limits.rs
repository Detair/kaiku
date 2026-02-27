//! Integration tests for guild resource limits.

use axum::body::Body;
use axum::http::{Method, StatusCode};
use super::helpers::{
    add_guild_member, body_to_json, create_bot_application, create_channel, create_guild,
    create_test_user, delete_bot_application, delete_guild, delete_user, generate_access_token,
    TestApp,
};
use vc_server::config::Config;

/// Helper: create a Config with low limits for testing.
fn low_limit_config() -> Config {
    let mut config = Config::default_for_test();
    config.max_guilds_per_user = 2;
    config.max_members_per_guild = 2;
    config.max_channels_per_guild = 2;
    config.max_roles_per_guild = 2;
    config.max_emojis_per_guild = 1;
    config.max_bots_per_guild = 1;
    config
}

// ============================================================================
// Guild creation limit
// ============================================================================

#[tokio::test]
async fn test_guild_creation_limit() {
    let app = TestApp::with_config(low_limit_config()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    // Create 2 guilds (at limit)
    let mut guild_ids = Vec::new();
    for i in 0..2 {
        let resp = app
            .oneshot(
                TestApp::request(Method::POST, "/api/guilds")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(r#"{{"name": "LimitGuild{i}"}}"#)))
                    .unwrap(),
            )
            .await;
        assert_eq!(resp.status(), StatusCode::OK, "Guild {i} should succeed");
        let body = body_to_json(resp).await;
        guild_ids.push(body["id"].as_str().unwrap().to_string());
    }

    // 3rd guild should fail
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, "/api/guilds")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name": "LimitGuild3"}"#))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");

    // Cleanup
    for gid in &guild_ids {
        let uuid = uuid::Uuid::parse_str(gid).unwrap();
        delete_guild(&app.pool, uuid).await;
    }
}

// ============================================================================
// Member limit on invite join
// ============================================================================

#[tokio::test]
async fn test_member_limit_on_invite_join() {
    let app = TestApp::with_config(low_limit_config()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user2_id, _) = create_test_user(&app.pool).await;
    let (user3_id, _) = create_test_user(&app.pool).await;
    let owner_token = generate_access_token(&app.config, owner_id);
    let user3_token = generate_access_token(&app.config, user3_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
        delete_user(&pool, user2_id).await;
        delete_user(&pool, user3_id).await;
    });

    // Owner is already member (1/2). Add user2 (2/2).
    add_guild_member(&app.pool, guild_id, user2_id).await;

    // Create invite
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/guilds/{guild_id}/invites"))
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"expires_in": "7d"}"#))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let invite_body = body_to_json(resp).await;
    let code = invite_body["code"].as_str().unwrap();

    // user3 tries to join via invite — should fail (2/2)
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/invites/{code}/join"))
                .header("authorization", format!("Bearer {user3_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");
}

// ============================================================================
// Channel limit
// ============================================================================

#[tokio::test]
async fn test_channel_limit() {
    let app = TestApp::with_config(low_limit_config()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
    });

    // Create @everyone role with MANAGE_CHANNELS
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default) VALUES ($1, $2, 'everyone', $3, 0, true)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(guild_id)
    .bind(vc_server::permissions::GuildPermissions::MANAGE_CHANNELS.to_db())
    .execute(&app.pool)
    .await
    .unwrap();

    // Create 2 channels (at limit)
    for i in 0..2 {
        let resp = app
            .oneshot(
                TestApp::request(Method::POST, "/api/channels")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"name": "ch{i}", "channel_type": "text", "guild_id": "{guild_id}"}}"#
                    )))
                    .unwrap(),
            )
            .await;
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "Channel {i} should succeed"
        );
    }

    // 3rd channel should fail
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, "/api/channels")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"name": "ch_over", "channel_type": "text", "guild_id": "{guild_id}"}}"#
                )))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");
}

// ============================================================================
// Role limit
// ============================================================================

#[tokio::test]
async fn test_role_limit() {
    let app = TestApp::with_config(low_limit_config()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
    });

    // Create @everyone role with MANAGE_ROLES (counts as 1 of limit 2)
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default) VALUES ($1, $2, 'everyone', $3, 0, true)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(guild_id)
    .bind(vc_server::permissions::GuildPermissions::MANAGE_ROLES.to_db())
    .execute(&app.pool)
    .await
    .unwrap();

    // Create 1 more role (2/2)
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/guilds/{guild_id}/roles"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name": "Mod"}"#))
                .unwrap(),
        )
        .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "First extra role should succeed"
    );

    // Next role should fail (3/2)
    let resp = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/guilds/{guild_id}/roles"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name": "Admin"}"#))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");
}

// ============================================================================
// Bot limit
// ============================================================================

#[tokio::test]
async fn test_bot_limit() {
    let app = TestApp::with_config(low_limit_config()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();

    // Create @everyone with MANAGE_GUILD
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default) VALUES ($1, $2, 'everyone', $3, 0, true)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(guild_id)
    .bind(vc_server::permissions::GuildPermissions::MANAGE_GUILD.to_db())
    .execute(&app.pool)
    .await
    .unwrap();

    // Create 2 bots
    let (app1_id, bot1_id, _) = create_bot_application(&app.pool, owner_id).await;
    let (app2_id, bot2_id, _) = create_bot_application(&app.pool, owner_id).await;

    guard.add(move |pool| async move {
        delete_bot_application(&pool, app1_id).await;
        delete_bot_application(&pool, app2_id).await;
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
    });

    // Install first bot (1/1)
    let resp = app
        .oneshot(
            TestApp::request(
                Method::POST,
                &format!("/api/guilds/{guild_id}/bots/{bot1_id}/add"),
            )
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "First bot should succeed"
    );

    // Install second bot (2/1) — should fail
    let resp = app
        .oneshot(
            TestApp::request(
                Method::POST,
                &format!("/api/guilds/{guild_id}/bots/{bot2_id}/add"),
            )
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");
}

// ============================================================================
// Emoji limit
// ============================================================================

#[tokio::test]
async fn test_emoji_limit() {
    let app = TestApp::with_config(low_limit_config()).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
    });

    // Insert 1 emoji directly (at limit of 1)
    sqlx::query(
        "INSERT INTO guild_emojis (guild_id, name, image_url, uploaded_by) VALUES ($1, $2, $3, $4)",
    )
    .bind(guild_id)
    .bind("existing_emoji")
    .bind("https://example.com/emoji.png")
    .bind(owner_id)
    .execute(&app.pool)
    .await
    .unwrap();

    // Next emoji upload should fail (2/1) — limit check runs before multipart parsing
    let boundary = "----TestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nover_limit\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.png\"\r\n\
         Content-Type: image/png\r\n\r\nfake-png-data\r\n\
         --{boundary}--\r\n"
    );

    let resp = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/guilds/{guild_id}/emojis"))
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");
}

// ============================================================================
// Member limit on discovery join
// ============================================================================

#[tokio::test]
async fn test_member_limit_on_discovery_join() {
    let mut config = low_limit_config();
    config.enable_guild_discovery = true;
    let app = TestApp::with_config(config).await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (user2_id, _) = create_test_user(&app.pool).await;
    let (user3_id, _) = create_test_user(&app.pool).await;
    let user3_token = generate_access_token(&app.config, user3_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
        delete_user(&pool, user2_id).await;
        delete_user(&pool, user3_id).await;
    });

    // Make guild discoverable
    sqlx::query("UPDATE guilds SET discoverable = true WHERE id = $1")
        .bind(guild_id)
        .execute(&app.pool)
        .await
        .unwrap();

    // Owner is member (1/2). Add user2 (2/2).
    add_guild_member(&app.pool, guild_id, user2_id).await;

    // user3 tries to join via discovery — should fail (2/2)
    let resp = app
        .oneshot(
            TestApp::request(
                Method::POST,
                &format!("/api/discover/guilds/{guild_id}/join"),
            )
            .header("authorization", format!("Bearer {user3_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "LIMIT_EXCEEDED");
}

// ============================================================================
// Usage endpoint
// ============================================================================

#[tokio::test]
async fn test_usage_endpoint() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
    });

    // Create some channels
    create_channel(&app.pool, guild_id, "general").await;
    create_channel(&app.pool, guild_id, "random").await;

    let resp = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/usage"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp).await;

    assert_eq!(body["guild_id"], guild_id.to_string());
    assert_eq!(body["plan"], "free");
    assert_eq!(body["members"]["current"], 1); // owner
    assert_eq!(body["channels"]["current"], 2);
    assert!(body["members"]["limit"].as_i64().unwrap() > 0);
    assert!(body["channels"]["limit"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn test_usage_requires_membership() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (outsider_id, _) = create_test_user(&app.pool).await;
    let outsider_token = generate_access_token(&app.config, outsider_id);
    let guild_id = create_guild(&app.pool, owner_id).await;
    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        delete_guild(&pool, guild_id).await;
        delete_user(&pool, owner_id).await;
        delete_user(&pool, outsider_id).await;
    });

    let resp = app
        .oneshot(
            TestApp::request(Method::GET, &format!("/api/guilds/{guild_id}/usage"))
                .header("authorization", format!("Bearer {outsider_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================================
// Instance limits endpoint
// ============================================================================

#[tokio::test]
async fn test_instance_limits_endpoint() {
    let app = TestApp::new().await;

    let resp = app
        .oneshot(
            TestApp::request(Method::GET, "/api/config/limits")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_to_json(resp).await;

    // Verify shape
    assert!(body["max_guilds_per_user"].is_number());
    assert!(body["max_members_per_guild"].is_number());
    assert!(body["max_channels_per_guild"].is_number());
    assert!(body["max_roles_per_guild"].is_number());
    assert!(body["max_emojis_per_guild"].is_number());
    assert!(body["max_bots_per_guild"].is_number());
    assert!(body["max_webhooks_per_app"].is_number());
    assert!(body["max_upload_size"].is_number());
}
