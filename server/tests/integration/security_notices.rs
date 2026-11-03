//! Security notices in the Kaiku DM, their pushes, the security log and the
//! minute sweep (C28, spec Part C, B.5 coalescing).
//!
//! Run with: `cargo test --test integration security_notices`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::client_class::ClientClass;
use vc_server::config::Config;
use vc_server::security_events::{notices, record, Actor, EventType, NewSecurityEvent};
use vc_server::system_account::SYSTEM_USER_ID;

use super::helpers::{
    body_to_json, create_elevated_session, create_session_with_token, create_test_user, make_admin,
    spawn_fcm_mock, spawn_test_server, test_service_account, ws_connect, FcmMock, FcmMockResponse,
    TestApp,
};

fn event(subject: Uuid, event_type: EventType, actor: Actor) -> NewSecurityEvent {
    let details = match event_type {
        EventType::AuthenticatorAdded => serde_json::json!({"type": "key_device"}),
        EventType::SignIn => serde_json::json!({"auth_methods": ["qr"], "assurance": "probation"}),
        _ => serde_json::json!({}),
    };
    NewSecurityEvent {
        subject_id: subject,
        event_type,
        actor,
        client: Some((ClientClass::Android, "Pixel 8".into())),
        ip: Some("203.0.113.44".parse().unwrap()),
        session_id: None,
        authenticator: None,
        request_id: None,
        details,
    }
}

async fn record_one(pool: &PgPool, ev: NewSecurityEvent) -> Uuid {
    let mut tx = pool.begin().await.unwrap();
    let id = record(&mut tx, ev).await.unwrap();
    tx.commit().await.unwrap();
    id
}

async fn record_and_deliver(app: &TestApp, ev: NewSecurityEvent) -> Uuid {
    let id = record_one(&app.pool, ev).await;
    notices::deliver(&app.state, &[id]).await.unwrap();
    id
}

/// Every notice message in `user`'s Kaiku DM, newest first, as the API shows it.
async fn kaiku_notices(app: &TestApp, user: Uuid, token: &str) -> Vec<Value> {
    let dm: Option<Uuid> = sqlx::query_scalar(
        "SELECT c.id FROM channels c
           JOIN dm_participants a ON a.channel_id = c.id AND a.user_id = $1
           JOIN dm_participants b ON b.channel_id = c.id AND b.user_id = $2
          WHERE c.read_only",
    )
    .bind(user)
    .bind(SYSTEM_USER_ID)
    .fetch_optional(&app.pool)
    .await
    .unwrap();
    let Some(dm) = dm else { return Vec::new() };
    let (status, page) = get(app, &format!("/api/messages/channel/{dm}?limit=100"), token).await;
    assert_eq!(status, 200, "{page}");
    page["items"].as_array().unwrap().clone()
}

async fn get(app: &TestApp, uri: &str, token: &str) -> (u16, Value) {
    let req = TestApp::request(Method::GET, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json(resp).await)
}

async fn user_with_token(app: &TestApp) -> (Uuid, String) {
    let (id, _) = create_test_user(&app.pool).await;
    let (_, token) = create_session_with_token(&app.pool, &app.config, id).await;
    (id, token)
}

