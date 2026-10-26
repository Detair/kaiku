//! End-to-end WebSocket tests against a real server on a random port.

use std::time::Duration;

use fred::interfaces::{ClientLike, PubsubInterface};
use futures::{SinkExt, StreamExt};
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::db;
use vc_server::ws::events::broadcast_to_user;
use vc_server::ws::ServerEvent;

use super::helpers::{
    create_dm_channel, create_test_user, expect_no_ws_event, fresh_test_app_with_s3_and_pool,
    generate_access_token, next_ws_event, rustfs_available, spawn_test_server, ws_connect, TestApp,
    TestServer,
};

/// Upper bound for awaited WebSocket events. Generous because the full suite
/// runs many servers in parallel; tests finish as soon as the event arrives.
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

#[sqlx::test]
async fn ws_message_posted_after_ready_is_delivered(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;

    let (user_a, _) = create_test_user(&pool).await;
    let (user_b, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, user_a, user_b).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let mut ws_b = ws_connect(&server, &token_b).await;
    next_ws_event(&mut ws_b, "ready", EVENT_TIMEOUT).await;
    let subscribe = serde_json::json!({ "type": "subscribe", "channel_id": dm }).to_string();
    ws_b.send(Message::Text(subscribe.into()))
        .await
        .expect("send subscribe");
    next_ws_event(&mut ws_b, "subscribed", EVENT_TIMEOUT).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/messages/channel/{dm}", server.url))
        .bearer_auth(&token_a)
        .json(&serde_json::json!({ "content": "hello" }))
        .send()
        .await
        .expect("POST message");
    assert!(
        resp.status().is_success(),
        "POST message failed: {}",
        resp.status()
    );

    let ev = next_ws_event(&mut ws_b, "message_new", EVENT_TIMEOUT).await;
    assert_eq!(ev["message"]["content"], "hello");
}

/// `ready` promises the socket's subscriptions are live: an event published to
/// the user's own channel right after `ready` must reach the socket.
#[sqlx::test]
async fn ws_user_event_published_right_after_ready_is_delivered(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    let mut ws = ws_connect(&server, &token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;

    let channel_id = Uuid::now_v7();
    broadcast_to_user(
        &redis,
        user,
        &ServerEvent::ChannelRead {
            channel_id,
            last_read_message_id: None,
        },
    )
    .await
    .expect("publish user event");

    let ev = next_ws_event(&mut ws, "channel_read", EVENT_TIMEOUT).await;
    assert_eq!(ev["channel_id"], channel_id.to_string());

    redis.quit().await.expect("quit test Valkey client");
}

/// `ready` is the first frame: events published while the socket is being set
/// up (including right after its subscriptions go live) are delivered after
/// `ready`, not before it.
#[sqlx::test]
async fn ws_ready_is_first_frame_while_events_are_published(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    // Publish user events continuously while the socket connects.
    let publisher_redis = redis.clone();
    let publisher = tokio::spawn(async move {
        loop {
            let event = ServerEvent::ChannelRead {
                channel_id: Uuid::now_v7(),
                last_read_message_id: None,
            };
            broadcast_to_user(&publisher_redis, user, &event)
                .await
                .expect("publish user event");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });

    let mut ws = ws_connect(&server, &token).await;
    let first = tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    break serde_json::from_str::<serde_json::Value>(&text).expect("JSON frame");
                }
                Some(Ok(Message::Close(frame))) => panic!("socket closed: {frame:?}"),
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    })
    .await
    .expect("no frame within 5s");
    assert_eq!(first["type"], "ready", "first frame was {first}");

    // Events published around setup still arrive (after `ready`).
    next_ws_event(&mut ws, "channel_read", EVENT_TIMEOUT).await;

    publisher.abort();
    redis.quit().await.expect("quit test Valkey client");
}

#[sqlx::test]
async fn ws_ready_carries_server_time(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);

    let mut ws = ws_connect(&server, &token).await;
    let ready = next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    let t = chrono::DateTime::parse_from_rfc3339(ready["server_time"].as_str().unwrap()).unwrap();
    assert!(
        (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
            .num_seconds()
            .abs()
            < 5
    );
}

/// POST /api/dm as `token` with the given participants; returns (status, body).
async fn post_dm(
    server: &TestServer,
    token: &str,
    participant_ids: &[Uuid],
) -> (reqwest::StatusCode, serde_json::Value) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/dm", server.url))
        .bearer_auth(token)
        .json(&serde_json::json!({ "participant_ids": participant_ids }))
        .send()
        .await
        .expect("POST /api/dm");
    let status = resp.status();
    let body = resp.json().await.expect("POST /api/dm JSON body");
    (status, body)
}

/// The first DM reaches the recipient live: `dm_created` carries the same
/// object as the recipient's `GET /api/dm` entry, and the recipient's socket
/// is subscribed to the new channel without sending `subscribe`.
#[sqlx::test]
async fn first_dm_is_pushed_and_auto_subscribed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;

    let (user_a, _) = create_test_user(&pool).await;
    let (user_b, _) = create_test_user(&pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let mut ws_a = ws_connect(&server, &token_a).await;
    next_ws_event(&mut ws_a, "ready", EVENT_TIMEOUT).await;
    let mut ws_b = ws_connect(&server, &token_b).await;
    next_ws_event(&mut ws_b, "ready", EVENT_TIMEOUT).await;

    let (status, dm) = post_dm(&server, &token_a, &[user_b]).await;
    assert_eq!(status, 201, "POST /api/dm failed: {dm}");
    let dm_id: Uuid = dm["id"].as_str().unwrap().parse().unwrap();

    let ev = next_ws_event(&mut ws_b, "dm_created", EVENT_TIMEOUT).await;
    assert_eq!(ev["channel"]["id"], dm_id.to_string());
    assert_eq!(ev["channel"]["participants"].as_array().unwrap().len(), 2);

    // The event's channel is exactly B's GET /api/dm element for this DM.
    let list: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/api/dm", server.url))
        .bearer_auth(&token_b)
        .send()
        .await
        .expect("GET /api/dm")
        .json()
        .await
        .expect("GET /api/dm JSON");
    let entry = list
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == dm_id.to_string())
        .expect("new DM in recipient's list");
    assert_eq!(&ev["channel"], entry);

    // The creator's own sockets (other devices) are told as well.
    let ev_a = next_ws_event(&mut ws_a, "dm_created", EVENT_TIMEOUT).await;
    assert_eq!(ev_a["channel"]["id"], dm_id.to_string());

    // A posts a message; B receives it without ever sending "subscribe".
    let resp = reqwest::Client::new()
        .post(format!("{}/api/messages/channel/{dm_id}", server.url))
        .bearer_auth(&token_a)
        .json(&serde_json::json!({ "content": "first" }))
        .send()
        .await
        .expect("POST message");
    assert!(
        resp.status().is_success(),
        "POST message: {}",
        resp.status()
    );

    let msg = next_ws_event(&mut ws_b, "message_new", EVENT_TIMEOUT).await;
    assert_eq!(msg["channel_id"], dm_id.to_string());
    assert_eq!(msg["message"]["content"], "first");
}

