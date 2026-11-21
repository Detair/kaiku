//! Per-account counters reserve a try before it is checked (B.5): a burst
//! of parallel tries on one account checks at most the free tries, every
//! other try answers 429 `TOO_MANY_ATTEMPTS` with `Retry-After` unchecked.
//!
//! Run with: `cargo test --test integration counter_reservation`

use axum::body::Body;
use axum::http::Method;
use axum::Router;
use fred::interfaces::{HashesInterface, KeysInterface};
use serde_json::{json, Value};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::ratelimit::account::{AccountLimiter, CounterKey, FREE_FAILURES};
use vc_server::ratelimit::{LimitConfig, RateLimitConfig, RateLimiter, RateLimits};

use super::helpers::login::{
    enable_totp, failing_valkey, password_user, peer_request, state_with_redis, TEST_PEER,
};
use super::helpers::{body_to_json_lenient, TestApp};
use super::login_attempts::{attempt_id, Secret};
use super::login_password::login_native;

const PASSWORD: &str = "zq8#vT4p-reserve";
const BURST: usize = 200;

/// A pool wide enough for a burst to run in parallel up to the account
/// lock (the default test pool of five connections would queue the tries
/// before they reach the counter).
async fn wide_pool(db: PgConnectOptions) -> PgPool {
    PgPoolOptions::new()
        .max_connections(60)
        .connect_with(db)
        .await
        .expect("test pool")
}

/// One answer: status, body, `Retry-After`.
type Answer = (u16, Value, Option<String>);

async fn send(router: &Router, req: axum::http::Request<Body>) -> Answer {
    let resp = tower::ServiceExt::oneshot(router.clone(), req)
        .await
        .expect("router");
    let status = resp.status().as_u16();
    let retry = resp
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    (status, body_to_json_lenient(resp).await, retry)
}

/// The app's router with the real per-IP limiter in front: the socket peer
/// is a trusted proxy (so `X-Forwarded-For` names the client), the attempt
/// routes' per-IP limit is raised so it cannot mask the per-account one.
async fn router_with_ip_limits(app: &TestApp) -> Router {
    let limits = RateLimits {
        auth_flow: LimitConfig {
            requests: 100_000,
            window_secs: 60,
        },
        ..RateLimits::default()
    };
    let peer = std::net::Ipv4Addr::from(TEST_PEER.0).to_string();
    let mut limiter = RateLimiter::new(
        app.state.redis.clone(),
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
            fail_open: false,
            trusted_proxies: vc_server::ratelimit::TrustedProxies::parse(&peer).unwrap(),
            allowlist: std::collections::HashSet::new(),
            limits,
        },
    );
    limiter.init().await.unwrap();
    let mut state = state_with_redis(
        app.pool.clone(),
        (*app.config).clone(),
        app.state.redis.clone(),
    );
    state.rate_limiter = Some(limiter);
    vc_server::api::create_router(state)
}

/// A distinct client address per index (documentation-free private range).
fn client_ip(i: usize) -> String {
    format!("10.77.{}.{}", i / 250, i % 250 + 1)
}

fn wrong_code(totp: &totp_rs::TOTP) -> String {
    let right: u32 = totp.generate_current().unwrap().parse().unwrap();
    format!("{:06}", (right + 500_000) % 1_000_000)
}

async fn totp_key(pool: &PgPool, user: Uuid) -> CounterKey {
    let g: i64 = sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap();
    CounterKey::totp(user, g)
}

async fn counted(app: &TestApp, key: &CounterKey) -> i64 {
    AccountLimiter::new(&app.state.redis)
        .read(key)
        .await
        .unwrap()
        .0
}

async fn set_count(app: &TestApp, key: &CounterKey, n: i64) {
    let k = key.redis_key();
    let _: () = app.state.redis.del(&k).await.unwrap();
    let _: () = app.state.redis.hset(&k, ("count", n)).await.unwrap();
}

