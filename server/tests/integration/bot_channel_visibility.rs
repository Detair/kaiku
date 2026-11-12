//! Bots and their applications' webhooks receive a channel's messages only
//! when the bot can view that channel (roles + overrides, like a member).

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use fred::prelude::*;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    create_bot_application, create_channel, create_guild_with_default_role, create_test_user,
    create_test_webhook, generate_access_token, install_bot_in_guild, TestApp,
};

const QUIET: Duration = Duration::from_millis(1500);

struct Fx {
    app: TestApp,
    owner: Uuid,
    owner_token: String,
    guild: Uuid,
    app_id: Uuid,
    bot: Uuid,
    public: Uuid,
    private: Uuid,
    insiders: Uuid,
}

async fn everyone_role(pool: &PgPool, guild: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
        .bind(guild)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn set_override(
    pool: &PgPool,
    channel: Uuid,
    role: Uuid,
    allow: GuildPermissions,
    deny: GuildPermissions,
) {
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(channel)
    .bind(role)
    .bind(allow.to_db())
    .bind(deny.to_db())
    .execute(pool)
    .await
    .unwrap();
}

/// Guild with a public channel and a private one (hidden from `@everyone`,
/// open to role "insiders"); a bot with the `messages` intent is installed.
async fn fixture(pool: &PgPool) -> Fx {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(
        pool,
        owner,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    let public = create_channel(pool, guild, "public").await;
    let private = create_channel(pool, guild, "private").await;
    let everyone = everyone_role(pool, guild).await;
    let insiders = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position) VALUES ($1, $2, 'insiders', 0, 1)",
    )
    .bind(insiders)
    .bind(guild)
    .execute(pool)
    .await
    .unwrap();
    set_override(
        pool,
        private,
        everyone,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    set_override(
        pool,
        private,
        insiders,
        GuildPermissions::VIEW_CHANNEL,
        GuildPermissions::empty(),
    )
    .await;

    let (app_id, bot, _token) = create_bot_application(pool, owner).await;
    sqlx::query(
        "UPDATE bot_applications SET gateway_intents = ARRAY['commands','messages'] WHERE id = $1",
    )
    .bind(app_id)
    .execute(pool)
    .await
    .unwrap();
    install_bot_in_guild(pool, guild, app_id, owner).await;
    let owner_token = generate_access_token(&app.config, owner);
    Fx {
        app,
        owner,
        owner_token,
        guild,
        app_id,
        bot,
        public,
        private,
        insiders,
    }
}

/// Subscribe to the bot's event channel.
async fn bot_events(
    fx: &Fx,
) -> (
    Client,
    tokio::sync::broadcast::Receiver<fred::types::Message>,
) {
    let subscriber = vc_server::db::create_redis_client(&fx.app.config.redis_url)
        .await
        .unwrap();
    let _ = subscriber.connect();
    subscriber.wait_for_connect().await.unwrap();
    let rx = subscriber.message_rx();
    subscriber
        .subscribe(format!("bot:{}", fx.bot))
        .await
        .unwrap();
    (subscriber, rx)
}

async fn post(fx: &Fx, channel: Uuid, content: &str) -> Uuid {
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{channel}"))
        .header("Authorization", format!("Bearer {}", fx.owner_token))
        .header("Content-Type", "application/json")
        .body(Body::from(json!({ "content": content }).to_string()))
        .unwrap();
    let resp = fx.app.oneshot(req).await;
    assert_eq!(resp.status(), 201);
    let body = super::helpers::body_to_json(resp).await;
    body["id"].as_str().unwrap().parse().unwrap()
}

/// Next `message_created` for the bot, or `None` within `QUIET`.
async fn next_message(
    rx: &mut tokio::sync::broadcast::Receiver<fred::types::Message>,
) -> Option<serde_json::Value> {
    let deadline = tokio::time::Instant::now() + QUIET;
    loop {
        let msg = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .ok()?
            .ok()?;
        let text = String::from_utf8(msg.value.as_bytes()?.to_vec()).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        if v["type"] == "message_created" {
            return Some(v);
        }
    }
}

/// Regression guard: passes on the code before this change too (positive control for the visibility
/// filter; delivery to public channels worked before).
#[sqlx::test]
async fn public_channel_delivers_to_bot(pool: PgPool) {
    let fx = fixture(&pool).await;
    let (_sub, mut rx) = bot_events(&fx).await;
    let id = post(&fx, fx.public, "hello").await;
    let ev = next_message(&mut rx)
        .await
        .expect("event for a public channel");
    assert_eq!(ev["message_id"], id.to_string());
}

#[sqlx::test]
async fn private_channel_hidden_from_bot(pool: PgPool) {
    let fx = fixture(&pool).await;
    let (_sub, mut rx) = bot_events(&fx).await;
    post(&fx, fx.private, "secret").await;
    assert!(
        next_message(&mut rx).await.is_none(),
        "bot saw a private channel"
    );
}

#[sqlx::test]
async fn role_lets_bot_into_private_channel(pool: PgPool) {
    let fx = fixture(&pool).await;
    let req = TestApp::request(
        Method::POST,
        &format!(
            "/api/guilds/{}/members/{}/roles/{}",
            fx.guild, fx.bot, fx.insiders
        ),
    )
    .header("Authorization", format!("Bearer {}", fx.owner_token))
    .body(Body::empty())
    .unwrap();
    assert_eq!(fx.app.oneshot(req).await.status(), 200);

    let (_sub, mut rx) = bot_events(&fx).await;
    let id = post(&fx, fx.private, "for insiders").await;
    let ev = next_message(&mut rx)
        .await
        .expect("bot with role sees channel");
    assert_eq!(ev["message_id"], id.to_string());
}

#[sqlx::test]
async fn role_deny_hides_channel_from_bot(pool: PgPool) {
    let fx = fixture(&pool).await;
    let muted = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position) VALUES ($1, $2, 'muted', 0, 2)",
    )
    .bind(muted)
    .bind(fx.guild)
    .execute(&pool)
    .await
    .unwrap();
    set_override(
        &pool,
        fx.public,
        muted,
        GuildPermissions::empty(),
        GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(fx.guild)
        .bind(fx.bot)
        .bind(muted)
        .execute(&pool)
        .await
        .unwrap();

    let (_sub, mut rx) = bot_events(&fx).await;
    post(&fx, fx.public, "hidden").await;
    assert!(next_message(&mut rx).await.is_none());
}