/// Re-opening an existing 1:1 DM returns it again and announces nothing.
#[sqlx::test]
async fn existing_dm_does_not_emit_dm_created(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;

    let (user_a, _) = create_test_user(&pool).await;
    let (user_b, _) = create_test_user(&pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let (status, first) = post_dm(&server, &token_a, &[user_b]).await;
    assert_eq!(status, 201, "POST /api/dm failed: {first}");

    let mut ws_b = ws_connect(&server, &token_b).await;
    next_ws_event(&mut ws_b, "ready", EVENT_TIMEOUT).await;

    // The existing API answers 201 with the same channel for an existing DM.
    let (status, again) = post_dm(&server, &token_a, &[user_b]).await;
    assert_eq!(status, 201, "POST /api/dm failed: {again}");
    assert_eq!(again["id"], first["id"]);

    expect_no_ws_event(&mut ws_b, "dm_created", Duration::from_secs(1)).await;

    // Control: the same socket does receive the event for a genuinely new DM.
    let (user_c, _) = create_test_user(&pool).await;
    let (status, group) = post_dm(&server, &token_a, &[user_b, user_c]).await;
    assert_eq!(status, 201, "POST /api/dm failed: {group}");
    let ev = next_ws_event(&mut ws_b, "dm_created", EVENT_TIMEOUT).await;
    assert_eq!(ev["channel"]["id"], group["id"]);
}

/// A new group DM is announced to every participant.
#[sqlx::test]
async fn group_dm_is_pushed_to_every_participant(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;

    let (user_a, _) = create_test_user(&pool).await;
    let (user_b, _) = create_test_user(&pool).await;
    let (user_c, _) = create_test_user(&pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);
    let token_c = generate_access_token(&app.config, user_c);

    let mut ws_b = ws_connect(&server, &token_b).await;
    next_ws_event(&mut ws_b, "ready", EVENT_TIMEOUT).await;
    let mut ws_c = ws_connect(&server, &token_c).await;
    next_ws_event(&mut ws_c, "ready", EVENT_TIMEOUT).await;

    let (status, dm) = post_dm(&server, &token_a, &[user_b, user_c]).await;
    assert_eq!(status, 201, "POST /api/dm failed: {dm}");

    for ws in [&mut ws_b, &mut ws_c] {
        let ev = next_ws_event(ws, "dm_created", EVENT_TIMEOUT).await;
        assert_eq!(ev["channel"]["id"], dm["id"]);
        assert_eq!(ev["channel"]["participants"].as_array().unwrap().len(), 3);
    }
}

/// Number of Valkey connections subscribed to `channel` (`PUBSUB NUMSUB`).
async fn numsub(redis: &fred::clients::Client, channel: &str) -> i64 {
    let reply: Vec<fred::types::Value> = redis.pubsub_numsub(channel).await.expect("PUBSUB NUMSUB");
    reply
        .get(1)
        .and_then(fred::types::Value::as_i64)
        .expect("NUMSUB count")
}

/// Closing a socket must close its per-socket Valkey subscriber connection.
/// Counting subscribers of the test user's own `user:{id}` channel keeps this
/// independent of other tests running in parallel.
#[sqlx::test]
async fn ws_close_releases_valkey_subscriber(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let (user, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user);
    let user_channel = vc_server::ws::channels::user_events(user);

    let mut ws = ws_connect(&server, &token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    assert_eq!(numsub(&redis, &user_channel).await, 1, "subscriber live");

    ws.close(None).await.expect("close socket");
    drop(ws);

    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        let count = numsub(&redis, &user_channel).await;
        if count == 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "per-socket subscriber still connected after close ({count} subscriber(s) on {user_channel})"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    redis.quit().await.expect("quit test Valkey client");
}

/// A photo/file upload reaches the other participant live, exactly like a
/// text message: one `message_new` carrying the attachment.
#[sqlx::test]
async fn test_upload_message_broadcasts_message_new(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("SKIP: RustFS unavailable");
        return;
    }
    let (app, _bucket) = fresh_test_app_with_s3_and_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;

    let (user_a, _) = create_test_user(&pool).await;
    let (user_b, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, user_a, user_b).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);

    let mut ws_b = ws_connect(&server, &token_b).await;
    next_ws_event(&mut ws_b, "ready", Duration::from_secs(5)).await;
    let subscribe = serde_json::json!({ "type": "subscribe", "channel_id": dm }).to_string();
    ws_b.send(Message::Text(subscribe.into()))
        .await
        .expect("send subscribe");
    next_ws_event(&mut ws_b, "subscribed", Duration::from_secs(5)).await;

    let boundary = "----TestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\nphoto\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"note.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    let resp = reqwest::Client::new()
        .post(format!("{}/api/messages/channel/{dm}/upload", server.url))
        .bearer_auth(&token_a)
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await
        .expect("POST upload");
    assert_eq!(resp.status(), 201, "upload failed: {}", resp.status());

    let ev = next_ws_event(&mut ws_b, "message_new", Duration::from_secs(5)).await;
    assert_eq!(ev["channel_id"], dm.to_string());
    assert_eq!(ev["message"]["content"], "photo");
    let attachments = ev["message"]["attachments"]
        .as_array()
        .expect("attachments");
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["filename"], "note.txt");

    expect_no_ws_event(&mut ws_b, "message_new", Duration::from_secs(1)).await;
}

