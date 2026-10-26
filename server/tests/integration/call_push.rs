//! Call wakes (contract C20 addendum "call wake"): `ringing` when a DM call
//! starts, `ended` once when it ends, end to end through the FCM mock.
//! Run: `cargo nextest run -p vc-server call_push`

use std::collections::HashSet;
use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::config::Config;
use vc_server::voice::call::EndReason;
use vc_server::voice::call_service::{end_call, CallService};

use crate::helpers::{
    body_to_json, create_dm_channel, create_test_user, generate_access_token, spawn_fcm_mock,
    test_service_account, FcmMock, FcmMockResponse, TestApp,
};

/// Upper bound for a push to arrive (generous: the full suite runs in parallel).
const ARRIVAL: Duration = Duration::from_secs(10);
/// How long a "no (further) push" assertion waits.
const QUIET: Duration = Duration::from_secs(1);

fn fcm_config(mock: &FcmMock) -> Config {
    let mut config = Config::default_for_test();
    config.fcm_service_account = Some(test_service_account(mock.token_uri()));
    config.fcm_api_base.clone_from(&mock.base_url);
    config
}

/// A user with one FCM device; returns (id, access token, fcm token).
async fn phone_user(app: &TestApp) -> (Uuid, String, String) {
    let (id, _) = create_test_user(&app.pool).await;
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
    (id, generate_access_token(&app.config, id), fcm_token)
}

struct Fx {
    app: TestApp,
    mock: FcmMock,
    caller: Uuid,
    caller_token: String,
    callee: Uuid,
    callee_token: String,
    fcm_callee: String,
    dm: Uuid,
}

async fn fixture_with(pool: PgPool, mut config_fn: impl FnMut(&mut Config)) -> Fx {
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let mut config = fcm_config(&mock);
    config_fn(&mut config);
    let app = TestApp::with_pool_and_config(pool, config).await;
    let (caller, caller_token, _) = phone_user(&app).await;
    let (callee, callee_token, fcm_callee) = phone_user(&app).await;
    let dm = create_dm_channel(&app.pool, caller, callee).await;
    Fx {
        app,
        mock,
        caller,
        caller_token,
        callee,
        callee_token,
        fcm_callee,
        dm,
    }
}

async fn fixture(pool: PgPool) -> Fx {
    fixture_with(pool, |_| {}).await
}