#[sqlx::test]
async fn notice_creates_kaiku_dm_message(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, token) = user_with_token(&app).await;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    record_and_deliver(&app, event(user, EventType::SignIn, Actor::User(user))).await;

    let items = kaiku_notices(&app, user, &token).await;
    assert_eq!(items.len(), 1);
    let m = &items[0];
    assert_eq!(m["message_type"], "security");
    assert_eq!(m["author"]["id"], SYSTEM_USER_ID.to_string());
    assert_eq!(m["author"]["is_system"], true);
    assert!(
        !m["content"].as_str().unwrap().is_empty(),
        "English fallback"
    );
    let n = &m["security_notice"];
    assert_eq!(n["count"], 1);
    assert_eq!(n["event_type"], "sign_in");
    assert_eq!(n["severity"], "normal");
    assert_eq!(n["subject"]["user_id"], user.to_string());
    assert_eq!(n["subject"]["username"], username);
    assert_eq!(n["subject"]["display_name"], "HTTP Test User");
    assert_eq!(n["events"].as_array().unwrap().len(), 1);
    assert_eq!(n["events_truncated"], false);
    assert_eq!(n["events"][0]["event_type"], "sign_in");
    assert_eq!(n["events"][0]["actor"]["kind"], "self");
    assert_eq!(n["events"][0]["client"]["class"], "android");
    assert_eq!(n["events"][0]["actions"], serde_json::json!([]));
    assert!(n["notice_id"].as_str().is_some());
    assert!(n["first_at"].as_str().is_some() && n["last_at"].as_str().is_some());

    let (status, list) = get(&app, "/api/dm", &token).await;
    assert_eq!(status, 200);
    let kaiku = list
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["read_only"] == true)
        .expect("Kaiku DM listed");
    assert!(kaiku["participants"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["is_system"] == true));
}

#[sqlx::test]
async fn high_events_copied_to_other_admins(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (a, a_token) = user_with_token(&app).await;
    let (b, b_token) = user_with_token(&app).await;
    let (u, u_token) = user_with_token(&app).await;
    make_admin(&pool, a).await;
    make_admin(&pool, b).await;

    record_and_deliver(
        &app,
        event(u, EventType::AuthenticatorAdded, Actor::User(u)),
    )
    .await;
    for (who, token) in [(u, &u_token), (a, &a_token), (b, &b_token)] {
        let items = kaiku_notices(&app, who, token).await;
        assert_eq!(items.len(), 1, "one notice for {who}");
        assert_eq!(
            items[0]["security_notice"]["subject"]["user_id"],
            u.to_string()
        );
    }

    // A normal event is not copied.
    record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    assert_eq!(kaiku_notices(&app, a, &a_token).await.len(), 1);
    assert_eq!(kaiku_notices(&app, u, &u_token).await.len(), 2);

    // A high event on admin A: A's own notice and B's copy; never two for A.
    record_and_deliver(
        &app,
        event(a, EventType::AuthenticatorAdded, Actor::User(a)),
    )
    .await;
    let a_items = kaiku_notices(&app, a, &a_token).await;
    assert_eq!(a_items.len(), 2);
    assert_eq!(
        a_items
            .iter()
            .filter(|m| m["security_notice"]["subject"]["user_id"] == a.to_string())
            .count(),
        1
    );
    let b_items = kaiku_notices(&app, b, &b_token).await;
    assert_eq!(b_items.len(), 2);
    assert_eq!(kaiku_notices(&app, u, &u_token).await.len(), 2);
}

#[sqlx::test]
async fn notices_coalesce_within_10_minutes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (u, token) = user_with_token(&app).await;
    let first = record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    let mut ws = ws_connect(&server, &token).await;
    super::helpers::next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;

    let second = record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    let update =
        super::helpers::next_ws_event(&mut ws, "security_notice_updated", Duration::from_secs(5))
            .await;
    assert_eq!(update["security_notice"]["count"], 2);
    assert!(update["channel_id"].as_str().is_some());
    assert!(update["message_id"].as_str().is_some());
    let third = record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;

    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 1, "one message for three events");
    let n = &items[0]["security_notice"];
    assert_eq!(n["count"], 3);
    let ids: Vec<&str> = n["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        [third.to_string(), second.to_string(), first.to_string()],
        "newest first"
    );
    assert!(
        items[0]["edited_at"].as_str().is_some(),
        "an update edits the message"
    );
    // A different event type is a different notice.
    record_and_deliver(
        &app,
        event(u, EventType::RecoveryCodesCreated, Actor::User(u)),
    )
    .await;
    assert_eq!(kaiku_notices(&app, u, &token).await.len(), 2);

    // 11 minutes after the notice's first event: a new message.
    sqlx::query(
        "UPDATE security_notices SET first_at = first_at - INTERVAL '11 minutes'
          WHERE recipient_id = $1 AND event_type = 'sign_in'",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["security_notice"]["count"], 1);
}