async fn events(pool: &PgPool, user: Uuid, t: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = $2",
    )
    .bind(user)
    .bind(t)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// `(checked, refused)`: answers with `checked_code`, and 429
/// `TOO_MANY_ATTEMPTS` with `Retry-After`; panics on anything else.
fn tally(answers: &[Answer], checked_status: u16, checked_code: &str) -> (usize, usize) {
    let (mut checked, mut refused) = (0, 0);
    for (s, b, retry) in answers {
        match (*s, b["error"].as_str()) {
            (s, Some(c)) if s == checked_status && c == checked_code => checked += 1,
            (429, Some("TOO_MANY_ATTEMPTS")) => {
                let secs: u64 = retry.as_deref().expect("Retry-After").parse().unwrap();
                assert!(secs >= 1, "{secs}");
                refused += 1;
            }
            _ => panic!("unexpected answer {s}: {b}"),
        }
    }
    (checked, refused)
}

/// The password step of a native sign-in: `(attempt id, verifier)`.
async fn password_step(app: &TestApp, name: &str) -> (String, String) {
    let (s, step, secret) = login_native(app, name, PASSWORD).await;
    assert_eq!(s, 202, "{step}");
    let Secret::Verifier(v) = secret else {
        panic!("native sign-in");
    };
    (attempt_id(&step["attempt"]), v)
}

fn totp_request(id: &str, verifier: &str, code: &str) -> axum::http::Request<Body> {
    peer_request(Method::POST, &format!("/auth/login-attempts/{id}/totp"))
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .body(Body::from(
            json!({"code": code, "verifier": verifier}).to_string(),
        ))
        .unwrap()
}

fn login_request(name: &str, password: &str, ip: &str) -> axum::http::Request<Body> {
    peer_request(Method::POST, "/auth/login")
        .header("Content-Type", "application/json")
        .header("X-Forwarded-For", ip)
        .body(Body::from(
            json!({"username": name, "password": password}).to_string(),
        ))
        .unwrap()
}

// ---------------------------------------------------------------------------
// The TOTP step of a sign-in attempt
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn parallel_wrong_totp_codes_check_at_most_five(_: PgPoolOptions, db: PgConnectOptions) {
    let pool = wide_pool(db).await;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (id, verifier) = password_step(&app, &name).await;
    let router = router_with_ip_limits(&app).await;
    let wrong = wrong_code(&totp);
    let tries = (0..BURST)
        .map(|_| {
            let router = router.clone();
            let req = totp_request(&id, &verifier, &wrong);
            tokio::spawn(async move { send(&router, req).await })
        })
        .collect::<Vec<_>>();
    let mut answers = Vec::with_capacity(BURST);
    for t in tries {
        answers.push(t.await.unwrap());
    }
    let (checked, refused) = tally(&answers, 401, "INVALID_MFA");
    let failed_events = events(&pool, user, "second_factor_failed").await;
    assert!(
        checked <= FREE_FAILURES as usize,
        "{checked} codes checked, {refused} refused"
    );
    assert_eq!(checked + refused, BURST);
    assert_eq!(i64::try_from(checked).unwrap(), failed_events);
    assert_eq!(
        counted(&app, &totp_key(&pool, user).await).await,
        i64::try_from(checked).unwrap()
    );
}

#[sqlx::test]
async fn right_code_after_four_wrong_signs_in(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (id, verifier) = password_step(&app, &name).await;
    for _ in 0..FREE_FAILURES - 1 {
        let (s, b, _) = send(
            &app.router,
            totp_request(&id, &verifier, &wrong_code(&totp)),
        )
        .await;
        assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_MFA")), "{b}");
    }
    let right = totp.generate_current().unwrap();
    let (s, b, _) = send(&app.router, totp_request(&id, &verifier, &right)).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(counted(&app, &totp_key(&pool, user).await).await, 0);
}

