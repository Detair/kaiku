//! End-to-end WebSocket tests against a real server on a random port.

use std::time::Duration;

use futures::SinkExt;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::db;
use vc_server::ws::events::broadcast_to_user;
use vc_server::ws::ServerEvent;

use super::helpers::{
    create_dm_channel, create_test_user, generate_access_token, next_ws_event, spawn_test_server,
    ws_connect, TestApp,
};

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
    next_ws_event(&mut ws_b, "ready", Duration::from_secs(5)).await;
    let subscribe = serde_json::json!({ "type": "subscribe", "channel_id": dm }).to_string();
    ws_b.send(Message::Text(subscribe.into()))
        .await
        .expect("send subscribe");
    next_ws_event(&mut ws_b, "subscribed", Duration::from_secs(5)).await;

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

    let ev = next_ws_event(&mut ws_b, "message_new", Duration::from_secs(5)).await;
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
    next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;

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

    let ev = next_ws_event(&mut ws, "channel_read", Duration::from_secs(5)).await;
    assert_eq!(ev["channel_id"], channel_id.to_string());
}
