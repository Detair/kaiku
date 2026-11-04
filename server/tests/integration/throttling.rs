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
    let k = key.redis_key();
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
    // Valkey outlives the test database: a fresh name per run.
    let name = format!("nobody_{}", &Uuid::new_v4().simple().to_string()[..12]);
    for _ in 0..5 {
        assert_eq!(login(&app, &name, "x-password").await.0, 401);
    }
    let (s, b, _) = login(&app, &name.to_uppercase(), "x-password").await;
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
    set_count(&app, &totp_key(&app, user).await, 99).await;
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": "000000"}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_MFA")), "{b}");
    // The effects run in their own transaction after the answer.
    let mut states = (String::new(), String::new());
    for _ in 0..50 {
        states = sqlx::query_as("SELECT password_state, totp_state FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
        if states.1 == "throttled" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(
        (states.0.as_str(), states.1.as_str()),
        ("compromised", "throttled")
    );
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

// ---------------------------------------------------------------------------
// Every TOTP check counts and waits (review round 1: Fable B1, Codex M3)
// ---------------------------------------------------------------------------

async fn retry_after_on_sixth<F, Fut>(mut call: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = (u16, Value, Option<String>)>,
{
    for i in 0..5 {
        let (s, b, _) = call().await;
        assert_ne!(s, 429, "try {i}: {b}");
    }
    let (s, b, retry) = call().await;
    assert_eq!(
        (s, b["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{b}"
    );
    assert!(retry.is_some(), "Retry-After");
}

async fn post_raw(
    app: &TestApp,
    uri: &str,
    headers: &[(&str, &str)],
    body: Value,
) -> (u16, Value, Option<String>) {
    let mut req = peer_request(Method::POST, uri).header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    let retry = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (status, body_to_json_lenient(resp).await, retry)
}

#[sqlx::test]
async fn oidc_attempt_totp_step_is_throttled(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    // The same attempt as an OIDC one (no password generation).
    sqlx::query(
        "UPDATE login_attempts SET kind = 'oidc', first_factor = 'oidc' WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    retry_after_on_sixth(|| async {
        let (s, b, h) = pc_call(
            &app,
            &format!("/auth/login-attempts/{id}/totp"),
            &secret,
            json!({"code": "000000"}),
        )
        .await;
        let retry = h
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        (s, b, retry)
    })
    .await;
}

#[sqlx::test]
async fn bootstrap_totp_is_throttled(pool: PgPool) {
    use super::account_requests::{admin_with_device, bootstrap_user, PASSWORD as BP};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, _) = bootstrap_user(&app).await;
    // Five wrong codes already counted (the start budget would stop the
    // starts themselves at three).
    set_count(&app, &totp_key(&app, user).await, 5).await;
    let _: () = app
        .state
        .redis
        .hset(
            totp_key(&app, user).await.redis_key(),
            ("next", chrono::Utc::now().timestamp() + 30),
        )
        .await
        .unwrap();
    let (s, b, retry) = post_raw(
        &app,
        "/auth/account-requests",
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "bootstrap",
               "first": {"type": "password", "password": BP}, "totp_code": "000000"}),
    )
    .await;
    // Fable r2 (i): neutral (the delay only shows after a correct password).
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_CREDENTIALS")),
        "{b}"
    );
    let _ = retry;
}

#[sqlx::test]
async fn recovery_totp_is_throttled_before_a_code_is_consumed(pool: PgPool) {
    use super::account_requests::admin_with_device;
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    set_count(&app, &totp_key(&app, user).await, 5).await;
    let _: () = app
        .state
        .redis
        .hset(
            totp_key(&app, user).await.redis_key(),
            ("next", chrono::Utc::now().timestamp() + 30),
        )
        .await
        .unwrap();
    let (s, b, retry) = post_raw(
        &app,
        "/auth/account-requests",
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "recovery", "recovery_code": codes[0],
               "second": {"type": "totp", "code": "000000"}, "lost": "none"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (429, Some("TOO_MANY_ATTEMPTS")),
        "{b}"
    );
    assert!(retry.is_some());
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(
        vc_server::auth::recovery_codes::left_conn(&mut conn, user)
            .await
            .unwrap(),
        10,
        "no code consumed"
    );
}

