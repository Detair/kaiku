//! After the user moved a DM call to another device, the old device's REST
//! leave or decline does not end the call the new device is in.

use std::time::Duration;

use futures::SinkExt;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::helpers::{
    create_dm_channel, create_test_user, generate_access_token, next_ws_event, spawn_test_server,
    ws_connect, TestApp, TestServer, WsClient,
};

const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

async fn send(ws: &mut WsClient, value: serde_json::Value) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .expect("send ws frame");
}

async fn connect_and_join(server: &TestServer, token: &str, channel: Uuid) -> WsClient {
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

struct DmCall {
    app: TestApp,
    server: TestServer,
    dm: Uuid,
    caller: Uuid,
    callee_old_token: String,
    callee_new_token: String,
    caller_token: String,
}

async fn post(server: &TestServer, dm: Uuid, token: &str, action: &str) -> serde_json::Value {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/dm/{dm}/call/{action}", server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("POST");
    assert!(resp.status().is_success(), "{action}: {}", resp.status());
    resp.json().await.unwrap_or(serde_json::Value::Null)
}

async fn call_state(server: &TestServer, dm: Uuid, token: &str) -> serde_json::Value {
    reqwest::Client::new()
        .get(format!("{}/api/dm/{dm}/call", server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET")
        .json()
        .await
        .expect("json")
}

/// Two sessions of the callee (two devices) with separate tokens.
async fn dm_call(pool: PgPool) -> DmCall {
    use super::helpers::create_session_with_token;

    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (caller, _) = create_test_user(&pool).await;
    let (callee, _) = create_test_user(&pool).await;
    let dm = create_dm_channel(&pool, caller, callee).await;
    let (_, callee_old_token) = create_session_with_token(&pool, &app.config, callee).await;
    let (_, callee_new_token) = create_session_with_token(&pool, &app.config, callee).await;
    let caller_token = generate_access_token(&app.config, caller);
    DmCall {
        app,
        server,
        dm,
        caller,
        callee_old_token,
        callee_new_token,
        caller_token,
    }
}

/// The callee answered on one device and moved to the other: the old
/// device's REST leave is a no-op, the call goes on with the callee in it.
#[sqlx::test]
async fn old_session_rest_leave_keeps_the_call(pool: PgPool) {
    let f = dm_call(pool).await;
    post(&f.server, f.dm, &f.caller_token, "start").await;
    let _caller_ws = connect_and_join(&f.server, &f.caller_token, f.dm).await;
    post(&f.server, f.dm, &f.callee_old_token, "join").await;
    let mut old = connect_and_join(&f.server, &f.callee_old_token, f.dm).await;
    let _new = connect_and_join(&f.server, &f.callee_new_token, f.dm).await;
    next_ws_event(&mut old, "voice_removed", EVENT_TIMEOUT).await;

    post(&f.server, f.dm, &f.callee_old_token, "leave").await;
    let state = call_state(&f.server, f.dm, &f.caller_token).await;
    assert_eq!(state["status"], "active", "{state}");
    let participants = state["participants"].as_array().cloned().unwrap_or_default();
    assert_eq!(participants.len(), 2, "callee dropped from the call: {state}");
    let _ = f.caller;
    let _ = &f.app;
}

/// The session that holds the seat leaves as before; so does a session when
/// the user holds no seat at all.
#[sqlx::test]
async fn owning_session_or_seatless_user_rest_leave_works(pool: PgPool) {
    let f = dm_call(pool).await;
    post(&f.server, f.dm, &f.caller_token, "start").await;
    post(&f.server, f.dm, &f.callee_old_token, "join").await;
    let _new = connect_and_join(&f.server, &f.callee_new_token, f.dm).await;

    // The caller holds no seat: their leave counts.
    post(&f.server, f.dm, &f.caller_token, "leave").await;
    let state = call_state(&f.server, f.dm, &f.callee_new_token).await;
    let participants = state["participants"].as_array().cloned().unwrap_or_default();
    assert!(
        !participants.iter().any(|p| p == &serde_json::json!(f.caller.to_string())),
        "seatless leave ignored: {state}"
    );

    // The callee's seat-holding session leaves: the call ends.
    post(&f.server, f.dm, &f.callee_new_token, "leave").await;
    assert!(call_state(&f.server, f.dm, &f.callee_new_token).await.is_null());
}
