//! Device-key sign-in (spec B.4, SM4, C26).
//!
//! Run with: `cargo test --test integration device_login`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::secrets::b64u_decode;
use vc_server::approvals::signing::Decision;
use vc_server::auth::ctx::AuthCtx;
use vc_server::system_account::SYSTEM_USER_ID;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{peer_request, state_with_redis};
use super::helpers::{body_to_json, create_test_user, TestApp};

async fn public(router: &axum::Router, path: &str, body: Value) -> (u16, Value, Option<String>) {
    let req = peer_request(Method::POST, path)
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "android")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(router.clone(), req)
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let retry = resp
        .headers()
        .get("retry-after")
        .map(|v| v.to_str().unwrap().to_string());
    (status, body_to_json(resp).await, retry)
}

async fn challenge(app: &TestApp, dev: &KeyDeviceFixture) -> (u16, Value) {
    let (s, b, _) = public(
        &app.router,
        "/auth/device-login/challenge",
        json!({"user_id": dev.user_id, "device_id": dev.device_id}),
    )
    .await;
    (s, b)
}

async fn sign_in(app: &TestApp, dev: &KeyDeviceFixture, c: &Value) -> (u16, Value) {
    let prefix = b64u_decode(c["signing_prefix"].as_str().unwrap()).unwrap();
    let sig = dev.sign(&prefix, Decision::Approve, b"");
    let (s, b, _) = public(
        &app.router,
        "/auth/device-login",
        json!({"challenge_id": c["challenge_id"], "signature": sig}),
    )
    .await;
    (s, b)
}

async fn me(app: &TestApp, token: &str) -> (u16, Value) {
    let req = TestApp::request(Method::GET, "/auth/me")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await;
    let s = resp.status().as_u16();
    (s, body_to_json(resp).await)
}

#[sqlx::test]
async fn device_login_mints_a_full_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, c) = challenge(&app, &dev).await;
    assert_eq!(s, 201, "{c}");
    assert_eq!(c["kind"], "device_login");
    assert_eq!(c["context"], json!({"client_class": "android"}));
    let (s, auth) = sign_in(&app, &dev, &c).await;
    assert_eq!(s, 200, "{auth}");
    assert_eq!(auth["assurance"], "full");
    assert!(
        auth["refresh_token"].as_str().is_some(),
        "refresh in the body"
    );
    let absolute: chrono::DateTime<chrono::Utc> = auth["absolute_expires_at"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((absolute - chrono::Utc::now()).num_days() >= 89);
    let (s, me) = me(&app, auth["access_token"].as_str().unwrap()).await;
    assert_eq!(s, 200);
    assert_eq!(me["session"]["assurance"], "full");
    assert_eq!(me["session"]["auth_methods"], json!(["device_key"]));
    assert_eq!(me["session"]["client_class"], "android");
    assert_eq!(me["session"]["key_device_id"], dev.row_id.to_string());
    let ev: Value = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'sign_in'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        ev,
        json!({"auth_methods": ["device_key"], "assurance": "full"})
    );
    let used: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT last_used_at FROM approval_devices WHERE id = $1")
            .bind(dev.row_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(used.is_some());
}

#[sqlx::test]
async fn pending_device_gets_ready_at(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let (s, b) = challenge(&app, &dev).await;
    assert_eq!((s, b["error"].as_str()), (409, Some("KEY_DEVICE_PENDING")));
    assert!(b.get("ready_at").is_some(), "{b}");
}

#[sqlx::test]
async fn revoked_unknown_banned_and_system_are_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let revoked = KeyDeviceFixture::insert(&pool, &app.config, user, "revoked").await;
    let (s, b) = challenge(&app, &revoked).await;
    assert_eq!((s, b["error"].as_str()), (403, Some("KEY_DEVICE_REVOKED")));

    let (s, b, _) = public(
        &app.router,
        "/auth/device-login/challenge",
        json!({"user_id": user, "device_id": Uuid::new_v4()}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (404, Some("KEY_DEVICE_UNKNOWN")));
    let (s, b, _) = public(
        &app.router,
        "/auth/device-login/challenge",
        json!({"user_id": SYSTEM_USER_ID, "device_id": Uuid::new_v4()}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (404, Some("KEY_DEVICE_UNKNOWN")));

    let active = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, c) = challenge(&app, &active).await;
    sqlx::query("INSERT INTO global_bans (user_id, reason) VALUES ($1, 'x')")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = challenge(&app, &active).await;
    assert_eq!((s, b["error"].as_str()), (403, Some("ACCOUNT_BANNED")));
    let (s, b) = sign_in(&app, &active, &c).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("ACCOUNT_BANNED")),
        "re-checked at sign-in"
    );
}

