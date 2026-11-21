//! Screen share start is a request with an answer (contract C30), and every
//! member may share unless a channel override takes it away.

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use futures::SinkExt;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::migrations::{apply_from, apply_until};
use super::helpers::{
    add_guild_member, body_to_json, create_guild_with_default_role, create_test_user,
    create_voice_channel, expect_no_ws_event, generate_access_token, next_ws_event,
    spawn_test_server, ws_connect, TestApp, WsClient,
};

const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

/// Version of the migration that gives @everyone `SCREEN_SHARE`.
const EVERYONE_SCREEN_SHARE_MIGRATION: i64 = 20_261_124_000_000;

async fn send(ws: &mut WsClient, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send ws frame");
}

/// Connect, wait for `ready`, join the voice room and wait for its state.
async fn connect_and_join(
    server: &super::helpers::TestServer,
    token: &str,
    channel: Uuid,
) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    send(
        &mut ws,
        serde_json::json!({ "type": "voice_join", "channel_id": channel }),
    )
    .await;
    next_ws_event(&mut ws, "voice_room_state", EVENT_TIMEOUT).await;
    ws
}

fn share_start(channel: Uuid, stream_id: Uuid) -> serde_json::Value {
    serde_json::json!({
        "type": "voice_screen_share_start",
        "channel_id": channel,
        "stream_id": stream_id,
        "quality": "medium",
        "has_audio": false,
        "source_label": "Display 1",
    })
}

/// Create a server through the API (as a real owner would) and return it.
async fn create_guild_via_api(app: &TestApp, token: &str) -> Uuid {
    let response = app
        .oneshot(
            TestApp::request(Method::POST, "/api/guilds")
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "name": "Family" }).to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), 200);
    let body = body_to_json(response).await;
    body["id"].as_str().unwrap().parse().unwrap()
}

/// A member without any role starts a share and gets the acknowledgement
/// (`screen_share_started` with their own `stream_id`).
async fn assert_member_can_share(app: &TestApp, guild: Uuid, member: Uuid) {
    let server = spawn_test_server(app.router.clone()).await;
    let voice = create_voice_channel(&app.pool, guild, "voice").await;
    let token = generate_access_token(&app.config, member);
    let mut ws = connect_and_join(&server, &token, voice).await;

    let stream_id = Uuid::new_v4();
    send(&mut ws, share_start(voice, stream_id)).await;
    let ack = next_ws_event(&mut ws, "screen_share_started", EVENT_TIMEOUT).await;
    assert_eq!(ack["stream_id"], stream_id.to_string());
    assert_eq!(ack["user_id"], member.to_string());

    if let (Some(limiter), Some(room)) = (
        &app.state.screen_share_limiter,
        app.state.sfu.get_room(voice).await,
    ) {
        limiter.stop(&room.share_counter).await;
    }
}

#[sqlx::test]
async fn member_without_role_can_share_on_a_new_server(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let guild = create_guild_via_api(&app, &generate_access_token(&app.config, owner)).await;
    add_guild_member(&pool, guild, member).await;

    assert_member_can_share(&app, guild, member).await;
}

#[sqlx::test(migrations = false)]
async fn member_can_share_on_a_server_from_before_the_upgrade(pool: PgPool) {
    apply_until(&pool, EVERYONE_SCREEN_SHARE_MIGRATION).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    // The @everyone default before the upgrade: no screen share.
    let old_default = GuildPermissions::EVERYONE_DEFAULT.difference(GuildPermissions::SCREEN_SHARE);
    let guild = create_guild_with_default_role(&pool, owner, old_default).await;
    add_guild_member(&pool, guild, member).await;
    apply_from(&pool, EVERYONE_SCREEN_SHARE_MIGRATION).await;

    let bits: i64 = sqlx::query_scalar(
        "SELECT permissions FROM guild_roles WHERE guild_id = $1 AND is_default",
    )
    .bind(guild)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(GuildPermissions::from_db(bits).contains(GuildPermissions::SCREEN_SHARE));

    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    assert_member_can_share(&app, guild, member).await;
}

