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
    create_guild, create_test_user, create_voice_channel, generate_access_token, next_ws_event,
    spawn_test_server, ws_connect, TestApp, WsClient,
};

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
