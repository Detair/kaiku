//! DM call lifecycle: ringing, ending and calling again.
//!
//! - `incoming_call` reaches the callee through their user channel, so a callee without the DM open
//!   still rings.
//! - A call ends (and its state is gone) on decline, ring timeout, when the last voice peer leaves
//!   the DM room, and when the caller's last socket drops while it rings. A new call right after
//!   that succeeds.
//! - A blocked pair cannot ring each other.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fred::interfaces::{KeysInterface, SetsInterface, StreamsInterface};
use futures::SinkExt;
use reqwest::StatusCode;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::voice::call::EndReason;
use vc_server::voice::call_service::{
    caller_socket_closed, end_call, ring_timeout_fired, CallError, CallService,
};

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

    // Past the disconnect grace (3 s) plus slack.
    expect_no_ws_event(&mut callee_ws, "call_ended", Duration::from_millis(4500)).await;
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
    assert_eq!(body["code"], "BLOCKED");

    assert_eq!(
        call_post(&fx, &fx.callee_token, "leave").await.status(),
        StatusCode::OK
    );

    let resp = call_post(&fx, &fx.caller_token, "start").await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.expect("error JSON");
    assert_eq!(body["code"], "BLOCKED");
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
    let calls = CallService::new(fx.app.state.redis.clone(), pool.clone());

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
    assert!(!ring_timeout_fired(&calls, fx.dm, &first)
        .await
        .expect("stale timeout"));
    assert_eq!(call_state(&fx).await["status"], "ringing");
    expect_no_ws_event(&mut callee_ws, "call_ended", Duration::from_millis(300)).await;

    // The second call's own timer ends it, exactly once.
    assert!(ring_timeout_fired(&calls, fx.dm, &second)
        .await
        .expect("timeout"));
    let ended = next_ws_event(&mut callee_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["reason"], "no_answer");
    assert!(!ring_timeout_fired(&calls, fx.dm, &second)
        .await
        .expect("repeated timeout"));
    assert!(call_state(&fx).await.is_null());
}