/// A channel override that denies @everyone `SCREEN_SHARE` refuses the start
/// with `SCREEN_SHARE_FORBIDDEN`; the others in the room see nothing.
#[sqlx::test]
async fn override_deny_refuses_share_without_telling_others(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (member, _) = create_test_user(&pool).await;
    let (viewer, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    add_guild_member(&pool, guild, member).await;
    add_guild_member(&pool, guild, viewer).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let everyone: Uuid =
        sqlx::query_scalar("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default")
            .bind(guild)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query(
        "INSERT INTO channel_overrides (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, 0, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(voice)
    .bind(everyone)
    .bind(GuildPermissions::SCREEN_SHARE.to_db())
    .execute(&pool)
    .await
    .unwrap();

    let mut viewer_ws =
        connect_and_join(&server, &generate_access_token(&app.config, viewer), voice).await;
    let mut ws =
        connect_and_join(&server, &generate_access_token(&app.config, member), voice).await;

    send(&mut ws, share_start(voice, Uuid::new_v4())).await;
    let err = next_ws_event(&mut ws, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "SCREEN_SHARE_FORBIDDEN", "{err}");
    expect_no_ws_event(
        &mut viewer_ws,
        "screen_share_started",
        Duration::from_secs(1),
    )
    .await;
    let room = app.state.sfu.get_room(voice).await.expect("room");
    assert!(room.get_screen_shares().await.is_empty());
}

/// The channel's share limit refuses the next start with
/// `SCREEN_SHARE_LIMIT`; the refused sharer gets no acknowledgement.
#[sqlx::test]
async fn channel_share_limit_answers_with_its_code(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    add_guild_member(&pool, guild, a).await;
    add_guild_member(&pool, guild, b).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    sqlx::query("UPDATE channels SET max_screen_shares = 1 WHERE id = $1")
        .bind(voice)
        .execute(&pool)
        .await
        .unwrap();

    let mut a_ws = connect_and_join(&server, &generate_access_token(&app.config, a), voice).await;
    let mut b_ws = connect_and_join(&server, &generate_access_token(&app.config, b), voice).await;
    send(&mut a_ws, share_start(voice, Uuid::new_v4())).await;
    next_ws_event(&mut a_ws, "screen_share_started", EVENT_TIMEOUT).await;
    next_ws_event(&mut b_ws, "screen_share_started", EVENT_TIMEOUT).await;

    send(&mut b_ws, share_start(voice, Uuid::new_v4())).await;
    let err = next_ws_event(&mut b_ws, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "SCREEN_SHARE_LIMIT", "{err}");
    expect_no_ws_event(
        &mut a_ws,
        "screen_share_started",
        Duration::from_millis(500),
    )
    .await;

    if let (Some(limiter), Some(room)) = (
        &app.state.screen_share_limiter,
        app.state.sfu.get_room(voice).await,
    ) {
        limiter.stop(&room.share_counter).await;
    }
}

/// Starts over the rate limit get an answer (`RATE_LIMITED`), not silence:
/// the client waits for one (C30).
#[sqlx::test]
async fn rate_limited_start_is_answered(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let mut ws = connect_and_join(&server, &generate_access_token(&app.config, owner), voice).await;

    // Burst of 5 allowed (3 accepted, 2 over the per-user limit), then limited.
    for _ in 0..6 {
        send(&mut ws, share_start(voice, Uuid::new_v4())).await;
    }
    let mut codes = Vec::new();
    for _ in 0..3 {
        let err = next_ws_event(&mut ws, "voice_error", EVENT_TIMEOUT).await;
        codes.push(err["code"].as_str().unwrap_or_default().to_owned());
    }
    assert_eq!(
        codes,
        ["SCREEN_SHARE_LIMIT", "SCREEN_SHARE_LIMIT", "RATE_LIMITED"],
        "answers to starts 4-6"
    );

    if let (Some(limiter), Some(room)) = (
        &app.state.screen_share_limiter,
        app.state.sfu.get_room(voice).await,
    ) {
        limiter.stop_n(&room.share_counter, 3).await;
    }
}

/// A stop is never lost to the rate limit: after a burst of start/stop
/// pairs at the limit no share is left registered and no slot is held.
#[sqlx::test]
async fn stops_are_never_rate_limited(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let mut ws = connect_and_join(&server, &generate_access_token(&app.config, owner), voice).await;

    for _ in 0..4 {
        let stream_id = Uuid::new_v4();
        send(&mut ws, share_start(voice, stream_id)).await;
        send(
            &mut ws,
            serde_json::json!({
                "type": "voice_screen_share_stop",
                "channel_id": voice,
                "stream_id": stream_id,
            }),
        )
        .await;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let room = app.state.sfu.get_room(voice).await.expect("room");
    assert!(
        room.get_screen_shares().await.is_empty(),
        "a share outlived its stop: {:?}",
        room.get_screen_shares().await
    );
}

/// An accepted start whose track never comes expires by itself: the others
/// get `screen_share_stopped` and the slot is free again.
#[sqlx::test]
async fn announcement_without_track_expires(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (viewer, _) = create_test_user(&pool).await;
    let guild =
        create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT).await;
    add_guild_member(&pool, guild, viewer).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    sqlx::query("UPDATE channels SET max_screen_shares = 1 WHERE id = $1")
        .bind(voice)
        .execute(&pool)
        .await
        .unwrap();
    let mut viewer_ws =
        connect_and_join(&server, &generate_access_token(&app.config, viewer), voice).await;
    let mut ws = connect_and_join(&server, &generate_access_token(&app.config, owner), voice).await;

    let stream_id = Uuid::new_v4();
    send(&mut ws, share_start(voice, stream_id)).await;
    next_ws_event(&mut ws, "screen_share_started", EVENT_TIMEOUT).await;
    let stopped = next_ws_event(
        &mut viewer_ws,
        "screen_share_stopped",
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(stopped["stream_id"], stream_id.to_string(), "{stopped}");

    // The slot is free: the viewer can share now.
    send(&mut viewer_ws, share_start(voice, Uuid::new_v4())).await;
    next_ws_event(&mut viewer_ws, "screen_share_started", EVENT_TIMEOUT).await;
    if let (Some(limiter), Some(room)) = (
        &app.state.screen_share_limiter,
        app.state.sfu.get_room(voice).await,
    ) {
        limiter.stop(&room.share_counter).await;
    }
}
