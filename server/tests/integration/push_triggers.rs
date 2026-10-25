//! Push triggers: DM, mention and file-message wakes, coalescing and read
//! clearing (contract C20), end to end through the FCM mock.
//! Run: `cargo test --test integration push_triggers`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::permissions::GuildPermissions;

use crate::helpers::{
    add_guild_member, body_to_json, create_channel, create_dm_channel,
    create_guild_with_default_role, create_test_user, fresh_test_app_with_s3_and_config,
    generate_access_token, insert_message, rustfs_available, spawn_fcm_mock, test_service_account,
    FcmMock, FcmMockResponse, TestApp,
};

/// Upper bound for a push to arrive.
const ARRIVAL: Duration = Duration::from_secs(5);
/// How long a "no (further) push" assertion waits.
const QUIET: Duration = Duration::from_secs(1);

fn fcm_config(mock: &FcmMock) -> Config {
    let mut config = Config::default_for_test();
    config.fcm_service_account = Some(test_service_account(mock.token_uri()));
    config.fcm_api_base.clone_from(&mock.base_url);
    config
}

async fn fcm_app(pool: PgPool) -> (TestApp, FcmMock) {
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let app = TestApp::with_pool_and_config(pool, fcm_config(&mock)).await;
    (app, mock)
}

/// A user with one FCM device; returns (id, username, token, fcm token).
async fn phone_user(app: &TestApp) -> (Uuid, String, String, String) {
    let (id, name) = create_test_user(&app.pool).await;
    let fcm_token = format!("fcm-{}", id.simple());
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id)
         VALUES ($1, 'fcm', $2, $3)",
    )
    .bind(id)
    .bind(&fcm_token)
    .bind(Uuid::now_v7())
    .execute(&app.pool)
    .await
    .unwrap();
    (id, name, generate_access_token(&app.config, id), fcm_token)
}

async fn post_json(app: &TestApp, token: &str, uri: &str, body: &Value) -> (u16, Value) {
    let req = TestApp::request(Method::POST, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    if status == 204 {
        return (status, Value::Null);
    }
    (status, body_to_json(resp).await)
}

/// Send a text message; returns its id.
async fn send(app: &TestApp, token: &str, channel: Uuid, content: &str) -> Uuid {
    let (status, body) = post_json(
        app,
        token,
        &format!("/api/messages/channel/{channel}"),
        &json!({ "content": content }),
    )
    .await;
    assert_eq!(status, 201, "send failed: {body}");
    body["id"].as_str().unwrap().parse().unwrap()
}

fn received(mock: &FcmMock) -> Vec<Value> {
    mock.received.lock().unwrap().clone()
}

/// Poll until at least `n` pushes arrived (up to [`ARRIVAL`]), then let
/// [`QUIET`] pass so a stray extra push would show up too.
async fn settle(mock: &FcmMock, n: usize) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + ARRIVAL;
    while received(mock).len() < n && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tokio::time::sleep(QUIET).await;
    received(mock)
}

fn msg(push: &Value) -> &Value {
    &push["message"]
}

#[sqlx::test]
async fn test_dm_text_message_pushes_recipient_not_author(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let (a, _, token_a, _) = phone_user(&app).await;
    let (b, _, _, fcm_b) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;

    send(&app, &token_a, dm, "hi").await;

    let got = settle(&mock, 1).await;
    assert_eq!(got.len(), 1, "{got:?}");
    let m = msg(&got[0]);
    assert_eq!(m["token"], fcm_b.as_str());
    assert_eq!(m["data"]["t"], "dm");
    assert_eq!(m["data"]["channel_id"], dm.to_string());
    assert_eq!(m["data"]["count"], "1");
    assert_eq!(m["android"]["priority"], "HIGH");
}

/// Wake targeting reads blocks from Postgres, not the expiring Redis cache: a
/// DB-only block means no push for the blocker.
#[sqlx::test]
async fn test_dm_from_blocked_user_does_not_push(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let (a, _, token_a, _) = phone_user(&app).await;
    let (b, _, _, _) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;
    // Seed a block row without touching the Redis cache; target resolution must
    // still drop the blocker. (The send itself is refused by the send path, so
    // the wake logic is exercised directly.)
    sqlx::query(
        "INSERT INTO friendships (requester_id, addressee_id, status) VALUES ($1, $2, 'blocked')",
    )
    .bind(b)
    .bind(a)
    .execute(&app.pool)
    .await
    .unwrap();

    let channel = vc_server::db::get_channel_by_id(&app.pool, dm)
        .await
        .unwrap()
        .expect("channel");
    let targets =
        vc_server::push::targets::wake_targets(&app.state, &channel, a, "hi", false).await;
    assert!(targets.is_empty(), "{targets:?}");

    // And end to end the send is refused, so nothing is pushed.
    let (status, _) = post_json(
        &app,
        &token_a,
        &format!("/api/messages/channel/{dm}"),
        &json!({ "content": "hi" }),
    )
    .await;
    assert_eq!(status, 403);
    assert!(settle(&mock, 1).await.is_empty());
}