/// An `authenticate` frame with an invalid token is an auth problem: the socket
/// is closed with 1008 so clients refresh or sign in instead of retrying.
#[sqlx::test]
async fn ws_invalid_auth_frame_closes_with_policy_violation(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    let server = spawn_test_server(app.router.clone()).await;

    let mut ws = ws_connect(&server, "not-a-valid-token").await;
    let close = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    })
    .await
    .expect("no close frame after an invalid authenticate frame");

    let code: u16 = close.expect("close frame carries a code").code.into();
    assert_eq!(code, 1008);
}

// ============================================================================
// Presence across devices and for server members
// ============================================================================

/// How long a "nothing arrives" check listens. Long enough for a publish made
/// in the same atomic step as the Valkey change we waited for to be delivered.
const QUIET_WINDOW: Duration = Duration::from_secs(1);

/// Wait for a `presence_update` about `user` with `status`; other presence
/// updates (e.g. the connect snapshot) are skipped.
async fn next_presence(ws: &mut super::helpers::WsClient, user: Uuid, status: &str) {
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let ev = next_ws_event(ws, "presence_update", remaining).await;
        if ev["user_id"] == user.to_string() && ev["status"] == status {
            return;
        }
    }
}

/// Assert that no `presence_update` about `user` arrives within
/// [`QUIET_WINDOW`] (any status).
async fn expect_no_presence(ws: &mut super::helpers::WsClient, user: Uuid) {
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let ev: serde_json::Value = serde_json::from_str(&text).expect("JSON frame");
                    if ev["type"] == "presence_update" && ev["user_id"] == user.to_string() {
                        panic!("unexpected presence update: {ev}");
                    }
                }
                Some(Ok(Message::Close(frame))) => panic!("socket closed: {frame:?}"),
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    };
    let _ = tokio::time::timeout(QUIET_WINDOW, wait).await;
}

/// Wait until `user` has exactly `count` registered sockets in Valkey.
async fn wait_for_sockets(redis: &fred::clients::Client, user: Uuid, count: i64) {
    use fred::interfaces::SortedSetsInterface;

    let key = vc_server::presence::connections::sockets_key(user);
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        let n: i64 = redis.zcard(&key).await.expect("ZCARD");
        if n == count {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{user} has {n} socket(s), expected {count}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Status of `user` in `GET /api/guilds/{guild}/members`, as seen by `token`.
async fn member_status(server: &TestServer, token: &str, guild: Uuid, user: Uuid) -> String {
    let members: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/api/guilds/{guild}/members", server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET members")
        .json()
        .await
        .expect("members JSON");
    members
        .as_array()
        .expect("member array")
        .iter()
        .find(|m| m["user_id"] == user.to_string())
        .unwrap_or_else(|| panic!("{user} not in {members}"))["status"]
        .as_str()
        .expect("status string")
        .to_string()
}

/// Connect and wait for `ready`.
async fn connect_ready(server: &TestServer, token: &str) -> super::helpers::WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    ws
}

/// Users `a` and `b` (friends if `friends`), both in a server owned by `b`.
async fn presence_pair(pool: &PgPool, friends: bool) -> (Uuid, Uuid, Uuid) {
    let (a, _) = create_test_user(pool).await;
    let (b, _) = create_test_user(pool).await;
    if friends {
        super::helpers::create_friendship(pool, a, b).await;
    }
    let guild = super::helpers::create_guild(pool, b).await;
    super::helpers::add_guild_member(pool, guild, a).await;
    (a, b, guild)
}

#[sqlx::test]
async fn test_second_device_disconnect_keeps_user_online(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut phone = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;
    let _desktop = connect_ready(&server, &token_a).await;
    wait_for_sockets(&redis, a, 2).await;
    // The second device is not a new transition.
    expect_no_presence(&mut ws_b, a).await;

    phone.close(None).await.expect("close phone");
    drop(phone);
    wait_for_sockets(&redis, a, 1).await;

    expect_no_presence(&mut ws_b, a).await;
    assert_eq!(member_status(&server, &token_b, guild, a).await, "online");

    redis.quit().await.expect("quit test Valkey client");
}

#[sqlx::test]
async fn test_last_device_disconnect_broadcasts_offline(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut phone = connect_ready(&server, &token_a).await;
    let mut desktop = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;
    wait_for_sockets(&redis, a, 2).await;

    phone.close(None).await.expect("close phone");
    desktop.close(None).await.expect("close desktop");
    drop((phone, desktop));

    next_presence(&mut ws_b, a, "offline").await;
    wait_for_sockets(&redis, a, 0).await;
    // Exactly one offline broadcast.
    expect_no_presence(&mut ws_b, a).await;
    assert_eq!(member_status(&server, &token_b, guild, a).await, "offline");

    redis.quit().await.expect("quit test Valkey client");
}

#[sqlx::test]
async fn test_guild_comember_receives_presence_without_friendship(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, b, _guild) = presence_pair(&pool, false).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let _ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;

    // A socket of `b` opened later gets `a` in its presence snapshot.
    let mut ws_b2 = connect_ready(&server, &token_b).await;
    next_presence(&mut ws_b2, a, "online").await;
}

#[sqlx::test]
async fn test_manual_dnd_survives_reconnect(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;
    // The wire value clients send for "do not disturb".
    let busy = serde_json::json!({ "type": "set_status", "status": "busy" }).to_string();
    ws_a.send(Message::Text(busy.into()))
        .await
        .expect("set busy");
    next_presence(&mut ws_b, a, "busy").await;

    ws_a.close(None).await.expect("close");
    drop(ws_a);
    next_presence(&mut ws_b, a, "offline").await;

    let _ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "busy").await;
    wait_for_sockets(&redis, a, 1).await;
    expect_no_presence(&mut ws_b, a).await;
    assert_eq!(member_status(&server, &token_b, guild, a).await, "busy");

    redis.quit().await.expect("quit test Valkey client");
}