#[sqlx::test]
async fn part2_totp_is_throttled(pool: PgPool) {
    use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let id = b["request"]["id"].as_str().unwrap().to_string();
    let (_, token) =
        super::helpers::create_probation_session_with_token(&pool, &app.config, user).await;
    retry_after_on_sixth(|| async {
        let auth = format!("Bearer {token}");
        post_raw(
            &app,
            &format!("/api/account-requests/{id}/cancel"),
            &[("Authorization", auth.as_str())],
            json!({"reason": null, "part2": {"factor": {"type": "totp", "code": "000000"}}}),
        )
        .await
    })
    .await;
}

#[sqlx::test]
async fn bootstrap_starts_share_the_budget(pool: PgPool) {
    use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    for i in 0..3 {
        let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
        assert_eq!(s, 201, "start {i}: {b}");
        let secret = b["request_secret"].as_str().unwrap().to_string();
        let req = peer_request(Method::DELETE, "/auth/account-requests/current")
            .header("X-Kaiku-Request-Secret", &secret)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app.oneshot(req).await.status().as_u16(), 204);
    }
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_CREDENTIALS")),
        "{b}"
    );
}

/// Codex r2 M6: with a delayed counter an invalid code is the neutral
/// answer, as for an unknown account (the delay shows only after a valid
/// code, still before it is consumed).
#[sqlx::test]
async fn an_invalid_code_is_neutral_while_delayed(pool: PgPool) {
    use super::account_requests::admin_with_device;
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    set_count(&app, &totp_key(&app, user).await, 5).await;
    let _: () = app
        .state
        .redis
        .hset(
            totp_key(&app, user).await.redis_key(),
            ("next", chrono::Utc::now().timestamp() + 30),
        )
        .await
        .unwrap();
    let body = |u: &str| {
        json!({"username": u, "purpose": "recovery", "recovery_code": "AAAAA-AAAAA-AAA",
               "second": {"type": "totp", "code": "000000"}, "lost": "none"})
    };
    let known = post_raw(
        &app,
        "/auth/account-requests",
        &[("X-Kaiku-Client", "android")],
        body(&name),
    )
    .await;
    let unknown = post_raw(
        &app,
        "/auth/account-requests",
        &[("X-Kaiku-Client", "android")],
        body("no_such_account_r2"),
    )
    .await;
    assert_eq!((known.0, &known.1), (unknown.0, &unknown.1));
    assert_eq!(known.0, 401, "{}", known.1);
}