/// Guild with A (owner), B and C as members of one text channel.
struct GuildFixture {
    guild_id: Uuid,
    channel: Uuid,
    a: Uuid,
    token_a: String,
    b: Uuid,
    b_name: String,
    token_b: String,
    fcm_b: String,
    fcm_c: String,
}

async fn guild_fixture(app: &TestApp) -> GuildFixture {
    let (a, _, token_a, _) = phone_user(app).await;
    let guild_id = create_guild_with_default_role(
        &app.pool,
        a,
        GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
    )
    .await;
    let channel = create_channel(&app.pool, guild_id, "general").await;
    let (b, b_name, token_b, fcm_b) = phone_user(app).await;
    let (c, _, _, fcm_c) = phone_user(app).await;
    add_guild_member(&app.pool, guild_id, b).await;
    add_guild_member(&app.pool, guild_id, c).await;
    GuildFixture {
        guild_id,
        channel,
        a,
        token_a,
        b,
        b_name,
        token_b,
        fcm_b,
        fcm_c,
    }
}

#[sqlx::test]
async fn test_guild_mention_pushes_only_mentioned_member(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let g = guild_fixture(&app).await;

    send(&app, &g.token_a, g.channel, &format!("@{} hi", g.b_name)).await;

    let got = settle(&mock, 1).await;
    assert_eq!(got.len(), 1, "{got:?}");
    let m = msg(&got[0]);
    assert_eq!(m["token"], g.fcm_b.as_str());
    assert_ne!(m["token"], g.fcm_c.as_str());
    assert_eq!(m["data"]["t"], "mention");
    assert_eq!(m["data"]["channel_id"], g.channel.to_string());
}

#[sqlx::test]
async fn test_plain_guild_message_does_not_push(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let g = guild_fixture(&app).await;

    send(
        &app,
        &g.token_a,
        g.channel,
        "hello everyone, no mention here",
    )
    .await;

    tokio::time::sleep(QUIET).await;
    assert!(received(&mock).is_empty(), "{:?}", received(&mock));
}

#[sqlx::test]
async fn test_dm_burst_is_coalesced(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let (a, _, token_a, _) = phone_user(&app).await;
    let (b, _, _, _) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;

    let start = tokio::time::Instant::now();
    for i in 0..5 {
        send(&app, &token_a, dm, &format!("msg {i}")).await;
    }
    let deadline = start + Duration::from_secs(2);
    while received(&mock).is_empty() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tokio::time::sleep_until(deadline.max(tokio::time::Instant::now() + QUIET)).await;
    let got = received(&mock);
    assert_eq!(got.len(), 1, "one leading push, the rest trail: {got:?}");
    assert_eq!(msg(&got[0])["data"]["count"], "1");
}

#[sqlx::test]
async fn test_mark_read_sends_read_signal_after_wake(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let (a, _, token_a, _) = phone_user(&app).await;
    let (b, _, token_b, fcm_b) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;

    let message_id = send(&app, &token_a, dm, "hi").await;
    assert_eq!(settle(&mock, 1).await.len(), 1);

    let read_body = json!({ "last_read_message_id": message_id });
    let (status, _) = post_json(&app, &token_b, &format!("/api/dm/{dm}/read"), &read_body).await;
    assert_eq!(status, 200);
    let got = settle(&mock, 2).await;
    assert_eq!(got.len(), 2, "{got:?}");
    let m = msg(&got[1]);
    assert_eq!(m["token"], fcm_b.as_str());
    assert_eq!(m["data"]["t"], "read");
    assert_eq!(m["data"]["channel_id"], dm.to_string());
    assert_eq!(m["android"]["priority"], "NORMAL");

    // Marking read again sends nothing.
    let (status, _) = post_json(&app, &token_b, &format!("/api/dm/{dm}/read"), &read_body).await;
    assert_eq!(status, 200);
    tokio::time::sleep(QUIET).await;
    assert_eq!(received(&mock).len(), 2);
}