/// Concurrent connects and disconnects of one user's devices report each
/// transition exactly once, whatever the interleaving (the set update and the
/// transition check are one atomic Valkey step).
#[sqlx::test]
async fn test_concurrent_device_connects_and_disconnects_transition_once(pool: PgPool) {
    use vc_server::presence::connections::{socket_offline, socket_online};

    let app = TestApp::with_pool(pool).await;
    // Two connections, like two server instances: commands of the two
    // "devices" do not share one pipeline.
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let other = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect second test Valkey client");
    let now = chrono::Utc::now().timestamp_millis();

    for _ in 0..50 {
        let user = Uuid::new_v4();
        let (s1, s2) = (Uuid::new_v4(), Uuid::new_v4());
        // Randomize which connection is polled first in this round.
        let (first, second) = if Uuid::new_v4().as_u128() % 2 == 0 {
            (&redis, &other)
        } else {
            (&other, &redis)
        };

        let (on1, on2) = tokio::join!(
            socket_online(first, user, s1, now),
            socket_online(second, user, s2, now)
        );
        let ups = [on1.unwrap(), on2.unwrap()];
        assert_eq!(ups.iter().filter(|t| **t).count(), 1, "0→1 once: {ups:?}");

        let (off1, off2) = tokio::join!(
            socket_offline(second, user, s1, now),
            socket_offline(first, user, s2, now)
        );
        let downs = [off1.unwrap(), off2.unwrap()];
        assert_eq!(
            downs.iter().filter(|t| **t).count(),
            1,
            "1→0 once: {downs:?}"
        );

        // Hand-over: one device leaves while another arrives. Either both
        // transitions happen (offline, then online) or neither does.
        let s3 = Uuid::new_v4();
        assert!(socket_online(&redis, user, s1, now).await.unwrap());
        let (off, on) = tokio::join!(
            socket_offline(first, user, s1, now),
            socket_online(second, user, s3, now)
        );
        assert_eq!(off.unwrap(), on.unwrap(), "hand-over transitions pair up");
        assert!(socket_offline(&redis, user, s3, now).await.unwrap());
    }

    redis.quit().await.expect("quit test Valkey client");
    other.quit().await.expect("quit second test Valkey client");
}

/// A socket left behind by a crashed server stops counting once its expiry
/// has passed.
#[sqlx::test]
async fn test_stale_socket_of_crashed_server_is_not_live(pool: PgPool) {
    use vc_server::presence::connections::socket_online;

    let app = TestApp::with_pool(pool).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let user = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();

    // Registered 100 s ago and never removed (its expiry is 10 s in the past).
    assert!(socket_online(&redis, user, Uuid::new_v4(), now - 100_000)
        .await
        .unwrap());
    // A new device is the first live socket again.
    assert!(socket_online(&redis, user, Uuid::new_v4(), now)
        .await
        .unwrap());

    redis.quit().await.expect("quit test Valkey client");
}

/// A socket that disappears without a disconnect (crashed server) expires;
/// the sweeper then tells already-connected observers that the user is
/// offline.
#[sqlx::test]
async fn test_sweeper_announces_offline_for_expired_socket(pool: PgPool) {
    use vc_server::presence::connections::{socket_online_detailed, sweep_users};

    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, _guild) = presence_pair(&pool, true).await;
    let token_b = generate_access_token(&app.config, b);
    let mut ws_b = connect_ready(&server, &token_b).await;

    // A device of `a` on a server that then crashed: registered 30 s ago,
    // never refreshed, never removed (expires in 60 s).
    let now = chrono::Utc::now().timestamp_millis();
    assert!(
        socket_online_detailed(&redis, a, Uuid::new_v4(), now - 30_000, Some(("online", 0)))
            .await
            .unwrap()
            .first
    );
    next_presence(&mut ws_b, a, "online").await;

    // Not expired yet: nothing happens.
    assert!(!sweep_users(&pool, &redis, &[a], now)
        .await
        .unwrap()
        .contains(&a));
    expect_no_presence(&mut ws_b, a).await;

    // After the expiry the sweeper announces `offline` exactly once. (Tests
    // sweep only their own users: the due set is shared.)
    let later = now + 61_000;
    assert!(sweep_users(&pool, &redis, &[a], later)
        .await
        .unwrap()
        .contains(&a));
    next_presence(&mut ws_b, a, "offline").await;
    assert!(!sweep_users(&pool, &redis, &[a], later)
        .await
        .unwrap()
        .contains(&a));
    expect_no_presence(&mut ws_b, a).await;

    redis.quit().await.expect("quit test Valkey client");
}

/// A socket whose own entry was already pruned still reports the last
/// transition when it closes, as long as it is the call that empties the set.
#[sqlx::test]
async fn test_close_after_own_entry_was_pruned_announces_offline(pool: PgPool) {
    use fred::interfaces::SortedSetsInterface;
    use vc_server::presence::connections::{
        socket_offline_announce, socket_online, socket_online_detailed, sockets_key,
    };

    let app = TestApp::with_pool(pool).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let user = Uuid::new_v4();
    let (own, stale) = (Uuid::new_v4(), Uuid::new_v4());
    let now = chrono::Utc::now().timestamp_millis();

    // Shown online once; both entries expired; the own one is already gone
    // (pruned elsewhere).
    socket_online_detailed(&redis, user, own, now - 100_000, Some(("online", 0)))
        .await
        .unwrap();
    socket_online(&redis, user, stale, now - 100_000)
        .await
        .unwrap();
    let _: i64 = redis
        .zrem(sockets_key(user), own.to_string())
        .await
        .expect("ZREM");

    assert!(socket_offline_announce(&redis, user, own, now)
        .await
        .unwrap());
    let version = Some(published_count(&redis, user).await);
    assert_eq!(version, Some(2), "online, then offline once");
    // Nothing left to report.
    assert!(!socket_offline_announce(&redis, user, own, now)
        .await
        .unwrap());

    redis.quit().await.expect("quit test Valkey client");
}