#[sqlx::test]
async fn coalesced_events_truncate_at_50(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    let mut ids = Vec::new();
    for _ in 0..51 {
        ids.push(record_one(&pool, event(u, EventType::SignIn, Actor::User(u))).await);
    }
    notices::deliver(&app.state, &ids).await.unwrap();
    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 1);
    let n = &items[0]["security_notice"];
    assert_eq!(n["count"], 51);
    assert_eq!(n["events"].as_array().unwrap().len(), 50);
    assert_eq!(n["events_truncated"], true);
    assert_eq!(n["events"][0]["id"], ids[50].to_string());
}

fn fcm_config(mock: &FcmMock) -> Config {
    let mut config = Config::default_for_test();
    config.fcm_service_account = Some(test_service_account(mock.token_uri()));
    config.fcm_api_base.clone_from(&mock.base_url);
    config
}

async fn add_fcm_device(pool: &PgPool, user: Uuid) -> String {
    let token = format!("fcm-{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, provider, endpoint, device_id)
         VALUES ($1, 'fcm', $2, $3)",
    )
    .bind(user)
    .bind(&token)
    .bind(Uuid::now_v7())
    .execute(pool)
    .await
    .unwrap();
    token
}

fn pushes(mock: &FcmMock) -> Vec<Value> {
    mock.received.lock().unwrap().clone()
}

async fn settle(mock: &FcmMock, n: usize) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while pushes(mock).len() < n && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    pushes(mock)
}

#[sqlx::test]
async fn push_payload_is_c28(pool: PgPool) {
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let app = TestApp::with_pool_and_config(pool.clone(), fcm_config(&mock)).await;
    let (u, _) = user_with_token(&app).await;
    let device = add_fcm_device(&pool, u).await;

    let e1 = record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    let got = settle(&mock, 1).await;
    assert_eq!(got.len(), 1, "{got:?}");
    let msg = &got[0]["message"];
    assert_eq!(msg["token"], device);
    let data = &msg["data"];
    let notice_id: Uuid =
        sqlx::query_scalar("SELECT notice_id FROM security_notice_events WHERE event_id = $1")
            .bind(e1)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        data,
        &serde_json::json!({
            "t": "security",
            "notice_id": notice_id.to_string(),
            "event_type": "sign_in",
            "severity": "normal",
            "count": "1",
            "subject_id": u.to_string(),
        })
    );
    assert_eq!(msg["android"]["priority"], "HIGH");
    assert_eq!(
        msg["android"]["collapse_key"],
        format!("security-{notice_id}")
    );
    assert_eq!(msg["android"]["ttl"], "86400s");

    // A second update within 60 s pushes nothing.
    record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    assert_eq!(settle(&mock, 2).await.len(), 1);

    // An admin actor is named.
    let (admin, _) = create_test_user(&pool).await;
    record_and_deliver(
        &app,
        event(u, EventType::PasswordCompromised, Actor::Admin(admin)),
    )
    .await;
    let got = settle(&mock, 2).await;
    assert_eq!(got.len(), 2);
    let data = &got[1]["message"]["data"];
    assert_eq!(data["actor_id"], admin.to_string());
    assert_eq!(data["severity"], "high");
}