#[sqlx::test]
async fn totp_success_resets_the_counter(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (id, verifier) = password_step(&app, &name).await;
    for _ in 0..FREE_FAILURES - 1 {
        send(
            &app.router,
            totp_request(&id, &verifier, &wrong_code(&totp)),
        )
        .await;
    }
    let right = totp.generate_current().unwrap();
    assert_eq!(
        send(&app.router, totp_request(&id, &verifier, &right))
            .await
            .0,
        200
    );
    // A new sign-in has all five tries again.
    let (id, verifier) = password_step(&app, &name).await;
    for i in 0..FREE_FAILURES {
        let (s, b, _) = send(
            &app.router,
            totp_request(&id, &verifier, &wrong_code(&totp)),
        )
        .await;
        assert_eq!(
            (s, b["error"].as_str()),
            (401, Some("INVALID_MFA")),
            "try {i}: {b}"
        );
    }
    let (s, b, retry) = send(
        &app.router,
        totp_request(&id, &verifier, &wrong_code(&totp)),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (429, Some("TOO_MANY_ATTEMPTS")));
    assert!(retry.is_some());
}

// ---------------------------------------------------------------------------
// The password on /auth/login
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn parallel_wrong_passwords_check_at_most_five(_: PgPoolOptions, db: PgConnectOptions) {
    let pool = wide_pool(db).await;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let router = router_with_ip_limits(&app).await;
    // One client address per try: the per-IP limit (5 per minute) and the
    // IP block never apply, only the per-account counter.
    let tries = (0..BURST)
        .map(|i| {
            let router = router.clone();
            let req = login_request(&name, "wrong-password-1", &client_ip(i));
            tokio::spawn(async move { send(&router, req).await })
        })
        .collect::<Vec<_>>();
    let mut answers = Vec::with_capacity(BURST);
    for t in tries {
        answers.push(t.await.unwrap());
    }
    let (checked, refused) = tally(&answers, 401, "INVALID_CREDENTIALS");
    assert!(
        checked <= FREE_FAILURES as usize,
        "{checked} passwords checked, {refused} refused"
    );
    assert_eq!(checked + refused, BURST);
    assert_eq!(
        counted(&app, &CounterKey::password(&name)).await,
        i64::try_from(checked).unwrap()
    );
}

#[sqlx::test]
async fn right_password_after_four_wrong_resets_the_counter(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    for _ in 0..FREE_FAILURES - 1 {
        let (s, b, _) = send(
            &app.router,
            login_request(&name, "wrong-password-1", "198.51.100.7"),
        )
        .await;
        assert_eq!(s, 401, "{b}");
    }
    let (s, b, _) = send(&app.router, login_request(&name, PASSWORD, "198.51.100.7")).await;
    assert_eq!(s, 202, "{b}");
    assert_eq!(counted(&app, &CounterKey::password(&name)).await, 0);
    for i in 0..FREE_FAILURES {
        let (s, b, _) = send(
            &app.router,
            login_request(&name, "wrong-password-1", "198.51.100.7"),
        )
        .await;
        assert_eq!(s, 401, "try {i}: {b}");
    }
    let (s, b, _) = send(&app.router, login_request(&name, PASSWORD, "198.51.100.7")).await;
    assert_eq!((s, b["error"].as_str()), (429, Some("TOO_MANY_ATTEMPTS")));
}

// ---------------------------------------------------------------------------
// Account requests and owner-rule part 2
// ---------------------------------------------------------------------------

fn bootstrap_request(name: &str, code: &str) -> axum::http::Request<Body> {
    use super::account_requests::PASSWORD as BP;
    peer_request(Method::POST, "/auth/account-requests")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "android")
        .body(Body::from(
            json!({"username": name, "purpose": "bootstrap",
                   "first": {"type": "password", "password": BP}, "totp_code": code})
            .to_string(),
        ))
        .unwrap()
}

