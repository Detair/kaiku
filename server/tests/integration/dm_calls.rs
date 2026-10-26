//! DM call lifecycle: ringing, ending and calling again.
//!
//! - `incoming_call` reaches the callee through their user channel, so a callee without the DM open
//!   still rings.
//! - A call ends (and its state is gone) on decline, ring timeout, when the last voice peer leaves
//!   the DM room, and when the caller's last socket drops while it rings. A new call right after
//!   that succeeds.
//! - A blocked pair cannot ring each other.

use std::time::Duration;

use futures::SinkExt;
use reqwest::StatusCode;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::voice::call::EndReason;
use vc_server::voice::call_service::{end_call, ring_timeout_fired, CallService};

use super::helpers::{
    create_dm_channel, create_test_user, expect_no_ws_event, generate_access_token, next_ws_event,
    spawn_test_server, ws_connect, TestApp, TestServer, WsClient,
};

/// Upper bound for awaited WebSocket events. Generous because the full suite
/// runs many servers in parallel; tests finish as soon as the event arrives.
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

struct Fixture {
    app: TestApp,
    server: TestServer,
    caller: Uuid,
    callee: Uuid,
    dm: Uuid,
    caller_token: String,
    callee_token: String,
}

async fn setup_with(pool: &PgPool, app: TestApp) -> Fixture {
    let server = spawn_test_server(app.router.clone()).await;
    let (caller, _) = create_test_user(pool).await;
    let (callee, _) = create_test_user(pool).await;
    let dm = create_dm_channel(pool, caller, callee).await;
    let caller_token = generate_access_token(&app.config, caller);
    let callee_token = generate_access_token(&app.config, callee);
    Fixture {
        app,
        server,
        caller,
        callee,
        dm,
        caller_token,
        callee_token,
    }
}

async fn setup(pool: &PgPool) -> Fixture {
    let app = TestApp::with_pool(pool.clone()).await;
    setup_with(pool, app).await
}

async fn connect_ready(server: &TestServer, token: &str) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    ws
}

async fn send_json(ws: &mut WsClient, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send WebSocket frame");
}

async fn call_post(fx: &Fixture, token: &str, action: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/api/dm/{}/call/{action}", fx.server.url, fx.dm))
        .bearer_auth(token)
        .send()
        .await
        .unwrap_or_else(|e| panic!("POST call/{action}: {e}"))
}

/// `GET /api/dm/{id}/call` as the caller; `null` when there is no call.
async fn call_state(fx: &Fixture) -> serde_json::Value {
    let resp = reqwest::Client::new()
        .get(format!("{}/api/dm/{}/call", fx.server.url, fx.dm))
        .bearer_auth(&fx.caller_token)
        .send()
        .await
        .expect("GET call");
    assert_eq!(resp.status(), StatusCode::OK, "GET call failed");
    resp.json().await.expect("call JSON")
}

async fn start(fx: &Fixture, token: &str) -> StatusCode {
    call_post(fx, token, "start").await.status()
}

#[sqlx::test]
async fn test_incoming_call_reaches_callee_without_channel_subscription(pool: PgPool) {
    let fx = setup(&pool).await;

    // The callee is online but has not subscribed to the DM channel.
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;
    // The caller has the DM open (subscribed) and must not ring itself.
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;
    send_json(
        &mut caller_ws,
        serde_json::json!({ "type": "subscribe", "channel_id": fx.dm }),
    )
    .await;
    next_ws_event(&mut caller_ws, "subscribed", EVENT_TIMEOUT).await;

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);

    let ev = next_ws_event(&mut callee_ws, "incoming_call", EVENT_TIMEOUT).await;
    assert_eq!(ev["channel_id"], fx.dm.to_string());
    assert_eq!(ev["initiator"], fx.caller.to_string());

    expect_no_ws_event(&mut caller_ws, "incoming_call", Duration::from_millis(500)).await;
}