/// Concurrent enders (decline, ring timeout, explicit end) end a call once:
/// exactly one `call_ended`, and a new call starts cleanly afterwards.
#[sqlx::test]
async fn test_concurrent_end_paths_end_call_once(pool: PgPool) {
    let fx = setup(&pool).await;
    let calls = CallService::new(fx.app.state.redis.clone(), pool.clone());
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
            ring_timeout_fired(&calls, fx.dm, &call_id),
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

fn calls(fx: &Fixture, pool: &PgPool) -> CallService {
    CallService::new(fx.app.state.redis.clone(), pool.clone())
}

/// REST responses and the call's WS events carry the same non-nil `call_id`;
/// the initiator's sessions get `call_started`.
#[sqlx::test]
async fn test_call_id_in_responses_matches_events(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;
    let mut callee_ws = connect_ready(&fx.server, &fx.callee_token).await;

    let resp = call_post(&fx, &fx.caller_token, "start").await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.expect("start JSON");
    let call_id = body["call_id"].as_str().expect("call_id").to_string();
    assert_ne!(call_id, Uuid::nil().to_string());
    assert_eq!(body["status"], "ringing");

    let started = next_ws_event(&mut caller_ws, "call_started", EVENT_TIMEOUT).await;
    assert_eq!(started["call_id"], call_id);
    assert_eq!(started["initiator"], fx.caller.to_string());
    let incoming = next_ws_event(&mut callee_ws, "incoming_call", EVENT_TIMEOUT).await;
    assert_eq!(incoming["call_id"], call_id);
    assert_eq!(call_state(&fx).await["call_id"], call_id);

    let resp = call_post(&fx, &fx.callee_token, "join").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("join JSON");
    assert_eq!(body["call_id"], call_id);
    assert_eq!(body["status"], "active");
    let joined = next_ws_event(&mut caller_ws, "call_participant_joined", EVENT_TIMEOUT).await;
    assert_eq!(joined["call_id"], call_id);

    let resp = call_post(&fx, &fx.caller_token, "leave").await;
    let body: serde_json::Value = resp.json().await.expect("leave JSON");
    assert_eq!(body["call_id"], call_id);
    let left = next_ws_event(&mut callee_ws, "call_participant_left", EVENT_TIMEOUT).await;
    assert_eq!(left["call_id"], call_id);
}

/// A decline whose call is replaced between its read and its write does not
/// touch the replacement call.
#[sqlx::test]
async fn test_decline_does_not_touch_replacement_call(pool: PgPool) {
    let fx = setup(&pool).await;
    let calls = calls(&fx, &pool);

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    let first = calls.current_call_id(fx.dm).await.unwrap().unwrap();

    let replaced = Arc::new(AtomicBool::new(false));
    let result = calls
        .decline_call_hooked(fx.dm, fx.callee, || {
            let calls = calls.clone();
            let state = fx.app.state.clone();
            let replaced = replaced.clone();
            let (dm, caller, callee) = (fx.dm, fx.caller, fx.callee);
            async move {
                if replaced.swap(true, Ordering::SeqCst) {
                    return;
                }
                // Barrier: call A ends and call B starts before the decline writes.
                end_call(&state, dm, EndReason::Cancelled)
                    .await
                    .expect("end A");
                calls
                    .start_call(
                        dm,
                        caller,
                        "caller".into(),
                        HashSet::from([callee]),
                        90,
                        false,
                    )
                    .await
                    .expect("start B");
            }
        })
        .await;
    assert!(
        matches!(result, Err(CallError::CallNotFound)),
        "decline must stop: {result:?}"
    );

    let state = call_state(&fx).await;
    assert_eq!(state["status"], "ringing", "call B: {state}");
    assert_ne!(state["call_id"], first);
    assert_eq!(state["declined_by"], serde_json::json!([]));
}

/// A voice peer from an earlier call leaving the room does not end a newer
/// active call.
#[sqlx::test]
async fn test_old_peer_leaving_does_not_end_newer_active_call(pool: PgPool) {
    let fx = setup(&pool).await;
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;

    // Call A: the caller joins the voice room, then cancels via REST only.
    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    send_json(
        &mut caller_ws,
        serde_json::json!({ "type": "voice_join", "channel_id": fx.dm }),
    )
    .await;
    next_ws_event(&mut caller_ws, "voice_room_state", EVENT_TIMEOUT).await;
    assert_eq!(
        call_post(&fx, &fx.caller_token, "leave").await.status(),
        StatusCode::OK
    );
    next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;

    // Call B, answered (active), while the old peer is still in the room.
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CREATED);
    assert_eq!(
        call_post(&fx, &fx.caller_token, "join").await.status(),
        StatusCode::OK
    );

    // The old peer leaves; the room is now empty.
    send_json(
        &mut caller_ws,
        serde_json::json!({ "type": "voice_leave", "channel_id": fx.dm }),
    )
    .await;
    expect_no_ws_event(&mut caller_ws, "call_ended", Duration::from_millis(500)).await;
    assert_eq!(call_state(&fx).await["status"], "active");
}