/// Wait until nobody follows `user`'s presence channel any more.
async fn wait_for_no_presence_subscribers(redis: &fred::clients::Client, user: Uuid) {
    let channel = vc_server::ws::channels::user_presence(user);
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while numsub(redis, &channel).await > 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{channel} still has subscribers"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Two connected non-friend co-members; `change` ends their shared audience.
/// Afterwards neither sees the other's status changes.
async fn assert_audience_ends(pool: PgPool, change: &str) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, guild) = presence_pair(&pool, false).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;
    // `a`'s connect snapshot.
    next_presence(&mut ws_a, b, "online").await;
    assert_eq!(
        numsub(&redis, &vc_server::ws::channels::user_presence(a)).await,
        1
    );

    let http = reqwest::Client::new();
    let resp = match change {
        // `a` leaves the server owned by `b`.
        "leave" => http
            .post(format!("{}/api/guilds/{guild}/leave", server.url))
            .bearer_auth(&token_a),
        // `b` (owner) kicks `a`.
        "kick" => http
            .delete(format!("{}/api/guilds/{guild}/members/{a}", server.url))
            .bearer_auth(&token_b),
        // `b` blocks `a`.
        "block" => http
            .post(format!("{}/api/friends/{a}/block", server.url))
            .bearer_auth(&token_b),
        other => panic!("unknown change {other}"),
    }
    .send()
    .await
    .expect("audience change request");
    assert!(resp.status().is_success(), "{change}: {}", resp.status());

    // Both sockets dropped each other from their presence peers, each with a
    // final `offline` so no status stays frozen in the client.
    next_presence(&mut ws_b, a, "offline").await;
    next_presence(&mut ws_a, b, "offline").await;
    wait_for_no_presence_subscribers(&redis, a).await;
    wait_for_no_presence_subscribers(&redis, b).await;

    let busy = serde_json::json!({ "type": "set_status", "status": "busy" }).to_string();
    ws_a.send(Message::Text(busy.clone().into()))
        .await
        .expect("a busy");
    ws_b.send(Message::Text(busy.into())).await.expect("b busy");
    expect_no_presence(&mut ws_b, a).await;
    expect_no_presence(&mut ws_a, b).await;

    if change == "block" {
        // Still co-members, but each shows `offline` to the other.
        assert_eq!(member_status(&server, &token_b, guild, a).await, "offline");
        assert_eq!(member_status(&server, &token_a, guild, b).await, "offline");
    }

    redis.quit().await.expect("quit test Valkey client");
}

#[sqlx::test]
async fn test_leaving_server_ends_presence_sharing(pool: PgPool) {
    assert_audience_ends(pool, "leave").await;
}

#[sqlx::test]
async fn test_kick_ends_presence_sharing(pool: PgPool) {
    assert_audience_ends(pool, "kick").await;
}

#[sqlx::test]
async fn test_block_ends_presence_sharing_both_ways(pool: PgPool) {
    assert_audience_ends(pool, "block").await;
}

/// Joining a server adds its members to the presence audience of already
/// connected sockets on both sides.
#[sqlx::test]
async fn test_joining_server_starts_presence_sharing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let guild = super::helpers::create_guild(&pool, b).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut ws_a = connect_ready(&server, &token_a).await;

    let invite: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/api/guilds/{guild}/invites", server.url))
        .bearer_auth(&token_b)
        .json(&serde_json::json!({ "expires_in": "7d" }))
        .send()
        .await
        .expect("create invite")
        .json()
        .await
        .expect("invite JSON");
    let code = invite["code"]
        .as_str()
        .unwrap_or_else(|| panic!("{invite}"));
    let resp = reqwest::Client::new()
        .post(format!("{}/api/invites/{code}/join", server.url))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("join");
    assert!(resp.status().is_success(), "join: {}", resp.status());

    // The joiner gets the existing member in a snapshot.
    next_presence(&mut ws_a, b, "online").await;
    // The existing member follows the joiner's status from now on.
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let channel = vc_server::ws::channels::user_presence(a);
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    while numsub(&redis, &channel).await < 1 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{channel} never followed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let busy = serde_json::json!({ "type": "set_status", "status": "busy" }).to_string();
    ws_a.send(Message::Text(busy.into())).await.expect("a busy");
    next_presence(&mut ws_b, a, "busy").await;

    redis.quit().await.expect("quit test Valkey client");
}

/// Real expiry: the socket set's own key TTL runs out (as it does in
/// production when a server dies) before the sweeper looks. The sweeper must
/// still announce `offline` to an already-connected observer.
#[sqlx::test]
async fn test_sweeper_announces_offline_after_real_key_expiry(pool: PgPool) {
    use fred::interfaces::{KeysInterface, SortedSetsInterface};
    use vc_server::presence::connections::{socket_online_detailed, sockets_key, sweep_users};

    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, _guild) = presence_pair(&pool, true).await;
    let token_b = generate_access_token(&app.config, b);
    let mut ws_b = connect_ready(&server, &token_b).await;

    // A device whose expiry is 50 ms from now (registered ~90 s ago), on a
    // server that then died.
    let now = chrono::Utc::now().timestamp_millis();
    assert!(
        socket_online_detailed(&redis, a, Uuid::new_v4(), now - 89_950, Some(("online", 0)))
            .await
            .unwrap()
            .first
    );
    next_presence(&mut ws_b, a, "online").await;

    // Valkey drops the whole key once its TTL runs out.
    let _: bool = redis
        .pexpire(sockets_key(a), 20, None)
        .await
        .expect("PEXPIRE");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let exists: i64 = redis.exists(sockets_key(a)).await.expect("EXISTS");
    assert_eq!(exists, 0, "key expired");

    let swept = sweep_users(&pool, &redis, &[a], chrono::Utc::now().timestamp_millis())
        .await
        .unwrap();
    assert!(swept.contains(&a), "sweeper reports the transition");
    next_presence(&mut ws_b, a, "offline").await;
    let due: Option<f64> = redis
        .zscore(vc_server::presence::connections::DUE_KEY, a.to_string())
        .await
        .expect("ZSCORE");
    assert_eq!(due, None, "due entry cleared");
    expect_no_presence(&mut ws_b, a).await;

    redis.quit().await.expect("quit test Valkey client");
}

/// Presence version of `user` (number of status updates published).
async fn published_count(redis: &fred::clients::Client, user: Uuid) -> i64 {
    use fred::interfaces::HashesInterface;
    let version: Option<i64> = redis
        .hget(vc_server::presence::connections::state_key(user), "v")
        .await
        .expect("HGET version");
    version.unwrap_or(0)
}