#[sqlx::test]
async fn challenge_is_single_use(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, c) = challenge(&app, &dev).await;
    assert_eq!(sign_in(&app, &dev, &c).await.0, 200);
    let (s, b) = sign_in(&app, &dev, &c).await;
    assert_eq!((s, b["error"].as_str()), (409, Some("APPROVAL_STATE")));
    // Another device's signature over a fresh challenge is refused.
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, c) = challenge(&app, &dev).await;
    let (s, b) = sign_in(&app, &other, &c).await;
    assert_eq!((s, b["error"].as_str()), (400, Some("SIGNATURE_INVALID")));
}

#[sqlx::test]
async fn revoke_and_device_login_race(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    for _ in 0..25 {
        let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
        let (_, c) = challenge(&app, &dev).await;
        let revoke = async {
            let ctx = AuthCtx::from(&app.state);
            let mut tx = pool.begin().await.unwrap();
            // As every revoking caller: the account lock first (B.7).
            vc_server::approvals::transition::lock_accounts(&mut tx, &[user])
                .await
                .unwrap();
            vc_server::approvals::devices::revoke_tx(
                &mut tx,
                &ctx,
                dev.row_id,
                vc_server::approvals::devices::RevokeReason::Admin,
                vc_server::security_events::Actor::Cli,
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
        };
        let ((_, _), ()) = tokio::join!(sign_in(&app, &dev, &c), revoke);
        let live: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions WHERE authenticator_id = $1 AND ended_at IS NULL",
        )
        .bind(dev.row_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live, 0, "no live session of a revoked device");
    }
}

#[sqlx::test]
async fn a_new_device_login_supersedes_the_old_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, c) = challenge(&app, &dev).await;
    let (_, first) = sign_in(&app, &dev, &c).await;
    let (_, c) = challenge(&app, &dev).await;
    let (_, second) = sign_in(&app, &dev, &c).await;
    assert_eq!(
        me(&app, first["access_token"].as_str().unwrap()).await.0,
        401
    );
    assert_eq!(
        me(&app, &dev.access_token).await.0,
        401,
        "the fixture's session too"
    );
    assert_eq!(
        me(&app, second["access_token"].as_str().unwrap()).await.0,
        200
    );
    let reason: Option<String> =
        sqlx::query_scalar("SELECT end_reason FROM sessions WHERE id = $1")
            .bind(Uuid::parse_str(first["session_id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reason.as_deref(), Some("superseded"));
}

#[sqlx::test]
async fn plain_sign_out_keeps_the_key(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let req = TestApp::request(Method::POST, "/auth/logout")
        .header("Authorization", format!("Bearer {}", dev.access_token))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 200);
    let (_, c) = challenge(&app, &dev).await;
    assert_eq!(sign_in(&app, &dev, &c).await.0, 200);
}

#[sqlx::test]
async fn never_ip_blocked_and_own_limit(pool: PgPool) {
    use vc_server::ratelimit::{LimitConfig, RateLimitConfig, RateLimiter, RateLimits};
    let app = TestApp::with_pool(pool.clone()).await;
    let config = (*app.config).clone();
    let limits = RateLimits {
        auth_flow: LimitConfig {
            requests: 60,
            window_secs: 60,
        },
        ..RateLimits::default()
    };
    let mut limiter = RateLimiter::new(
        app.state.redis.clone(),
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
            fail_open: false,
            trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
            allowlist: std::collections::HashSet::new(),
            limits,
        },
    );
    limiter.init().await.unwrap();
    // The test peer is blocked for failed logins.
    for _ in 0..20 {
        let _ = limiter.record_failed_auth("203.0.113.9").await;
    }
    assert!(limiter.is_blocked("203.0.113.9").await.unwrap());
    let mut state = state_with_redis(pool.clone(), config, app.state.redis.clone());
    state.rate_limiter = Some(limiter);
    let router = vc_server::api::create_router(state);

    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let body = json!({"user_id": user, "device_id": dev.device_id});
    let (s, c, _) = public(&router, "/auth/device-login/challenge", body.clone()).await;
    assert_eq!(s, 201, "a blocked IP still signs in with its key: {c}");
    for _ in 0..59 {
        let (s, _, _) = public(&router, "/auth/device-login/challenge", body.clone()).await;
        assert_eq!(s, 201);
    }
    let (s, b, retry) = public(&router, "/auth/device-login/challenge", body).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{b}"
    );
    assert!(retry.is_some(), "Retry-After");
}