#[sqlx::test]
async fn test_read_without_prior_wake_sends_nothing(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let (a, _, _, _) = phone_user(&app).await;
    let (b, _, token_b, _) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;
    // Stored directly: no send path, so no wake.
    let message_id = insert_message(&app.pool, dm, a, "old").await;

    let (status, _) = post_json(
        &app,
        &token_b,
        &format!("/api/dm/{dm}/read"),
        &json!({ "last_read_message_id": message_id }),
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = post_json(&app, &token_b, "/api/me/read-all", &json!({})).await;
    assert_eq!(status, 204);

    tokio::time::sleep(QUIET).await;
    assert!(received(&mock).is_empty(), "{:?}", received(&mock));
}

#[sqlx::test]
async fn test_guild_channel_read_clears_mention(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let g = guild_fixture(&app).await;

    let message_id = send(&app, &g.token_a, g.channel, &format!("@{} ping", g.b_name)).await;
    assert_eq!(settle(&mock, 1).await.len(), 1);

    let (status, _) = post_json(
        &app,
        &g.token_b,
        &format!("/api/channels/{}/read", g.channel),
        &json!({ "last_read_message_id": message_id }),
    )
    .await;
    assert_eq!(status, 200);
    let got = settle(&mock, 2).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(msg(&got[1])["token"], g.fcm_b.as_str());
    assert_eq!(msg(&got[1])["data"]["t"], "read");
}

#[sqlx::test]
async fn test_read_all_endpoints_clear_woken_channels(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let g = guild_fixture(&app).await;
    let (channel, token_a, token_b) = (g.channel, g.token_a.clone(), g.token_b.clone());
    let dm = create_dm_channel(&app.pool, g.a, g.b).await;

    // Wake B in the guild channel and in the DM.
    send(&app, &token_a, channel, &format!("@{} ping", g.b_name)).await;
    send(&app, &token_a, dm, "hi").await;
    assert_eq!(settle(&mock, 2).await.len(), 2);

    // Server read-all clears the guild channel only.
    let (status, _) = post_json(
        &app,
        &token_b,
        &format!("/api/guilds/{}/read-all", g.guild_id),
        &json!({}),
    )
    .await;
    assert_eq!(status, 204);
    let got = settle(&mock, 3).await;
    assert_eq!(got.len(), 3, "{got:?}");
    assert_eq!(msg(&got[2])["data"]["t"], "read");
    assert_eq!(msg(&got[2])["data"]["channel_id"], channel.to_string());

    // DM read-all clears the DM.
    let (status, _) = post_json(&app, &token_b, "/api/dm/read-all", &json!({})).await;
    assert_eq!(status, 204);
    let got = settle(&mock, 4).await;
    assert_eq!(got.len(), 4, "{got:?}");
    assert_eq!(msg(&got[3])["data"]["channel_id"], dm.to_string());

    // Everything is cleared: a global read-all sends nothing more.
    let (status, _) = post_json(&app, &token_b, "/api/me/read-all", &json!({})).await;
    assert_eq!(status, 204);
    tokio::time::sleep(QUIET).await;
    assert_eq!(received(&mock).len(), 4);
}

#[sqlx::test]
async fn test_global_read_all_sends_read_after_wake(pool: PgPool) {
    let (app, mock) = fcm_app(pool).await;
    let (a, _, token_a, _) = phone_user(&app).await;
    let (b, _, token_b, fcm_b) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;

    send(&app, &token_a, dm, "hi").await;
    assert_eq!(settle(&mock, 1).await.len(), 1);

    let (status, _) = post_json(&app, &token_b, "/api/me/read-all", &json!({})).await;
    assert_eq!(status, 204);
    let got = settle(&mock, 2).await;
    assert_eq!(got.len(), 2, "{got:?}");
    let m = msg(&got[1]);
    assert_eq!(m["token"], fcm_b.as_str());
    assert_eq!(m["data"]["t"], "read");
    assert_eq!(m["data"]["channel_id"], dm.to_string());
    assert_eq!(m["android"]["priority"], "NORMAL");
}

#[sqlx::test]
async fn test_dm_file_message_pushes_recipient(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("SKIP: RustFS unavailable");
        return;
    }
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let (app, _bucket) = fresh_test_app_with_s3_and_config(pool, fcm_config(&mock)).await;
    let (a, _, token_a, _) = phone_user(&app).await;
    let (b, _, _, fcm_b) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, a, b).await;

    let boundary = "----PushBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"photo.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    let req = TestApp::request(Method::POST, &format!("/api/messages/channel/{dm}/upload"))
        .header("Authorization", format!("Bearer {token_a}"))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 201);

    let got = settle(&mock, 1).await;
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(msg(&got[0])["token"], fcm_b.as_str());
    assert_eq!(msg(&got[0])["data"]["t"], "dm");
    assert_eq!(msg(&got[0])["data"]["channel_id"], dm.to_string());
}
