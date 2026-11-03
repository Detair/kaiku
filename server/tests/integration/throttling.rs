//! Per-account throttling of passwords and codes (B.5).
//!
//! Run with: `cargo test --test integration throttling`

use axum::body::Body;
use axum::http::Method;
use fred::interfaces::{HashesInterface, KeysInterface};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::ratelimit::account::CounterKey;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, password_user, peer_request};
use super::helpers::{body_to_json_lenient, TestApp};
use super::login_attempts::{attempt_id, pc_call};
use super::login_password::login_native;

const PASSWORD: &str = "zq8#vT4p-throttle";

async fn login(app: &TestApp, username: &str, password: &str) -> (u16, Value, Option<String>) {
    let req = peer_request(Method::POST, "/auth/login")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"username": username, "password": password}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let retry = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (status, body_to_json_lenient(resp).await, retry)
}

async fn set_count(app: &TestApp, key: &CounterKey, n: i64) {
    let k = match key {
        CounterKey::Password(h) => format!("ratelimit:acct:password:{h}"),
        CounterKey::Totp(u) => format!("ratelimit:acct:totp:{u}"),
    };
    let _: () = app.state.redis.del(&k).await.unwrap();
    let _: () = app.state.redis.hset(&k, ("count", n)).await.unwrap();
}

async fn count_events(pool: &PgPool, user: Uuid, t: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = $2",
    )
    .bind(user)
    .bind(t)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn password_failures_delay_after_5(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, name) = password_user(&pool, PASSWORD).await;
    for _ in 0..5 {
        let (s, _, _) = login(&app, &name, "wrong-password-1").await;
        assert_eq!(s, 401);
    }
    let (s, b, retry) = login(&app, &name, PASSWORD).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{b}"
    );
    let retry: u64 = retry.expect("Retry-After").parse().unwrap();
    assert!((1..=30).contains(&retry), "{retry}");
}

#[sqlx::test]
async fn unknown_usernames_count_alike(pool: PgPool) {
    let app = TestApp::with_pool(pool).await;
    for _ in 0..5 {
        assert_eq!(login(&app, "nobody_here", "x-password").await.0, 401);
    }
    let (s, b, _) = login(&app, "Nobody_Here", "x-password").await;
    assert_eq!(
        (s, b["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{b}"
    );
}

#[sqlx::test]
async fn notice_at_20_is_coalesced(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let key = CounterKey::password(&name);
    set_count(&app, &key, 19).await;
    assert_eq!(login(&app, &name, "wrong-password-1").await.0, 401);
    assert_eq!(count_events(&pool, user, "guessing").await, 1);
    let details: Value = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'guessing'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(details, json!({"factor": "password", "count": 20}));
    // The 21st (after the delay) adds none.
    set_count(&app, &key, 20).await;
    assert_eq!(login(&app, &name, "wrong-password-1").await.0, 401);
    assert_eq!(count_events(&pool, user, "guessing").await, 1);
}

#[sqlx::test]
async fn throttled_at_100(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    set_count(&app, &CounterKey::password(&name), 99).await;
    assert_eq!(login(&app, &name, "wrong-password-1").await.0, 401);
    let state: String = sqlx::query_scalar("SELECT password_state FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "throttled");
}

#[sqlx::test]
async fn totp_100_after_correct_password_compromises_password(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (s, step, secret) = login_native(&app, &name, PASSWORD).await;
    assert_eq!(s, 202, "{step}");
    let id = attempt_id(&step["attempt"]);
    set_count(&app, &CounterKey::Totp(user), 99).await;
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": "000000"}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_MFA")), "{b}");
    let (pw, totp): (String, String) =
        sqlx::query_as("SELECT password_state, totp_state FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((pw.as_str(), totp.as_str()), ("compromised", "throttled"));
}

#[sqlx::test]
async fn strong_paths_unaffected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    for _ in 0..6 {
        login(&app, &name, "wrong-password-1").await;
    }
    assert_eq!(login(&app, &name, PASSWORD).await.0, 429);
    // The device signs in, and its session keeps working.
    let req = peer_request(Method::POST, "/auth/device-login/challenge")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"user_id": user, "device_id": dev.device_id}).to_string(),
        ))
        .unwrap();
    assert_eq!(app.oneshot(req).await.status().as_u16(), 201);
    let req = TestApp::request(Method::GET, "/auth/me")
        .header("Authorization", format!("Bearer {}", dev.access_token))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status().as_u16(), 200);
}