#[sqlx::test]
async fn push_bypasses_notification_settings(pool: PgPool) {
    let mock = spawn_fcm_mock(FcmMockResponse::Ok).await;
    let app = TestApp::with_pool_and_config(pool.clone(), fcm_config(&mock)).await;
    let (u, _) = user_with_token(&app).await;
    add_fcm_device(&pool, u).await;
    let (other, _) = create_test_user(&pool).await;
    let dm = super::helpers::create_dm_channel(&pool, u, other).await;

    // A DM wake fills the C20 coalescer for (u, dm); security pushes are
    // outside it and arrive at once, one per notice.
    app.state
        .push
        .notify(&app.state, u, vc_server::push::WakeSignal::dm(dm, 1));
    settle(&mock, 1).await;
    for t in [EventType::SignIn, EventType::KeyInvalidated] {
        record_and_deliver(&app, event(u, t, Actor::User(u))).await;
    }
    let got = settle(&mock, 3).await;
    let security: Vec<&Value> = got
        .iter()
        .filter(|p| p["message"]["data"]["t"] == "security")
        .collect();
    assert_eq!(security.len(), 2, "{got:?}");
    assert!(security
        .iter()
        .all(|p| p["message"]["android"]["priority"] == "HIGH"));
}

#[sqlx::test]
async fn undelivered_events_are_delivered_by_the_sweep(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    let id = record_one(&pool, event(u, EventType::SignIn, Actor::Cli)).await;
    assert!(kaiku_notices(&app, u, &token).await.is_empty());

    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(kaiku_notices(&app, u, &token).await.len(), 1);
    let delivered: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT delivered_at FROM security_events WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(delivered.is_some());

    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    notices::deliver(&app.state, &[id]).await.unwrap();
    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["security_notice"]["count"], 1, "no duplicate");
}

#[sqlx::test]
async fn security_log_is_newest_first_and_paginated(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    let (other, _) = create_test_user(&pool).await;
    let mut ids = Vec::new();
    for t in [
        EventType::SignIn,
        EventType::PasswordSet,
        EventType::RecoveryCodesCreated,
    ] {
        ids.push(record_one(&pool, event(u, t, Actor::User(u))).await);
    }
    record_one(&pool, event(other, EventType::SignIn, Actor::User(other))).await;

    let (status, page) = get(&app, "/api/me/security-events", &token).await;
    assert_eq!(status, 200, "{page}");
    let got: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        got,
        [ids[2].to_string(), ids[1].to_string(), ids[0].to_string()]
    );
    assert_eq!(page["has_more"], false);
    assert_eq!(
        page["items"][0]["ip"], "203.0.113.44",
        "own log shows the IP"
    );

    let (_, page) = get(&app, "/api/me/security-events?limit=2", &token).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert_eq!(page["has_more"], true);
    let cursor = page["next_cursor"].as_str().unwrap().to_string();
    assert_eq!(cursor, ids[1].to_string());
    let (_, page) = get(
        &app,
        &format!("/api/me/security-events?limit=2&before={cursor}"),
        &token,
    )
    .await;
    let rest: Vec<&str> = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap())
        .collect();
    assert_eq!(rest, [ids[0].to_string()]);
    assert_eq!(page["has_more"], false);

    for bad in ["limit=101", "limit=0", "limit=x", "before=not-a-uuid"] {
        let (status, body) = get(&app, &format!("/api/me/security-events?{bad}"), &token).await;
        assert_eq!(status, 400, "{bad}: {body}");
        assert_eq!(body["error"], "VALIDATION_ERROR", "{bad}");
    }
}

#[sqlx::test]
async fn copies_hide_ip_and_place(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, admin_token) = user_with_token(&app).await;
    make_admin(&pool, admin).await;
    let (u, u_token) = user_with_token(&app).await;
    record_and_deliver(
        &app,
        event(u, EventType::AuthenticatorAdded, Actor::User(u)),
    )
    .await;

    let own = kaiku_notices(&app, u, &u_token).await;
    assert_eq!(own[0]["security_notice"]["events"][0]["ip"], "203.0.113.44");
    let copy = kaiku_notices(&app, admin, &admin_token).await;
    let ev = &copy[0]["security_notice"]["events"][0];
    assert!(ev["ip"].is_null(), "{ev}");
    assert!(ev["place"].is_null());
    assert_eq!(ev["client"]["label"], "Pixel 8", "device label stays");

    let elevated = create_elevated_session(&pool, &app.config, admin).await;
    let (status, page) = get(
        &app,
        &format!("/api/admin/security-events?user_id={u}"),
        &elevated,
    )
    .await;
    assert_eq!(status, 200, "{page}");
    assert_eq!(
        page["items"][0]["ip"], "203.0.113.44",
        "admin log shows the IP"
    );
}

