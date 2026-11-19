//! Screen share slots never outlive their share: stale counters are cleared
//! at startup, and a replaced voice peer gives its slots back.

use std::time::Duration;

use fred::interfaces::{ClientLike, KeysInterface};
use futures::SinkExt;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::db;
use vc_server::voice::ScreenShareLimiter;

use super::helpers::{
    add_guild_member, create_dm_channel, create_guild, create_guild_with_default_role,
    create_test_user, create_voice_channel, expect_no_ws_event, generate_access_token,
    next_ws_event, spawn_test_server, ws_connect, TestApp, WsClient,
};
use vc_server::permissions::GuildPermissions;

const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

/// `clear_all` (run at startup) removes every counter under the limiter's
/// prefix and nothing else.
#[tokio::test]
async fn screen_share_counters_cleared_at_startup() {
    let config = vc_server::config::Config::default_for_test();
    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("connect test Valkey");
    // A prefix of its own, so parallel tests keep their counters.
    let prefix = format!("screenshare:limit:test-{}:", Uuid::new_v4());
    let mut limiter = ScreenShareLimiter::with_key_prefix(redis.clone(), prefix.clone());
    limiter.init().await.expect("load script");

    // Slots left behind by a previous process.
    let stale: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    for channel in &stale {
        limiter.start(*channel, 6).await.expect("reserve slot");
    }
    let other = format!("screenshare:limit-other:{}", Uuid::new_v4());
    let _: () = redis
        .set(&other, 1, None, None, false)
        .await
        .expect("set other key");

    let cleared = limiter.clear_all().await.expect("clear counters");
    assert_eq!(cleared, stale.len());
    for channel in &stale {
        let exists: i64 = redis
            .exists(format!("{prefix}{channel}"))
            .await
            .expect("exists");
        assert_eq!(exists, 0, "stale counter for {channel} survived");
    }
    let other_exists: i64 = redis.exists(&other).await.expect("exists");
    assert_eq!(other_exists, 1, "a key outside the prefix was deleted");

    // A cleared channel can be shared again up to the limit.
    limiter.start(stale[0], 1).await.expect("slot free again");

    let _: i64 = redis.del(&other).await.expect("cleanup");
    limiter.clear_all().await.expect("cleanup");
    redis.quit().await.expect("quit");
}

async fn send(ws: &mut WsClient, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send ws frame");
}

async fn join_voice(ws: &mut WsClient, channel: Uuid) {
    send(
        ws,
        serde_json::json!({ "type": "voice_join", "channel_id": channel }),
    )
    .await;
}