#[sqlx::test]
async fn webhook_message_created_respects_visibility(pool: PgPool) {
    let fx = fixture(&pool).await;
    let url = format!("https://hooks.invalid/{}", Uuid::now_v7());
    create_test_webhook(&pool, fx.app_id, &url, &["message.created"]).await;

    post(&fx, fx.private, "secret").await;
    post(&fx, fx.public, "hello").await;

    // Dispatch runs in a spawned task; wait for the public item.
    let redis = &fx.app.state.redis;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mine = loop {
        let items: Vec<String> = redis
            .lrange("webhook:delivery:queue", 0, -1)
            .await
            .unwrap_or_default();
        let mine: Vec<serde_json::Value> = items
            .iter()
            .filter_map(|i| serde_json::from_str::<serde_json::Value>(i).ok())
            .filter(|v| v["url"] == url)
            .collect();
        if !mine.is_empty() || std::time::Instant::now() > deadline {
            break mine;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(mine.len(), 1, "exactly the public message: {mine:?}");
    assert_eq!(mine[0]["payload"]["channel_id"], fx.public.to_string());
}

#[sqlx::test]
async fn uninstalled_bot_loses_roles(pool: PgPool) {
    let fx = fixture(&pool).await;
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(fx.guild)
        .bind(fx.bot)
        .bind(fx.insiders)
        .execute(&pool)
        .await
        .unwrap();
    let req = TestApp::request(
        Method::DELETE,
        &format!("/api/guilds/{}/bots/{}", fx.guild, fx.bot),
    )
    .header("Authorization", format!("Bearer {}", fx.owner_token))
    .body(Body::empty())
    .unwrap();
    assert_eq!(fx.app.oneshot(req).await.status(), 204);
    let roles: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM guild_member_roles WHERE user_id = $1")
            .bind(fx.bot)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(roles, 0);
    let _ = fx.owner;
}

/// Regression guard: passes on the code before this change too (role assignment refused non-members
/// before bots could hold roles).
#[sqlx::test]
async fn role_for_user_outside_guild_is_rejected(pool: PgPool) {
    let fx = fixture(&pool).await;
    let (stranger, _) = create_test_user(&pool).await;
    let req = TestApp::request(
        Method::POST,
        &format!(
            "/api/guilds/{}/members/{}/roles/{}",
            fx.guild, stranger, fx.insiders
        ),
    )
    .header("Authorization", format!("Bearer {}", fx.owner_token))
    .body(Body::empty())
    .unwrap();
    assert_eq!(fx.app.oneshot(req).await.status(), 400);
}

/// Next interaction event (`command_invoked`) for the bot within `QUIET`.
async fn next_command(
    rx: &mut tokio::sync::broadcast::Receiver<fred::types::Message>,
) -> Option<serde_json::Value> {
    let deadline = tokio::time::Instant::now() + QUIET;
    loop {
        let msg = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .ok()?
            .ok()?;
        let text = String::from_utf8(msg.value.as_bytes()?.to_vec()).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        if v["type"] == "command_invoked" {
            return Some(v);
        }
    }
}

async fn webhook_items(fx: &Fx, url: &str) -> Vec<serde_json::Value> {
    let items: Vec<String> = fx
        .app
        .state
        .redis
        .lrange("webhook:delivery:queue", 0, -1)
        .await
        .unwrap_or_default();
    items
        .iter()
        .filter_map(|i| serde_json::from_str::<serde_json::Value>(i).ok())
        .filter(|v| v["url"] == url)
        .collect()
}

/// A slash command in a channel the bot cannot view is refused with
/// `BOT_NOT_ALLOWED_IN_CHANNEL`; neither the gateway nor the bot's webhook
/// hears about it. The same command in a public channel goes through.
#[sqlx::test]
async fn command_in_hidden_channel_reaches_no_bot(pool: PgPool) {
    let fx = fixture(&pool).await;
    sqlx::query(
        "INSERT INTO slash_commands (application_id, name, description) VALUES ($1, 'peek', 'Peek')",
    )
    .bind(fx.app_id)
    .execute(&pool)
    .await
    .unwrap();
    let url = format!("https://hooks.invalid/{}", Uuid::now_v7());
    create_test_webhook(&pool, fx.app_id, &url, &["command.invoked"]).await;
    let (_sub, mut rx) = bot_events(&fx).await;

    let invoke = |channel: Uuid| {
        TestApp::request(Method::POST, &format!("/api/messages/channel/{channel}"))
            .header("Authorization", format!("Bearer {}", fx.owner_token))
            .header("Content-Type", "application/json")
            .body(Body::from(json!({ "content": "/peek now" }).to_string()))
            .unwrap()
    };

    let resp = fx.app.oneshot(invoke(fx.private)).await;
    assert_eq!(resp.status(), 403);
    let body = super::helpers::body_to_json(resp).await;
    assert_eq!(body["error"], "BOT_NOT_ALLOWED_IN_CHANNEL");
    assert!(
        next_command(&mut rx).await.is_none(),
        "bot heard a command from a hidden channel"
    );
    assert!(webhook_items(&fx, &url).await.is_empty());

    // Positive control: the public channel dispatches to both.
    let resp = fx.app.oneshot(invoke(fx.public)).await;
    assert_eq!(resp.status(), 202);
    let ev = next_command(&mut rx).await.expect("public command");
    assert_eq!(ev["channel_id"], fx.public.to_string());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mine = loop {
        let mine = webhook_items(&fx, &url).await;
        if !mine.is_empty() || std::time::Instant::now() > deadline {
            break mine;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert_eq!(mine.len(), 1, "one webhook item for the public channel");
    assert_eq!(mine[0]["payload"]["channel_id"], fx.public.to_string());
}
