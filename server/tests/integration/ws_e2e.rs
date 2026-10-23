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