async fn call_post(fx: &Fx, token: &str, action: &str) -> (u16, Value) {
    let req = TestApp::request(Method::POST, &format!("/api/dm/{}/call/{action}", fx.dm))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = fx.app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

/// Start a call as the caller; returns its `call_id`.
async fn start(fx: &Fx) -> String {
    let (status, body) = call_post(fx, &fx.caller_token, "start").await;
    assert_eq!(status, 201, "start failed: {body}");
    body["call_id"].as_str().unwrap().to_string()
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

/// Assert one `ended` wake for `token` with `reason`.
fn assert_ended(push: &Value, fx: &Fx, call_id: &str, token: &str, reason: &str) {
    let m = msg(push);
    assert_eq!(m["token"], token);
    assert_eq!(
        m["data"],
        serde_json::json!({
            "t": "call",
            "channel_id": fx.dm.to_string(),
            "call_id": call_id,
            "caller_id": fx.caller.to_string(),
            "call_state": "ended",
            "reason": reason,
        })
    );
    assert_eq!(m["android"]["priority"], "HIGH");
    assert_eq!(m["android"]["ttl"], "3600s");
    assert_eq!(m["android"]["collapse_key"], format!("call-{}", fx.dm));
}

#[sqlx::test]
async fn test_ringing_reaches_callee_not_caller(pool: PgPool) {
    let fx = fixture(pool).await;
    let call_id = start(&fx).await;

    let got = settle(&fx.mock, 1).await;
    assert_eq!(got.len(), 1, "only the callee's device rings: {got:?}");
    let m = msg(&got[0]);
    assert_eq!(m["token"], fx.fcm_callee.as_str());
    assert_eq!(
        m["data"],
        serde_json::json!({
            "t": "call",
            "channel_id": fx.dm.to_string(),
            "call_id": call_id,
            "caller_id": fx.caller.to_string(),
            "call_state": "ringing",
        })
    );
    assert!(m.get("notification").is_none());
    assert_eq!(m["android"]["priority"], "HIGH");
    assert_eq!(m["android"]["ttl"], "45s");
    assert_eq!(m["android"]["collapse_key"], format!("call-{}", fx.dm));
}

#[sqlx::test]
async fn test_ended_after_decline(pool: PgPool) {
    let fx = fixture(pool).await;
    let call_id = start(&fx).await;
    assert_eq!(settle(&fx.mock, 1).await.len(), 1);

    let (status, _) = call_post(&fx, &fx.callee_token, "decline").await;
    assert_eq!(status, 200);

    let got = settle(&fx.mock, 2).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_ended(&got[1], &fx, &call_id, &fx.fcm_callee, "all_declined");
}

#[sqlx::test]
async fn test_ended_after_cancel(pool: PgPool) {
    let fx = fixture(pool).await;
    let call_id = start(&fx).await;
    assert_eq!(settle(&fx.mock, 1).await.len(), 1);

    let (status, _) = call_post(&fx, &fx.caller_token, "leave").await;
    assert_eq!(status, 200);

    let got = settle(&fx.mock, 2).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_ended(&got[1], &fx, &call_id, &fx.fcm_callee, "cancelled");
}

#[sqlx::test]
async fn test_ended_after_ring_timeout(pool: PgPool) {
    let fx = fixture_with(pool, |c| c.call_ring_timeout_secs = 1).await;
    let call_id = start(&fx).await;

    let got = settle(&fx.mock, 2).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(msg(&got[0])["data"]["call_state"], "ringing");
    assert_ended(&got[1], &fx, &call_id, &fx.fcm_callee, "no_answer");
}

/// `ended` goes only to users who got `ringing`: a participant blocked by the
/// caller (DB only) and one added after the start get nothing. Ending twice
/// sends `ended` once.
#[sqlx::test]
async fn test_ended_only_to_rung_users(pool: PgPool) {
    let fx = fixture(pool).await;
    let (blocked, _, _) = phone_user(&fx.app).await;
    let (late, _, _) = phone_user(&fx.app).await;
    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)")
        .bind(fx.dm)
        .bind(blocked)
        .execute(&fx.app.pool)
        .await
        .unwrap();
    // A DB-only block (no Redis cache entry), blocker = the participant.
    sqlx::query(
        "INSERT INTO friendships (requester_id, addressee_id, status) VALUES ($1, $2, 'blocked')",
    )
    .bind(blocked)
    .bind(fx.caller)
    .execute(&fx.app.pool)
    .await
    .unwrap();

    // The HTTP start refuses a blocked pair, so start through the service to
    // exercise the push recipient rules.
    let calls = CallService::for_state(&fx.app.state);
    let started = calls
        .start_call(
            fx.dm,
            fx.caller,
            "caller".into(),
            HashSet::from([fx.callee, blocked]),
            90,
            false,
        )
        .await
        .unwrap();
    let call_id = started.meta.call_id.clone();

    let got = settle(&fx.mock, 1).await;
    assert_eq!(got.len(), 1, "blocked participant must not ring: {got:?}");
    assert_eq!(msg(&got[0])["token"], fx.fcm_callee.as_str());

    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)")
        .bind(fx.dm)
        .bind(late)
        .execute(&fx.app.pool)
        .await
        .unwrap();
    end_call(&fx.app.state, fx.dm, EndReason::Cancelled)
        .await
        .unwrap();
    end_call(&fx.app.state, fx.dm, EndReason::Cancelled)
        .await
        .unwrap();

    let got = settle(&fx.mock, 2).await;
    assert_eq!(got.len(), 2, "ended once, to the rung callee only: {got:?}");
    assert_ended(&got[1], &fx, &call_id, &fx.fcm_callee, "cancelled");
}

/// Call wakes are not coalesced: ringing/ended/ringing/ended within seconds
/// all go out (a message wake would fold the second one into a trailing push).
#[sqlx::test]
async fn test_call_wakes_bypass_coalescing(pool: PgPool) {
    let fx = fixture(pool).await;
    let first = start(&fx).await;
    assert_eq!(settle(&fx.mock, 1).await.len(), 1);
    assert_eq!(call_post(&fx, &fx.caller_token, "leave").await.0, 200);
    assert_eq!(settle(&fx.mock, 2).await.len(), 2);
    let second = start(&fx).await;
    assert_ne!(first, second);

    let got = settle(&fx.mock, 3).await;
    assert_eq!(got.len(), 3, "{got:?}");
    let m = msg(&got[2]);
    assert_eq!(m["data"]["call_state"], "ringing");
    assert_eq!(m["data"]["call_id"], second.as_str());
}

/// The callee's own message wakes and call wakes share the per-user cap.
#[sqlx::test]
async fn test_call_wake_counts_toward_user_cap(pool: PgPool) {
    let fx = fixture(pool).await;
    // Exhaust the callee's per-minute budget through the service.
    for _ in 0..vc_server::push::coalesce::USER_MAX_PER_MINUTE {
        assert!(fx.app.state.push.admit_call(fx.callee));
    }
    start(&fx).await;
    assert!(
        settle(&fx.mock, 1).await.is_empty(),
        "over the cap no call wake goes out"
    );
}