#[sqlx::test]
async fn test_recall_after_decline_succeeds_immediately(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    assert_eq!(
        call_post(&fx, &fx.callee_token, "decline").await.status(),
        StatusCode::OK
    );
    let ended = next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "all_declined");
    assert!(
        call_state(&fx).await.is_null(),
        "declined call must be gone"
    );

    // Calling again right away works and leaves a clean ringing call.
    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    let state = call_state(&fx).await;
    assert_eq!(state["status"], "ringing", "new call state: {state}");

    // Declining and calling once more also works (no leftovers in between).
    assert_eq!(
        call_post(&fx, &fx.callee_token, "decline").await.status(),
        StatusCode::OK
    );
    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CREATED);
    assert_eq!(call_state(&fx).await["status"], "ringing");
}

#[sqlx::test]
async fn test_call_ends_when_last_participant_leaves(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    send_json(
        &mut caller_ws,
        serde_json::json!({ "type": "voice_join", "channel_id": fx.dm }),
    )
    .await;
    next_ws_event(&mut caller_ws, "voice_room_state", EVENT_TIMEOUT).await;

    assert_eq!(
        call_post(&fx, &fx.callee_token, "join").await.status(),
        StatusCode::OK
    );
    send_json(
        &mut callee_ws,
        serde_json::json!({ "type": "voice_join", "channel_id": fx.dm }),
    )
    .await;
    next_ws_event(&mut callee_ws, "voice_room_state", EVENT_TIMEOUT).await;
    assert_eq!(call_state(&fx).await["status"], "active");

    // Both leave the voice room without calling the REST leave endpoint.
    send_json(
        &mut caller_ws,
        serde_json::json!({ "type": "voice_leave", "channel_id": fx.dm }),
    )
    .await;
    next_ws_event(&mut callee_ws, "voice_user_left", EVENT_TIMEOUT).await;
    assert_eq!(
        call_state(&fx).await["status"],
        "active",
        "the call goes on while one peer is still in the room"
    );
    send_json(
        &mut callee_ws,
        serde_json::json!({ "type": "voice_leave", "channel_id": fx.dm }),
    )
    .await;

    let ended = next_ws_event(&mut callee_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "last_left");
    let ended = next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "last_left");
    assert!(call_state(&fx).await.is_null(), "ended call must be gone");

    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CREATED);
}

#[sqlx::test]
async fn test_abandoned_ringing_call_does_not_block_new_call(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    next_ws_event(&mut callee_ws, "incoming_call", EVENT_TIMEOUT).await;

    // The caller's only socket drops while the call rings.
    caller_ws.close(None).await.expect("close caller socket");
    drop(caller_ws);

    let ended = next_ws_event(&mut callee_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["channel_id"], fx.dm.to_string());
    assert_eq!(ended["reason"], "cancelled");
    assert!(
        call_state(&fx).await.is_null(),
        "abandoned call must be gone"
    );

    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CREATED);
}

/// A second socket of the caller closing does not cancel a call that the
/// caller is still around for.
#[sqlx::test]
async fn test_closing_one_of_two_caller_sockets_keeps_ringing(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;
    let _caller_ws = connect_ready(&fx.server, &fx.caller_token).await;
    let mut caller_ws2 = connect_ready(&fx.server, &fx.caller_token).await;

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    next_ws_event(&mut callee_ws, "incoming_call", EVENT_TIMEOUT).await;

    caller_ws2
        .close(None)
        .await
        .expect("close second caller socket");
    drop(caller_ws2);

    expect_no_ws_event(&mut callee_ws, "call_ended", Duration::from_secs(1)).await;
    assert_eq!(call_state(&fx).await["status"], "ringing");
}