/// A user who joins the same room again (second socket or reconnect)
/// replaces the old peer; the old peer's screen share is ended and its slot
/// released, so no ghost share remains.
#[sqlx::test]
async fn replacing_peer_frees_screen_share_slot(pool: PgPool) {
    let app = TestApp::with_pool_and_screen_share_limiter(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild(&pool, owner).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let token = generate_access_token(&app.config, owner);
    let counter = format!("screenshare:limit:{voice}");

    let mut first = ws_connect(&server, &token).await;
    next_ws_event(&mut first, "ready", EVENT_TIMEOUT).await;
    send(
        &mut first,
        serde_json::json!({ "type": "voice_join", "channel_id": voice }),
    )
    .await;
    next_ws_event(&mut first, "voice_room_state", EVENT_TIMEOUT).await;
    let stream_id = Uuid::new_v4();
    send(
        &mut first,
        serde_json::json!({
            "type": "voice_screen_share_start",
            "channel_id": voice,
            "stream_id": stream_id,
            "quality": "medium",
            "has_audio": false,
            "source_label": "Display 1",
        }),
    )
    .await;
    next_ws_event(&mut first, "screen_share_started", EVENT_TIMEOUT).await;
    let used: Option<i64> = redis.get(&counter).await.expect("get counter");
    assert_eq!(used, Some(1));

    // The same user joins again from a second socket.
    let mut second = ws_connect(&server, &token).await;
    next_ws_event(&mut second, "ready", EVENT_TIMEOUT).await;
    send(
        &mut second,
        serde_json::json!({ "type": "voice_join", "channel_id": voice }),
    )
    .await;
    let state = next_ws_event(&mut second, "voice_room_state", EVENT_TIMEOUT).await;
    assert_eq!(
        state["screen_shares"],
        serde_json::json!([]),
        "the replaced peer's share is still listed: {state}"
    );
    let used: Option<i64> = redis.get(&counter).await.expect("get counter");
    assert_eq!(
        used.unwrap_or(0),
        0,
        "slot of the replaced peer not released"
    );

    let room = app.state.sfu.get_room(voice).await.expect("room");
    assert!(room.get_screen_shares().await.is_empty());

    let _: i64 = redis.del(&counter).await.expect("cleanup");
    redis.quit().await.expect("quit");
}

/// A member who already holds a seat in a full room can rejoin (the new
/// peer replaces the stale one; the count stays), a new member cannot.
#[tokio::test]
async fn rejoin_into_a_full_room_replaces_the_seat() {
    use std::sync::Arc;

    use vc_server::voice::sfu::{Room, SfuServer};
    use vc_server::voice::VoiceError;

    let config = Arc::new(vc_server::config::Config::default_for_test());
    let sfu = SfuServer::new(config, None).expect("sfu");
    let channel = Uuid::new_v4();
    let room = Room::new(channel, 2);
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let peer = |user: Uuid| {
        let sfu = &sfu;
        let tx = tx.clone();
        async move {
            sfu.create_peer(user, "u".into(), "U".into(), channel, tx)
                .await
                .expect("peer")
        }
    };
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    room.add_peer(peer(a).await).await.expect("a joins");
    room.add_peer(peer(b).await).await.expect("b joins");

    let rejoin = peer(a).await;
    let session = rejoin.session_id;
    room.add_peer(rejoin)
        .await
        .expect("a rejoins the full room");
    assert_eq!(room.peers.read().await.len(), 2);
    assert_eq!(room.get_peer(a).await.expect("a").session_id, session);

    assert!(matches!(
        room.add_peer(peer(c).await).await,
        Err(VoiceError::ChannelFull { .. })
    ));
}

/// A second `voice_join` on the same socket replaces the peer without
/// telling that socket it was removed.
#[sqlx::test]
async fn same_socket_rejoin_is_not_told_it_was_replaced(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let guild = create_guild(&pool, owner).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    let token = generate_access_token(&app.config, owner);

    let mut ws = ws_connect(&server, &token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    for _ in 0..2 {
        send(
            &mut ws,
            serde_json::json!({ "type": "voice_join", "channel_id": voice }),
        )
        .await;
        next_ws_event(&mut ws, "voice_room_state", EVENT_TIMEOUT).await;
    }
    expect_no_ws_event(&mut ws, "voice_removed", Duration::from_millis(500)).await;
    let room = app.state.sfu.get_room(voice).await.expect("room");
    assert_eq!(room.participant_count().await, 1);
}

/// C8: a channel's `user_limit` refuses one more member with `CHANNEL_FULL`;
/// a member who is already in the room may still join again (replacement).
#[sqlx::test]
async fn user_limit_refuses_extra_member_but_not_a_rejoin(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (owner, _) = create_test_user(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let guild = create_guild_with_default_role(&pool, owner, GuildPermissions::EVERYONE_DEFAULT)
        .await;
    add_guild_member(&pool, guild, a).await;
    add_guild_member(&pool, guild, b).await;
    let voice = create_voice_channel(&pool, guild, "voice").await;
    sqlx::query("UPDATE channels SET user_limit = 2 WHERE id = $1")
        .bind(voice)
        .execute(&pool)
        .await
        .unwrap();

    let mut owner_ws = ws_connect(&server, &generate_access_token(&app.config, owner)).await;
    next_ws_event(&mut owner_ws, "ready", EVENT_TIMEOUT).await;
    join_voice(&mut owner_ws, voice).await;
    next_ws_event(&mut owner_ws, "voice_room_state", EVENT_TIMEOUT).await;
    let mut a_ws = ws_connect(&server, &generate_access_token(&app.config, a)).await;
    next_ws_event(&mut a_ws, "ready", EVENT_TIMEOUT).await;
    join_voice(&mut a_ws, voice).await;
    next_ws_event(&mut a_ws, "voice_room_state", EVENT_TIMEOUT).await;

    // Third member, the server owner neither: the room is full.
    let mut b_ws = ws_connect(&server, &generate_access_token(&app.config, b)).await;
    next_ws_event(&mut b_ws, "ready", EVENT_TIMEOUT).await;
    join_voice(&mut b_ws, voice).await;
    let err = next_ws_event(&mut b_ws, "voice_error", EVENT_TIMEOUT).await;
    assert_eq!(err["code"], "CHANNEL_FULL", "{err}");

    // A member already in the room joins again from another socket.
    let mut a2_ws = ws_connect(&server, &generate_access_token(&app.config, a)).await;
    next_ws_event(&mut a2_ws, "ready", EVENT_TIMEOUT).await;
    join_voice(&mut a2_ws, voice).await;
    next_ws_event(&mut a2_ws, "voice_room_state", EVENT_TIMEOUT).await;
    let room = app.state.sfu.get_room(voice).await.expect("room");
    assert_eq!(room.participant_count().await, 2);
}

/// C8: in a group DM, a participant who blocked (or was blocked by) another
/// participant cannot join the call; the refusal does not say who blocked
/// whom. A third participant without blocks joins.
#[sqlx::test]
async fn blocked_user_cannot_join_group_dm_call(pool: PgPool) {
    use axum::body::Body;
    use axum::http::Method;

    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (x, _) = create_test_user(&pool).await;
    let (y, _) = create_test_user(&pool).await;
    let (z, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, x, y).await;
    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)")
        .bind(dm)
        .bind(z)
        .execute(&pool)
        .await
        .unwrap();
    // X blocks Y.
    let response = app
        .oneshot(
            TestApp::request(Method::POST, &format!("/api/friends/{y}/block"))
                .header(
                    "Authorization",
                    format!("Bearer {}", generate_access_token(&app.config, x)),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(response.status().is_success(), "{}", response.status());

    for (user, refused) in [(y, true), (x, true), (z, false)] {
        let mut ws = ws_connect(&server, &generate_access_token(&app.config, user)).await;
        next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
        send(
            &mut ws,
            serde_json::json!({ "type": "voice_join", "channel_id": dm }),
        )
        .await;
        if refused {
            let err = next_ws_event(&mut ws, "voice_error", EVENT_TIMEOUT).await;
            assert_eq!(err["code"], "JOIN_REFUSED", "{err}");
            let text = err["message"].as_str().unwrap_or_default();
            assert!(!text.to_lowercase().contains("block"), "{text}");
        } else {
            next_ws_event(&mut ws, "voice_room_state", EVENT_TIMEOUT).await;
        }
    }
    let room = app.state.sfu.get_room(dm).await.expect("room");
    assert_eq!(room.participant_count().await, 1);
}