#[sqlx::test]
async fn bootstrap_totp_burst(_: PgPoolOptions, db: PgConnectOptions) {
    use super::account_requests::{admin_with_device, bootstrap_user};
    let pool = wide_pool(db).await;
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    // Three wrong codes counted already; the start budget allows three
    // starts, so only two more codes may be checked.
    let key = totp_key(&pool, user).await;
    set_count(&app, &key, 3).await;
    let wrong = wrong_code(&totp);
    let tries = (0..20)
        .map(|_| {
            let router = app.router.clone();
            let req = bootstrap_request(&name, &wrong);
            tokio::spawn(async move { send(&router, req).await })
        })
        .collect::<Vec<_>>();
    for t in tries {
        let (s, b, _) = t.await.unwrap();
        // The code's refusals are the neutral answer; parallel password
        // tries are reserved too, so beyond five in flight they wait.
        assert!(
            matches!(
                (s, b["error"].as_str()),
                (401, Some("INVALID_CREDENTIALS")) | (429, Some("TOO_MANY_ATTEMPTS"))
            ),
            "{s}: {b}"
        );
    }
    let n = counted(&app, &key).await;
    assert!(n <= i64::from(FREE_FAILURES), "{n} codes counted");
    let starts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_request_starts WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(starts, n - 3, "a start that checked no code is not counted");
}

#[sqlx::test]
async fn recovery_totp_burst(_: PgPoolOptions, db: PgConnectOptions) {
    use super::account_requests::admin_with_device;
    let pool = wide_pool(db).await;
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let key = totp_key(&pool, user).await;
    set_count(&app, &key, 3).await;
    let wrong = wrong_code(&totp);
    let tries = codes
        .iter()
        .map(|code| {
            let router = app.router.clone();
            let req = peer_request(Method::POST, "/auth/account-requests")
                .header("Content-Type", "application/json")
                .header("X-Kaiku-Client", "android")
                .body(Body::from(
                    json!({"username": name, "purpose": "recovery", "recovery_code": code,
                       "second": {"type": "totp", "code": wrong}, "lost": "none"})
                    .to_string(),
                ))
                .unwrap();
            tokio::spawn(async move { send(&router, req).await })
        })
        .collect::<Vec<_>>();
    for t in tries {
        let (s, b, _) = t.await.unwrap();
        assert!(
            matches!(
                (s, b["error"].as_str()),
                (401, Some("REQUEST_FACTOR_INVALID" | "INVALID_CREDENTIALS"))
                    | (429, Some("TOO_MANY_ATTEMPTS"))
            ),
            "{s}: {b}"
        );
    }
    let n = counted(&app, &key).await;
    assert!(n <= i64::from(FREE_FAILURES), "{n} codes counted");
    let mut conn = pool.acquire().await.unwrap();
    let left = vc_server::auth::recovery_codes::left_conn(&mut conn, user)
        .await
        .unwrap();
    assert_eq!(
        i64::try_from(codes.len()).unwrap() - left,
        n - 3,
        "only a code whose second factor was checked is consumed"
    );
}

#[sqlx::test]
async fn part2_totp_burst(_: PgPoolOptions, db: PgConnectOptions) {
    use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap};
    let pool = wide_pool(db).await;
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let id = b["request"]["id"].as_str().unwrap().to_string();
    let (_, token) =
        super::helpers::create_probation_session_with_token(&pool, &app.config, user).await;
    let wrong = wrong_code(&totp);
    let tries = (0..BURST)
        .map(|_| {
            let router = app.router.clone();
            let req = peer_request(Method::POST, &format!("/api/account-requests/{id}/cancel"))
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::from(
                    json!({"reason": null, "part2": {"factor": {"type": "totp", "code": wrong}}})
                        .to_string(),
                ))
                .unwrap();
            tokio::spawn(async move { send(&router, req).await })
        })
        .collect::<Vec<_>>();
    let mut answers = Vec::with_capacity(BURST);
    for t in tries {
        answers.push(t.await.unwrap());
    }
    let (checked, refused) = tally(&answers, 401, "INVALID_MFA");
    assert!(
        checked <= FREE_FAILURES as usize,
        "{checked} codes checked, {refused} refused"
    );
    assert_eq!(checked + refused, BURST);
}