/// Blocks are enforced from the database even when the block cache has no
/// entry (same rule as sending a DM).
#[sqlx::test]
async fn test_blocked_user_cannot_ring_or_join(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;

    // The callee starts a call first, then blocks the caller (DB only).
    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CREATED);
    sqlx::query(
        "INSERT INTO friendships (requester_id, addressee_id, status) VALUES ($1, $2, 'blocked')",
    )
    .bind(fx.callee)
    .bind(fx.caller)
    .execute(&pool)
    .await
    .expect("insert block");

    let resp = call_post(&fx, &fx.caller_token, "join").await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.expect("error JSON");
    assert_eq!(body["code"], "blocked");

    assert_eq!(
        call_post(&fx, &fx.callee_token, "leave").await.status(),
        StatusCode::OK
    );

    let resp = call_post(&fx, &fx.caller_token, "start").await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.expect("error JSON");
    assert_eq!(body["code"], "blocked");
    assert!(call_state(&fx).await.is_null());
    expect_no_ws_event(&mut callee_ws, "incoming_call", Duration::from_millis(500)).await;
}

/// A call nobody answers ends after the ring timeout (shortened here) with
/// `no_answer`, and calling again works.
#[sqlx::test]
async fn test_unanswered_call_ends_after_ring_timeout(pool: PgPool) {
    let mut config = Config::default_for_test();
    config.call_ring_timeout_secs = 1;
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let fx = setup_with(&pool, app).await;
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    next_ws_event(&mut callee_ws, "incoming_call", EVENT_TIMEOUT).await;

    let ended = next_ws_event(&mut callee_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "no_answer");
    let ended = next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "no_answer");
    assert!(call_state(&fx).await.is_null());

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
}

/// The ring timer of an earlier call on the same channel must not end a
/// newer call; the newer call's own timer does.
#[sqlx::test]
async fn test_stale_ring_timeout_does_not_end_newer_call(pool: PgPool) {
    let fx = setup(&pool).await;
    let calls = CallService::new(fx.app.state.redis.clone());
    let redis = fx.app.state.redis.clone();

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    let first = calls
        .current_call_id(fx.dm)
        .await
        .expect("read call")
        .expect("first call");
    assert_eq!(
        call_post(&fx, &fx.callee_token, "decline").await.status(),
        StatusCode::OK
    );
    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    let second = calls
        .current_call_id(fx.dm)
        .await
        .expect("read call")
        .expect("second call");
    assert_ne!(first, second, "every call has its own id");

    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;

    // The first call's timer fires late: the second call keeps ringing.
    assert!(!ring_timeout_fired(&redis, fx.dm, &first)
        .await
        .expect("stale timeout"));
    assert_eq!(call_state(&fx).await["status"], "ringing");
    expect_no_ws_event(&mut callee_ws, "call_ended", Duration::from_millis(300)).await;

    // The second call's own timer ends it, exactly once.
    assert!(ring_timeout_fired(&redis, fx.dm, &second)
        .await
        .expect("timeout"));
    let ended = next_ws_event(&mut callee_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "no_answer");
    assert!(!ring_timeout_fired(&redis, fx.dm, &second)
        .await
        .expect("repeated timeout"));
    assert!(call_state(&fx).await.is_null());
}

/// Concurrent enders (decline, ring timeout, explicit end) end a call once:
/// exactly one `call_ended`, and a new call starts cleanly afterwards.
#[sqlx::test]
async fn test_concurrent_end_paths_end_call_once(pool: PgPool) {
    let fx = setup(&pool).await;
    let calls = CallService::new(fx.app.state.redis.clone());
    let redis = fx.app.state.redis.clone();
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;

    for _ in 0..5 {
        assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
        let call_id = calls
            .current_call_id(fx.dm)
            .await
            .expect("read call")
            .expect("call");

        let (decline, timeout, end) = tokio::join!(
            call_post(&fx, &fx.callee_token, "decline"),
            ring_timeout_fired(&redis, fx.dm, &call_id),
            end_call(&fx.app.state, fx.dm, EndReason::Cancelled),
        );
        // A decline that lost the race finds no call (404).
        assert!(
            decline.status() == StatusCode::OK || decline.status() == StatusCode::NOT_FOUND,
            "decline: {}",
            decline.status()
        );
        timeout.expect("timeout path");
        end.expect("end path");

        next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;
        expect_no_ws_event(&mut caller_ws, "call_ended", Duration::from_millis(300)).await;
        assert!(call_state(&fx).await.is_null());
    }
}
