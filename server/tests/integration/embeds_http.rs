//! Integration tests for bot-authored message embeds.
//! Run: `cargo test --test integration embeds_http`

use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    body_to_json, create_channel, create_guild_with_default_role, create_test_user,
    generate_access_token, TestApp,
};

/// Make an existing user a bot (sets is_bot + bot_owner_id, which a CHECK
/// constraint requires). The bot owns itself for test simplicity.
async fn make_bot(pool: &PgPool, user_id: Uuid) {
    sqlx::query("UPDATE users SET is_bot = true, bot_owner_id = $1 WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("make bot");
}

fn post_message(token: &str, channel: Uuid, body: serde_json::Value) -> axum::http::Request<Body> {
    TestApp::request(Method::POST, &format!("/api/messages/channel/{channel}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn setup(pool: &PgPool) -> (TestApp, Uuid, Uuid) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(pool).await;
    let guild = create_guild_with_default_role(
        pool,
        owner,
        GuildPermissions::SEND_MESSAGES | GuildPermissions::VIEW_CHANNEL,
    )
    .await;
    let channel = create_channel(pool, guild, "general").await;
    (app, owner, channel)
}

#[sqlx::test]
async fn bot_can_post_embed(pool: PgPool) {
    let (app, owner, channel) = setup(&pool).await;
    make_bot(&pool, owner).await;
    let token = generate_access_token(&app.config, owner);

    let body = json!({
        "content": "see card",
        "encrypted": false,
        "embeds": [{ "title": "Hello", "description": "world", "color": 16711680 }]
    });
    let res = app.oneshot(post_message(&token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let j = body_to_json(res).await;
    assert_eq!(j["embeds"][0]["title"], "Hello");
    assert_eq!(j["embeds"][0]["color"], 16711680);
}

#[sqlx::test]
async fn human_cannot_post_embed(pool: PgPool) {
    let (app, owner, channel) = setup(&pool).await;
    let token = generate_access_token(&app.config, owner); // NOT a bot

    let body = json!({ "content": "x", "encrypted": false, "embeds": [{ "title": "nope" }] });
    let res = app.oneshot(post_message(&token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn oversized_embed_rejected(pool: PgPool) {
    let (app, owner, channel) = setup(&pool).await;
    make_bot(&pool, owner).await;
    let token = generate_access_token(&app.config, owner);

    let big = "t".repeat(300);
    let body = json!({ "content": "x", "encrypted": false, "embeds": [{ "title": big }] });
    let res = app.oneshot(post_message(&token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn non_https_embed_url_rejected(pool: PgPool) {
    let (app, owner, channel) = setup(&pool).await;
    make_bot(&pool, owner).await;
    let token = generate_access_token(&app.config, owner);

    let body = json!({
        "content": "x",
        "encrypted": false,
        "embeds": [{ "title": "t", "image": "http://evil.example/x.png" }]
    });
    let res = app.oneshot(post_message(&token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn plain_message_has_no_embeds(pool: PgPool) {
    let (app, owner, channel) = setup(&pool).await;
    let token = generate_access_token(&app.config, owner);

    let body = json!({ "content": "plain", "encrypted": false });
    let res = app.oneshot(post_message(&token, channel, body)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let j = body_to_json(res).await;
    assert!(j.get("embeds").is_none() || j["embeds"].is_null());
}