#[sqlx::test]
async fn admin_log_requires_elevation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, token) = user_with_token(&app).await;
    make_admin(&pool, admin).await;
    let (u, u_token) = user_with_token(&app).await;
    record_one(&pool, event(u, EventType::SignIn, Actor::User(u))).await;

    let (status, body) = get(&app, "/api/admin/security-events", &token).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"], "ELEVATION_REQUIRED");
    let (status, _) = get(&app, "/api/admin/security-events", &u_token).await;
    assert_eq!(status, 403);
    let elevated = create_elevated_session(&pool, &app.config, admin).await;
    let (status, page) = get(&app, "/api/admin/security-events", &elevated).await;
    assert_eq!(status, 200);
    assert!(page["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["subject_id"] == u.to_string()));
}

#[sqlx::test]
async fn there_is_no_delete_route(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    let id = record_one(&pool, event(u, EventType::SignIn, Actor::User(u))).await;
    for uri in [
        "/api/me/security-events".to_string(),
        format!("/api/me/security-events?before={id}"),
    ] {
        let req = TestApp::request(Method::DELETE, &uri)
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.oneshot(req).await.status(), 405, "{uri}");
    }
}

/// Review round 3 (Argon coverage, part 5 M1): concurrent deliveries of
/// events of one type for one recipient still coalesce into one notice:
/// the recipient's Kaiku DM advisory lock is taken before the notice
/// lookup, so the second delivery sees the first's notice.
#[sqlx::test]
async fn concurrent_deliveries_coalesce(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    for _ in 0..5 {
        let a = record_one(&pool, event(u, EventType::SignIn, Actor::User(u))).await;
        let b = record_one(&pool, event(u, EventType::SignIn, Actor::User(u))).await;
        let (ids_a, ids_b) = ([a], [b]);
        let (ra, rb) = tokio::join!(
            notices::deliver(&app.state, &ids_a),
            notices::deliver(&app.state, &ids_b)
        );
        ra.unwrap();
        rb.unwrap();
    }
    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 1, "one notice");
    assert_eq!(items[0]["security_notice"]["count"], 10);
}

/// Review round 3 (Argon coverage, part 5 M2): two admins' high events,
/// delivered at the same time, take the recipients' locks in one order.
/// Admin A's delivery is held after A's lock (A's Kaiku DM channel row is
/// locked); admin B's delivery then needs A's lock. Subject-first order made
/// A's delivery wait for B's lock in turn (40P01).
#[sqlx::test]
async fn admin_deliveries_never_deadlock(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (a, _) = create_test_user(&pool).await;
    let (b, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    make_admin(&pool, b).await;
    let (low, high) = if a < b { (a, b) } else { (b, a) };
    let mut tx = pool.begin().await.unwrap();
    let low_dm = vc_server::system_account::kaiku_dm_tx(&mut tx, low)
        .await
        .unwrap();
    vc_server::system_account::kaiku_dm_tx(&mut tx, high)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // High severity: the other admin gets a copy.
    let ev_low = record_one(
        &pool,
        event(low, EventType::AuthenticatorRemoved, Actor::User(low)),
    )
    .await;
    let ev_high = record_one(
        &pool,
        event(high, EventType::AuthenticatorRemoved, Actor::User(high)),
    )
    .await;

    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM channels WHERE id = $1 FOR UPDATE")
        .bind(low_dm)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (low_ids, high_ids) = ([ev_low], [ev_high]);
    let low_delivery = notices::deliver(&app.state, &low_ids);
    let high_delivery = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        notices::deliver(&app.state, &high_ids).await
    };
    let release = async {
        super::helpers::wait_for_lock_waiters(&pool, 2).await;
        blocker.commit().await.unwrap();
    };
    let (rl, rh, ()) = tokio::join!(low_delivery, high_delivery, release);
    assert!(rl.is_ok(), "{rl:?}");
    assert!(rh.is_ok(), "{rh:?}");
    let delivered: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE id = ANY($1) AND delivered_at IS NOT NULL",
    )
    .bind(vec![ev_low, ev_high])
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(delivered, 2);
}