/// Disconnect cleanup cancels only the call generation it read from the
/// index, never a newer call of the same initiator.
#[sqlx::test]
async fn test_disconnect_cleanup_does_not_cancel_newer_call(pool: PgPool) {
    let fx = setup(&pool).await;
    let calls = calls(&fx, &pool);
    let redis = fx.app.state.redis.clone();
    let index = format!("call_ringing_by:{}", fx.caller);

    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    let first = calls.current_call_id(fx.dm).await.unwrap().unwrap();
    let first_member = format!("{}:{first}", fx.dm);
    let members: Vec<String> = redis.smembers(&index).await.unwrap();
    assert_eq!(members, vec![first_member.clone()]);

    assert_eq!(
        call_post(&fx, &fx.callee_token, "decline").await.status(),
        StatusCode::OK
    );
    let members: Vec<String> = redis.smembers(&index).await.unwrap();
    assert!(members.is_empty(), "an ended call leaves the index");
    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    let second = calls.current_call_id(fx.dm).await.unwrap().unwrap();

    // The cleanup sees only the old generation (as if it read the index
    // before call A ended).
    let second_member = format!("{}:{second}", fx.dm);
    let _: i64 = redis.srem(&index, &second_member).await.unwrap();
    let _: i64 = redis.sadd(&index, &first_member).await.unwrap();

    // The caller has no open socket.
    caller_socket_closed(&calls, fx.caller)
        .await
        .expect("cleanup");

    let state = call_state(&fx).await;
    assert_eq!(state["status"], "ringing");
    assert_eq!(state["call_id"], second);
    let members: Vec<String> = redis.smembers(&index).await.unwrap();
    assert!(
        !members.contains(&first_member),
        "a gone generation is dropped from the index"
    );
}

/// An active call left behind with no voice room (e.g. after a restart) does
/// not block a new call; a just-accepted call with an empty room does.
#[sqlx::test]
async fn test_orphaned_active_call_does_not_block_new_call(pool: PgPool) {
    let fx = setup(&pool).await;
    let redis = fx.app.state.redis.clone();
    let mut caller_ws = connect_ready(&fx.server, &fx.caller_token).await;

    // Fresh active call, nobody in voice yet: still protected.
    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);
    assert_eq!(
        call_post(&fx, &fx.callee_token, "join").await.status(),
        StatusCode::OK
    );
    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CONFLICT);
    end_call(&fx.app.state, fx.dm, EndReason::Cancelled)
        .await
        .unwrap();
    next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;

    // Restart-like orphan: an active call whose last event is 2 minutes old.
    let key = format!("call_events:{}", fx.dm);
    let orphan_id = Uuid::new_v4().to_string();
    let old_ms = chrono::Utc::now().timestamp_millis() - 120_000;
    let started = serde_json::json!({ "type": "started", "initiator": fx.caller }).to_string();
    let joined = serde_json::json!({ "type": "joined", "user_id": fx.callee }).to_string();
    let targets = serde_json::json!([fx.callee]).to_string();
    let _: String = redis
        .xadd(
            &key,
            false,
            None,
            format!("{old_ms}-0"),
            vec![
                ("call_id", orphan_id.as_str()),
                ("data", started.as_str()),
                ("targets", targets.as_str()),
                ("started_ms", old_ms.to_string().as_str()),
            ],
        )
        .await
        .unwrap();
    let _: String = redis
        .xadd(
            &key,
            false,
            None,
            format!("{old_ms}-1"),
            vec![("data", joined.as_str())],
        )
        .await
        .unwrap();
    let _: bool = redis.expire(&key, 600, None).await.unwrap();
    assert_eq!(call_state(&fx).await["status"], "active");

    assert_eq!(start(&fx, &fx.callee_token).await, StatusCode::CREATED);
    let ended = next_ws_event(&mut caller_ws, "call_ended", EVENT_TIMEOUT).await;
    assert_eq!(ended["call_id"], orphan_id);
    assert_eq!(ended["reason"], "last_left");
    let state = call_state(&fx).await;
    assert_eq!(state["status"], "ringing");
    assert_ne!(state["call_id"], orphan_id);
}

/// The initiator cannot answer their own ringing call.
#[sqlx::test]
async fn test_initiator_cannot_join_own_ringing_call(pool: PgPool) {
    let fx = setup(&pool).await;
    assert_eq!(start(&fx, &fx.caller_token).await, StatusCode::CREATED);

    let resp = call_post(&fx, &fx.caller_token, "join").await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    assert_eq!(call_state(&fx).await["status"], "ringing");
}