// ---------------------------------------------------------------------------
// Valkey down: fail closed (503), nothing consumed
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn bootstrap_valkey_down_fails_closed(pool: PgPool) {
    use super::account_requests::{admin_with_device, bootstrap_user};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (_, name, totp) = bootstrap_user(&app).await;
    let broken = vc_server::api::create_router(state_with_redis(
        pool.clone(),
        (*app.config).clone(),
        failing_valkey(&app.config).await,
    ));
    let (s, b, _) = send(
        &broken,
        bootstrap_request(&name, &totp.generate_current().unwrap()),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (503, Some("SERVICE_UNAVAILABLE")),
        "{b}"
    );
}

#[sqlx::test]
async fn recovery_valkey_down_fails_closed(pool: PgPool) {
    use super::account_requests::admin_with_device;
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let broken = vc_server::api::create_router(state_with_redis(
        pool.clone(),
        (*app.config).clone(),
        failing_valkey(&app.config).await,
    ));
    let req = peer_request(Method::POST, "/auth/account-requests")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "android")
        .body(Body::from(
            json!({"username": name, "purpose": "recovery", "recovery_code": codes[0],
                   "second": {"type": "totp", "code": totp.generate_current().unwrap()},
                   "lost": "none"})
            .to_string(),
        ))
        .unwrap();
    let (s, b, _) = send(&broken, req).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (503, Some("SERVICE_UNAVAILABLE")),
        "{b}"
    );
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
async fn part2_valkey_down_fails_closed(pool: PgPool) {
    use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let id = b["request"]["id"].as_str().unwrap().to_string();
    let (_, token) =
        super::helpers::create_probation_session_with_token(&pool, &app.config, user).await;
    let broken = vc_server::api::create_router(state_with_redis(
        pool.clone(),
        (*app.config).clone(),
        failing_valkey(&app.config).await,
    ));
    let req = peer_request(Method::POST, &format!("/api/account-requests/{id}/cancel"))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({"reason": null,
                   "part2": {"factor": {"type": "totp", "code": wrong_code(&totp)}}})
            .to_string(),
        ))
        .unwrap();
    let (s, b, _) = send(&broken, req).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (503, Some("SERVICE_UNAVAILABLE")),
        "{b}"
    );
}

// ---------------------------------------------------------------------------
// The command-line reset frees what the reservations counted
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn cli_reset_frees_reserved_counters(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (id, verifier) = password_step(&app, &name).await;
    for _ in 0..FREE_FAILURES {
        send(
            &app.router,
            totp_request(&id, &verifier, &wrong_code(&totp)),
        )
        .await;
        send(
            &app.router,
            login_request(&name, "wrong-password-1", "198.51.100.7"),
        )
        .await;
    }
    assert_eq!(
        send(&app.router, login_request(&name, PASSWORD, "198.51.100.7"))
            .await
            .0,
        429
    );
    assert_eq!(
        send(
            &app.router,
            totp_request(&id, &verifier, &wrong_code(&totp))
        )
        .await
        .0,
        429
    );
    super::cli_recovery::cli(&app, &["throttle", "reset", &name])
        .await
        .unwrap();
    assert_eq!(counted(&app, &CounterKey::password(&name)).await, 0);
    assert_eq!(counted(&app, &totp_key(&pool, user).await).await, 0);
    let right = totp.generate_current().unwrap();
    assert_eq!(
        send(&app.router, totp_request(&id, &verifier, &right))
            .await
            .0,
        200
    );
    let (id, verifier) = password_step(&app, &name).await;
    assert_ne!(
        send(
            &app.router,
            totp_request(&id, &verifier, &wrong_code(&totp))
        )
        .await
        .0,
        429
    );
}