/// A server change that happens after a socket read its audience but before
/// its subscriptions are live is not missed: the socket reloads its audience
/// once the subscriptions are live.
#[sqlx::test]
async fn test_audience_change_during_connect_is_not_missed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, b, guild) = presence_pair(&pool, false).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    // `b` connects and stops right after reading its audience (with `a`).
    let pause = vc_server::ws::test_hooks::pause_after_audience_read(b);
    let connecting = {
        let server_url = server.addr;
        let token = token_b.clone();
        tokio::spawn(async move {
            let url = format!("ws://{server_url}/ws");
            let (mut ws, _) = tokio_tungstenite::connect_async(url)
                .await
                .expect("connect");
            let auth = serde_json::json!({ "type": "authenticate", "token": token }).to_string();
            ws.send(Message::Text(auth.into())).await.expect("auth");
            next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
            ws
        })
    };
    tokio::time::timeout(EVENT_TIMEOUT, pause.reached.notified())
        .await
        .expect("socket reached the pause");

    // `a` leaves while `b` is not subscribed to anything yet.
    let resp = reqwest::Client::new()
        .post(format!("{}/api/guilds/{guild}/leave", server.url))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("leave");
    assert!(resp.status().is_success());
    pause.release.notify_one();
    let mut ws_b = connecting.await.expect("connect task");

    // The reload after the subscriptions went live drops `a`.
    wait_for_no_presence_subscribers(&redis, a).await;
    let _ws_a = connect_ready(&server, &token_a).await;
    expect_no_ws_event_about(&mut ws_b, a).await;

    redis.quit().await.expect("quit test Valkey client");
}

/// Assert that no `presence_update` about `user` arrives within
/// [`QUIET_WINDOW`], except an `offline` (a dropped peer's final update).
async fn expect_no_ws_event_about(ws: &mut super::helpers::WsClient, user: Uuid) {
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let ev: serde_json::Value = serde_json::from_str(&text).expect("JSON frame");
                    if ev["type"] == "presence_update"
                        && ev["user_id"] == user.to_string()
                        && ev["status"] != "offline"
                    {
                        panic!("unexpected presence update: {ev}");
                    }
                }
                Some(Ok(Message::Close(frame))) => panic!("socket closed: {frame:?}"),
                Some(Ok(_)) => {}
                other => panic!("socket ended: {other:?}"),
            }
        }
    };
    let _ = tokio::time::timeout(QUIET_WINDOW, wait).await;
}

/// Two devices change the status at once and their Valkey steps run in the
/// opposite order of the database: observers end up with the newest status,
/// each change is published at most once, and a late older change publishes
/// nothing.
#[sqlx::test]
async fn test_reordered_status_changes_follow_database_order(pool: PgPool) {
    use vc_server::presence::connections::{reconcile, socket_online, Reconciled};

    let app = TestApp::with_pool(pool).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let user = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();
    socket_online(&redis, user, Uuid::new_v4(), now)
        .await
        .unwrap();

    // Database order: busy (seq 1), then away (seq 2). Valkey sees away first.
    let away = reconcile(&redis, user, 2, "away", now).await.unwrap();
    assert_eq!(away, Reconciled::Applied { published: true });
    let busy = reconcile(&redis, user, 1, "busy", now).await.unwrap();
    assert_eq!(busy, Reconciled::Ahead { applied: 2 });
    // A retry of the newest change publishes nothing new.
    let again = reconcile(&redis, user, 2, "away", now).await.unwrap();
    assert_eq!(again, Reconciled::Applied { published: false });
    assert_eq!(published_count(&redis, user).await, 1);

    redis.quit().await.expect("quit test Valkey client");
}

/// A registration whose database read is older than the newest applied status
/// change publishes nothing (the caller reconciles from a fresh read).
#[sqlx::test]
async fn test_stale_registration_publishes_nothing(pool: PgPool) {
    use vc_server::presence::connections::{reconcile, socket_online_detailed};

    let app = TestApp::with_pool(pool).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let user = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp_millis();

    // Device A changed to busy at sequence 5 (and has since disconnected).
    reconcile(&redis, user, 5, "busy", now).await.unwrap();
    let before = published_count(&redis, user).await;
    // Device B registers with its older read (online at sequence 4).
    let up = socket_online_detailed(&redis, user, Uuid::new_v4(), now, Some(("online", 4)))
        .await
        .unwrap();
    assert!(up.first && up.stale);
    assert_eq!(
        published_count(&redis, user).await,
        before,
        "stale online not published"
    );
    // The fresh reconcile shows busy.
    let fresh = reconcile(&redis, user, 5, "busy", now).await.unwrap();
    assert_eq!(
        fresh,
        vc_server::presence::connections::Reconciled::Applied { published: true }
    );

    redis.quit().await.expect("quit test Valkey client");
}

/// Repairing an applied sequence that is ahead of the database is
/// compare-and-set on the exact value found: a newer change in between is
/// never undone.
#[sqlx::test]
async fn test_future_seq_repair_never_undoes_a_newer_change(pool: PgPool) {
    use fred::interfaces::KeysInterface;
    use vc_server::presence::connections::{replace_future_seq, seq_key};

    let app = TestApp::with_pool(pool).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let user = Uuid::new_v4();

    // The repair found 11 ahead of the database (10), but another device has
    // meanwhile applied 12.
    let _: () = redis
        .set(seq_key(user), "12", None, None, false)
        .await
        .expect("SET");
    assert!(!replace_future_seq(&redis, user, 11, 10).await.unwrap());
    let seq: Option<String> = redis.get(seq_key(user)).await.expect("GET");
    assert_eq!(seq.as_deref(), Some("12"));

    // Still the value found: replaced.
    assert!(replace_future_seq(&redis, user, 12, 10).await.unwrap());
    let seq: Option<String> = redis.get(seq_key(user)).await.expect("GET");
    assert_eq!(seq.as_deref(), Some("10"));

    redis.quit().await.expect("quit test Valkey client");
}

/// Message authors show what others see: a connected busy author is `busy`.
#[sqlx::test]
async fn test_message_author_status_is_shown_status(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, a, b).await;
    let token_a = generate_access_token(&app.config, a);

    let mut ws_a = connect_ready(&server, &token_a).await;
    wait_for_sockets(&redis, a, 1).await;
    let busy = serde_json::json!({ "type": "set_status", "status": "busy" }).to_string();
    ws_a.send(Message::Text(busy.into())).await.expect("busy");
    let ping = serde_json::json!({ "type": "ping" }).to_string();
    ws_a.send(Message::Text(ping.into())).await.expect("ping");
    next_ws_event(&mut ws_a, "pong", EVENT_TIMEOUT).await;

    let body: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/api/messages/channel/{dm}", server.url))
        .bearer_auth(&token_a)
        .json(&serde_json::json!({ "content": "hello" }))
        .send()
        .await
        .expect("POST message")
        .json()
        .await
        .expect("message JSON");
    assert_eq!(body["author"]["status"], "busy");

    redis.quit().await.expect("quit test Valkey client");
}