/// Codex r2: the 100th failure against a TOTP replaced since throttles
/// nothing (its generation is re-checked under the account lock).
#[sqlx::test]
async fn a_stale_totp_failure_does_not_throttle_the_new_totp(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let old: i64 = sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET totp_generation = $2 WHERE id = $1")
        .bind(user)
        .bind(old + 1)
        .execute(&pool)
        .await
        .unwrap();
    set_count(&app, &totp_key(&app, user).await, 99).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    vc_server::auth::totp_guard::after(
        &ctx,
        None,
        user,
        false,
        vc_server::auth::totp_guard::Used {
            totp_generation: old,
            password: None,
        },
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let state: String = sqlx::query_scalar("SELECT totp_state FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
    // The same failure against the current TOTP throttles it.
    set_count(&app, &totp_key(&app, user).await, 99).await;
    vc_server::auth::totp_guard::after(
        &ctx,
        None,
        user,
        false,
        vc_server::auth::totp_guard::Used {
            totp_generation: old + 1,
            password: None,
        },
    )
    .await;
    let mut state = String::new();
    for _ in 0..40 {
        state = sqlx::query_scalar("SELECT totp_state FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
        if state == "throttled" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(state, "throttled");
}

/// Fable r3: a TOTP renewal starts a fresh counter (the old secret's
/// failures do not delay the new one).
#[sqlx::test]
async fn a_totp_renewal_starts_a_fresh_counter(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    for _ in 0..5 {
        let (s, b, _) = pc_call(
            &app,
            &format!("/auth/login-attempts/{id}/totp"),
            &secret,
            json!({"code": "000000"}),
        )
        .await;
        assert_eq!(s, 401, "{b}");
    }
    // Renewed (a new secret, a new generation).
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    sqlx::query("UPDATE users SET totp_generation = totp_generation + 1 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200, "the new TOTP is not delayed: {b}");
}

/// Codex r3: effects scheduled before a success never apply after it, even
/// when the count is high again (a new epoch).
#[sqlx::test]
async fn effects_of_an_old_epoch_never_apply(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let generation: i64 = sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    let used = vc_server::auth::totp_guard::Used {
        totp_generation: generation,
        password: None,
    };
    let key = vc_server::ratelimit::account::CounterKey::totp(user, generation);
    let _: () = app.state.redis.del(key.redis_key()).await.unwrap();
    let _: () = app
        .state
        .redis
        .hset(key.redis_key(), ("count", 99))
        .await
        .unwrap();
    // The effects wait for the account lock this test holds.
    let mut hold = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut hold, &[user])
        .await
        .unwrap();
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    vc_server::auth::totp_guard::after(&ctx, None, user, false, used).await;
    // A success, then many new failures (the count is high again).
    vc_server::auth::totp_guard::after(&ctx, None, user, true, used).await;
    let _: () = app
        .state
        .redis
        .hset(key.redis_key(), ("count", 150))
        .await
        .unwrap();
    hold.commit().await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let state: String = sqlx::query_scalar("SELECT totp_state FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
}

/// The counter of `user`'s current TOTP generation.
async fn totp_key(app: &TestApp, user: Uuid) -> CounterKey {
    let g: i64 = sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    CounterKey::totp(user, g)
}

/// Codex r4 #2: a success resets only the counter of the generation it was
/// checked against. The trigger renews the TOTP inside the successful
/// step's own transaction, so its post-commit reset runs after a renewal
/// whose counter already holds failures; that count stays.
#[sqlx::test]
async fn a_delayed_success_keeps_the_renewed_generations_count(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let old = totp_key(&app, user).await;
    let CounterKey::Totp(_, g) = old else {
        unreachable!()
    };
    let renewed = CounterKey::totp(user, g + 1);
    set_count(&app, &renewed, 7).await;
    for sql in [
        "CREATE FUNCTION test_renew_totp() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN
           UPDATE users SET totp_generation = totp_generation + 1 WHERE id = NEW.user_id;
           RETURN NEW;
         END $$",
        "CREATE TRIGGER test_renew_totp AFTER INSERT ON sessions
         FOR EACH ROW EXECUTE FUNCTION test_renew_totp()",
    ] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(totp_key(&app, user).await, renewed, "the trigger renewed");
    let (count, _) = vc_server::ratelimit::account::AccountLimiter::new(&app.state.redis)
        .read(&renewed)
        .await
        .unwrap();
    assert_eq!(count, 7, "the renewed generation's failures stay");
}

/// Codex r4 #3: the count, the delay and the expiry are one script; its
/// delay schedule is `delay_after`'s.
#[sqlx::test]
async fn the_failure_script_sets_the_schedules_delay(pool: PgPool) {
    use vc_server::ratelimit::account::{delay_after, AccountLimiter};
    let app = TestApp::with_pool(pool.clone()).await;
    let key = CounterKey::totp(Uuid::new_v4(), 1);
    let limiter = AccountLimiter::new(&app.state.redis);
    for before in [0_i64, 3, 4, 5, 10, 11, 50] {
        set_count(&app, &key, before).await;
        let start = chrono::Utc::now().timestamp();
        let (n, epoch) = limiter.record_failure_epoch(&key).await.unwrap();
        assert_eq!((i64::from(n), epoch), (before + 1, 0));
        let next: Option<i64> = app.state.redis.hget(key.redis_key(), "next").await.unwrap();
        match delay_after(n) {
            None => assert_eq!(next, None, "n = {n}"),
            Some(d) => {
                let wait = next.expect("a delay") - start;
                let want = i64::try_from(d.as_secs()).unwrap();
                assert!(
                    (want..=want + 2).contains(&wait),
                    "n = {n}: {wait} vs {want}"
                );
            }
        }
        let ttl: i64 = app.state.redis.ttl(key.redis_key()).await.unwrap();
        assert!(ttl > 23 * 3600, "the expiry is set: {ttl}");
    }
}