/// Review round 3 (Argon coverage, part 5 M3): an event joins a notice only
/// within the window around the notice's first event, in both directions;
/// an event far older than the notice (delivered late) is a notice of its
/// own.
#[sqlx::test]
async fn coalescing_window_is_bounded_both_ways(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    // The notice's first event lies two hours after the next one.
    sqlx::query(
        "UPDATE security_notices SET first_at = first_at + INTERVAL '2 hours',
                                     last_at = last_at + INTERVAL '2 hours'
          WHERE recipient_id = $1",
    )
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 2, "two notices");
    for item in &items {
        assert_eq!(item["security_notice"]["count"], 1);
    }
}

/// Review round 3 (Argon coverage, part 5 M4): one event that cannot be
/// delivered does not hold back the events after it.
#[sqlx::test]
async fn one_failing_event_never_blocks_the_outbox(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    // A test-only failure: notice messages for this event type are refused.
    sqlx::raw_sql(
        "CREATE FUNCTION refuse_poison() RETURNS trigger AS $$
         BEGIN
             IF NEW.content LIKE '%poison test%' THEN
                 RAISE EXCEPTION 'poison';
             END IF;
             RETURN NEW;
         END $$ LANGUAGE plpgsql;
         CREATE TRIGGER refuse_poison BEFORE INSERT ON messages
             FOR EACH ROW EXECUTE FUNCTION refuse_poison();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let poison = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO security_events (id, subject_id, event_type, severity, actor_kind, created_at)
         VALUES ($1, $2, 'poison_test', 'normal', 'system', NOW() - INTERVAL '1 minute')",
    )
    .bind(poison)
    .bind(u)
    .execute(&pool)
    .await
    .unwrap();
    let good = record_one(&pool, event(u, EventType::SignIn, Actor::User(u))).await;

    let swept = notices::deliver_pending(&app.state).await;
    assert!(swept.is_err(), "the failure is still reported");
    let delivered: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT delivered_at FROM security_events WHERE id = $1")
            .bind(good)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(delivered.is_some(), "the later event is delivered");
    assert_eq!(kaiku_notices(&app, u, &token).await.len(), 1);
}

/// Review round 3 (Argon coverage, part 6 M1 / part 5 m1): the system
/// account has no Kaiku DM of its own. The DM lookup joined its participant
/// row twice, so for it any user's Kaiku DM matched; an event naming it is
/// delivered to no one.
#[sqlx::test]
async fn system_account_event_never_reaches_another_users_dm(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (u, token) = user_with_token(&app).await;
    record_and_deliver(&app, event(u, EventType::SignIn, Actor::User(u))).await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO security_events (id, subject_id, event_type, severity, actor_kind)
         VALUES ($1, $2, 'sign_in', 'normal', 'system')",
    )
    .bind(id)
    .bind(SYSTEM_USER_ID)
    .execute(&pool)
    .await
    .unwrap();
    notices::deliver(&app.state, &[id]).await.unwrap();
    let items = kaiku_notices(&app, u, &token).await;
    assert_eq!(items.len(), 1, "only the user's own notice");
    assert_eq!(items[0]["security_notice"]["count"], 1);
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        vc_server::system_account::kaiku_dm_tx(&mut conn, SYSTEM_USER_ID)
            .await
            .is_err(),
        "no Kaiku DM for the system account"
    );
}