/// "Appear offline" is not available: `set_status` `offline` is refused with
/// `VALIDATION_ERROR` and changes nothing.
#[sqlx::test]
async fn test_set_status_offline_is_rejected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, b, guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;

    let hide = serde_json::json!({ "type": "set_status", "status": "offline" }).to_string();
    ws_a.send(Message::Text(hide.into()))
        .await
        .expect("set offline");
    let error = next_ws_event(&mut ws_a, "error", EVENT_TIMEOUT).await;
    assert_eq!(error["code"], "VALIDATION_ERROR", "{error}");

    expect_no_presence(&mut ws_b, a).await;
    assert_eq!(member_status(&server, &token_b, guild, a).await, "online");
}

/// The migration that removes "appear offline" makes every invisible user
/// visible again.
#[sqlx::test]
async fn test_clear_invisible_migration_makes_users_visible(pool: PgPool) {
    let (user, _) = create_test_user(&pool).await;
    sqlx::query("UPDATE users SET invisible = true WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .expect("mark invisible");

    let sql = include_str!("../../migrations/20261113000100_clear_invisible.sql");
    sqlx::raw_sql(sql).execute(&pool).await.expect("migration");

    let (invisible,): (bool,) = sqlx::query_as("SELECT invisible FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .expect("read flag");
    assert!(!invisible);
}

/// A block that happens after a connecting socket read its audience but
/// before its subscriptions are live never reaches the client: the audience
/// is revalidated before the presence snapshot.
#[sqlx::test]
async fn test_block_during_connect_is_not_in_snapshot(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (u, b, _guild) = presence_pair(&pool, false).await;
    let token_u = generate_access_token(&app.config, u);
    let token_b = generate_access_token(&app.config, b);
    let _ws_u = connect_ready(&server, &token_u).await;

    // `b` connects and stops right after reading its audience (with `u`).
    let pause = vc_server::ws::test_hooks::pause_after_audience_read(b);
    let connecting = {
        let addr = server.addr;
        let token = token_b.clone();
        tokio::spawn(async move {
            let url = format!("ws://{addr}/ws");
            let (mut ws, _) = tokio_tungstenite::connect_async(url)
                .await
                .expect("connect");
            let auth = serde_json::json!({ "type": "authenticate", "token": token }).to_string();
            ws.send(Message::Text(auth.into())).await.expect("auth");
            next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
            ws
        })
    };
    tokio::time::timeout(EVENT_TIMEOUT, pause.reached.notified())
        .await
        .expect("socket reached the pause");

    // `u` blocks `b` while `b` is not subscribed to anything yet.
    let resp = reqwest::Client::new()
        .post(format!("{}/api/friends/{b}/block", server.url))
        .bearer_auth(&token_u)
        .send()
        .await
        .expect("block");
    assert!(resp.status().is_success(), "block: {}", resp.status());
    pause.release.notify_one();
    let mut ws_b = connecting.await.expect("connect task");

    // Nothing about `u`: not in the snapshot, not afterwards.
    expect_no_presence(&mut ws_b, u).await;
}

/// Wait until the socket closes; returns its close code.
async fn close_code(ws: &mut super::helpers::WsClient) -> u16 {
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(Some(frame)))) => break u16::from(frame.code),
                Some(Ok(Message::Close(None))) | None => panic!("closed without a code"),
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("socket error: {e}"),
            }
        }
    };
    tokio::time::timeout(EVENT_TIMEOUT, wait)
        .await
        .expect("socket closed in time")
}

/// Users `owner` (server owner), `u` (member), `b` (outsider), all connected
/// except `owner`; returns the server and an invite code for it.
async fn join_scenario(
    pool: &PgPool,
    app: &TestApp,
    server: &TestServer,
) -> (Uuid, Uuid, Uuid, String) {
    let (owner, _) = create_test_user(pool).await;
    let (u, _) = create_test_user(pool).await;
    let (b, _) = create_test_user(pool).await;
    let guild = super::helpers::create_guild(pool, owner).await;
    super::helpers::add_guild_member(pool, guild, u).await;
    let token_owner = generate_access_token(&app.config, owner);
    let invite: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/api/guilds/{guild}/invites", server.url))
        .bearer_auth(&token_owner)
        .json(&serde_json::json!({ "expires_in": "7d" }))
        .send()
        .await
        .expect("create invite")
        .json()
        .await
        .expect("invite JSON");
    let code = invite["code"]
        .as_str()
        .unwrap_or_else(|| panic!("{invite}"))
        .to_string();
    (u, b, guild, code)
}

/// POST as `token`; asserts success.
async fn post_ok(url: String, token: &str) {
    let resp = reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .send()
        .await
        .expect("POST");
    assert!(resp.status().is_success(), "POST: {}", resp.status());
}

/// A member who leaves a server while an observer is joining it never reaches
/// the observer: peers subscribed by a reload are not forwarded until a read
/// after their subscription barrier confirms them, and that read drops the
/// member who left.
#[sqlx::test]
async fn test_new_peer_is_not_forwarded_before_validation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");
    let (u, b, guild, code) = join_scenario(&pool, &app, &server).await;
    let token_u = generate_access_token(&app.config, u);
    let token_b = generate_access_token(&app.config, b);
    let mut ws_u = connect_ready(&server, &token_u).await;
    let mut ws_b = connect_ready(&server, &token_b).await;

    // `b` joins; its reload stops after reading the members (with `u`).
    let read = vc_server::ws::test_hooks::pause_in_audience_reload(b);
    post_ok(format!("{}/api/invites/{code}/join", server.url), &token_b).await;
    tokio::time::timeout(EVENT_TIMEOUT, read.reached.notified())
        .await
        .expect("reload read");

    // `u` leaves before `b` follows the server channel.
    post_ok(format!("{}/api/guilds/{guild}/leave", server.url), &token_u).await;

    // `b` subscribes `u` (and the server) and stops after the barrier: `u`
    // is subscribed but not validated.
    let barrier = vc_server::ws::test_hooks::pause_after_audience_barrier(b);
    read.release.notify_one();
    tokio::time::timeout(EVENT_TIMEOUT, barrier.reached.notified())
        .await
        .expect("barrier");
    assert_eq!(
        numsub(&redis, &vc_server::ws::channels::user_presence(u)).await,
        1,
        "u subscribed (pending)"
    );

    // `u` publishes while pending: not forwarded.
    let busy = serde_json::json!({ "type": "set_status", "status": "busy" }).to_string();
    ws_u.send(Message::Text(busy.into())).await.expect("u busy");
    let ping = serde_json::json!({ "type": "ping" }).to_string();
    ws_u.send(Message::Text(ping.into())).await.expect("ping");
    next_ws_event(&mut ws_u, "pong", EVENT_TIMEOUT).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    barrier.release.notify_one();

    // The next read drops `u` again; `b` never heard of `u`.
    wait_for_no_presence_subscribers(&redis, u).await;
    expect_no_presence(&mut ws_b, u).await;

    redis.quit().await.expect("quit test Valkey client");
}

/// If a reload cannot complete after it already installed subscriptions (the
/// follow-up read fails), the socket closes with 1011 so the client
/// reconnects with a clean, full state.
#[sqlx::test]
async fn test_failed_reload_after_subscribing_closes_with_1011(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (_u, b, _guild, code) = join_scenario(&pool, &app, &server).await;
    let token_b = generate_access_token(&app.config, b);
    let mut ws_b = connect_ready(&server, &token_b).await;

    let read = vc_server::ws::test_hooks::pause_in_audience_reload(b);
    post_ok(format!("{}/api/invites/{code}/join", server.url), &token_b).await;
    tokio::time::timeout(EVENT_TIMEOUT, read.reached.notified())
        .await
        .expect("reload read");
    // The read after the new subscriptions fails.
    vc_server::ws::test_hooks::fail_next_audience_read(b);
    read.release.notify_one();

    assert_eq!(close_code(&mut ws_b).await, 1011);
}

/// The snapshot is the peer's **published** state with its version, never the
/// database: a status stored but not (yet) published is not shown to a new
/// observer.
#[sqlx::test]
async fn test_snapshot_shows_published_state_not_database(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, b, _guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let _ws_a = connect_ready(&server, &token_a).await;
    wait_for_sockets(&redis, a, 1).await;
    // A device stored "busy" but has not reconciled yet.
    sqlx::query("UPDATE users SET status = 'busy', presence_seq = presence_seq + 1 WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .expect("store busy");

    let mut ws_b = connect_ready(&server, &token_b).await;
    let snap = next_ws_event(&mut ws_b, "presence_update", EVENT_TIMEOUT).await;
    assert_eq!(snap["user_id"], a.to_string());
    assert_eq!(
        snap["status"], "online",
        "published state, not the database"
    );

    redis.quit().await.expect("quit test Valkey client");
}

/// A status change whose publish never happened (Valkey failed, the device
/// then closed) stays marked dirty in the database; the sweeper on any server
/// reconciles it, so observers get the new status anyway.
#[sqlx::test]
async fn test_dirty_status_change_is_reconciled_by_sweeper(pool: PgPool) {
    use vc_server::presence::connections::sweep_dirty;

    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, b, _guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let mut ws_b = connect_ready(&server, &token_b).await;
    let _phone = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;

    // The desktop stored "busy" (dirty), its reconcile failed, and it closed.
    sqlx::query(
        "UPDATE users SET status = 'busy', presence_seq = presence_seq + 1,
             presence_dirty_since = NOW() - INTERVAL '1 minute'
         WHERE id = $1",
    )
    .bind(a)
    .execute(&pool)
    .await
    .expect("store dirty busy");

    // (Another test's sweep may get there first: the observer must get busy
    // either way.)
    sweep_dirty(&pool, &redis, 10_000).await;
    next_presence(&mut ws_b, a, "busy").await;
    let (dirty,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT presence_dirty_since FROM users WHERE id = $1")
            .bind(a)
            .fetch_one(&pool)
            .await
            .expect("read mark");
    assert!(dirty.is_none(), "dirty mark cleared");

    redis.quit().await.expect("quit test Valkey client");
}

/// A normal `set_status` publishes at once and leaves no dirty mark.
#[sqlx::test]
async fn test_set_status_clears_dirty_mark(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, b, _guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);

    let mut ws_b = connect_ready(&server, &token_b).await;
    let mut ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;
    let away = serde_json::json!({ "type": "set_status", "status": "away" }).to_string();
    ws_a.send(Message::Text(away.into())).await.expect("away");
    next_presence(&mut ws_b, a, "away").await;

    // Cleared right after the publish.
    let deadline = tokio::time::Instant::now() + EVENT_TIMEOUT;
    loop {
        let (dirty,): (Option<chrono::DateTime<chrono::Utc>>,) =
            sqlx::query_as("SELECT presence_dirty_since FROM users WHERE id = $1")
                .bind(a)
                .fetch_one(&pool)
                .await
                .expect("read mark");
        if dirty.is_none() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "dirty mark not cleared"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Self-healing: an observer whose view went stale for any reason (here the
/// database changed without a publish and without a dirty mark) gets the
/// correct status from the next periodic reconcile.
#[sqlx::test]
async fn test_stale_observer_status_heals_within_one_reconcile(pool: PgPool) {
    use vc_server::presence::connections::reconcile_users;

    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (a, b, _guild) = presence_pair(&pool, true).await;
    let token_a = generate_access_token(&app.config, a);
    let token_b = generate_access_token(&app.config, b);
    let redis = db::create_redis_client(&app.config.redis_url)
        .await
        .expect("connect test Valkey");

    let mut ws_b = connect_ready(&server, &token_b).await;
    let _ws_a = connect_ready(&server, &token_a).await;
    next_presence(&mut ws_b, a, "online").await;

    // Stale: the database says busy, nothing was published, nothing is dirty.
    sqlx::query("UPDATE users SET status = 'busy', presence_seq = presence_seq + 1 WHERE id = $1")
        .bind(a)
        .execute(&pool)
        .await
        .expect("store busy");
    expect_no_presence(&mut ws_b, a).await;

    // One reconcile period (driven directly; only this test's user).
    assert_eq!(reconcile_users(&pool, &redis, &[a]).await, 1);
    next_presence(&mut ws_b, a, "busy").await;
    // Already correct: the next period publishes nothing.
    assert_eq!(reconcile_users(&pool, &redis, &[a]).await, 1);
    expect_no_presence(&mut ws_b, a).await;

    redis.quit().await.expect("quit test Valkey client");
}
