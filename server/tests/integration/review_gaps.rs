//! Negative paths Fable's review round 1 (M4) listed: request voids and
//! races, throttle completion, TOTP-step refusals, probation refusals.
//!
//! Run with: `cargo test --test integration review_gaps`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::cli::{run, CliCtx, CliIo, Command};

use super::account_requests::{admin_with_device, bootstrap_user, post, start_bootstrap};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, password_user, peer_request};
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_test_user, grant_step_up,
    TestApp,
};
use super::login_attempts::{attempt_id, claim, create_native, pc_call, qr_token};
use super::login_password::login_native;

struct Quiet;

impl CliIo for Quiet {
    fn print(&mut self, _s: &str) {}
    async fn read_line(&mut self, _p: &str) -> std::io::Result<String> {
        Err(std::io::ErrorKind::UnexpectedEof.into())
    }
}

async fn cli(app: &TestApp, args: &[&str]) {
    let ctx = CliCtx {
        db: app.pool.clone(),
        redis: app.state.redis.clone(),
        config: (*app.config).clone(),
    };
    let args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(&ctx, Command::parse(&args).unwrap(), &mut Quiet)
        .await
        .unwrap();
}

async fn state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM account_requests WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn username(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A started bootstrap request: `(user, request id, request secret)`.
async fn started(app: &TestApp) -> (Uuid, Uuid, String) {
    let (user, name, totp) = bootstrap_user(app).await;
    let (s, b) = start_bootstrap(app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    (
        user,
        b["request"]["id"].as_str().unwrap().parse().unwrap(),
        b["request_secret"].as_str().unwrap().to_string(),
    )
}

async fn make_ready(app: &TestApp, id: Uuid) {
    sqlx::query("UPDATE account_requests SET ready_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(id)
        .execute(&app.pool)
        .await
        .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
}

#[sqlx::test]
async fn a_late_cancel_loses_against_completion(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, adev) = admin_with_device(&app).await;
    let (_, id, secret) = started(&app).await;
    make_ready(&app, id).await;
    let (s, b) = post(
        &app,
        "/auth/account-requests/current/finish",
        None,
        &[("X-Kaiku-Request-Secret", &secret)],
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    grant_step_up(
        &pool,
        adev.session_id.unwrap(),
        "request.cancel",
        &id.to_string(),
    )
    .await;
    let (s, b) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&adev.access_token),
        &[],
        json!({"reason": null, "part2": null}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("ACCOUNT_REQUEST_COMPLETED")),
        "{b}"
    );
}

#[sqlx::test]
async fn the_only_routes_gone_voids_the_request(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    // The admin route: its only device removed → void.
    let (admin, adev) = admin_with_device(&app).await;
    let (_, id, _) = started(&app).await;
    let admin_name = username(&pool, admin).await;
    cli(
        &app,
        &["devices", "remove", &admin_name, &adev.row_id.to_string()],
    )
    .await;
    assert_eq!(state(&pool, id).await, "void");

    // The admin demoted → its route is gone → void.
    let (admin, _) = admin_with_device(&app).await;
    let (_, id, _) = started(&app).await;
    let admin_name = username(&pool, admin).await;
    cli(&app, &["admin", "demote", &admin_name]).await;
    assert_eq!(state(&pool, id).await, "void");
}

#[sqlx::test]
async fn a_contact_change_voids_a_request_it_was_the_route_of(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (contact, contact_name) = create_test_user(&pool).await;
    KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    cli(&app, &["recovery-contact", &name, &contact_name]).await;
    // Confirmed long enough ago to count (one delay period).
    sqlx::query(
        "UPDATE users SET recovery_contact_confirmed_at = NOW() - INTERVAL '3 days',
                recovery_contact_named_at = NOW() - INTERVAL '4 days'
          WHERE id = $1",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "the contact is the only route: {b}");
    let id: Uuid = b["request"]["id"].as_str().unwrap().parse().unwrap();
    cli(&app, &["recovery-contact", &name, "--none"]).await;
    assert_eq!(state(&pool, id).await, "void");
}

#[sqlx::test]
async fn a_totp_renewal_before_finish_voids(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, id, secret) = started(&app).await;
    make_ready(&app, id).await;
    sqlx::query("UPDATE users SET totp_generation = totp_generation + 1 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = post(
        &app,
        "/auth/account-requests/current/finish",
        None,
        &[("X-Kaiku-Request-Secret", &secret)],
        json!({}),
    )
    .await;
    assert_eq!((s, b["state"].as_str()), (409, Some("void")), "{b}");
}

#[sqlx::test]
async fn a_success_resets_the_password_counter(pool: PgPool) {
    use fred::interfaces::KeysInterface;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, "zq8#vT4p-reset").await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    for _ in 0..4 {
        assert_eq!(login_native(&app, &name, "wrong-password-1").await.0, 401);
    }
    assert_eq!(login_native(&app, &name, "zq8#vT4p-reset").await.0, 202);
    let vc_server::ratelimit::account::CounterKey::Password(h) =
        vc_server::ratelimit::account::CounterKey::password(&name)
    else {
        unreachable!()
    };
    let exists: i64 = app
        .state
        .redis
        .exists(format!("ratelimit:acct:password:{h}"))
        .await
        .unwrap();
    assert_eq!(exists, 0, "a started attempt resets the counter");
    // Five more wrong ones are needed for a delay again.
    for _ in 0..4 {
        assert_eq!(login_native(&app, &name, "wrong-password-1").await.0, 401);
    }
}

#[sqlx::test]
async fn totp_step_refusals(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, "zq8#vT4p-step").await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;

    // A QR attempt has no TOTP step.
    let (a, secret) = create_native(&app).await;
    let id = attempt_id(&a);
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": "123456"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("TOTP_UNAVAILABLE")),
        "{b}"
    );

    // A compromised TOTP: 403 TOTP_UNAVAILABLE.
    let (_, step, secret) = login_native(&app, &name, "zq8#vT4p-step").await;
    let id = attempt_id(&step["attempt"]);
    sqlx::query("UPDATE users SET totp_state = 'compromised' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("TOTP_UNAVAILABLE")),
        "{b}"
    );
    sqlx::query("UPDATE users SET totp_state = 'active' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    // "Phone not at hand?" while the phone holds the claim: its challenge
    // becomes void.
    let (_, step, secret) = login_native(&app, &name, "zq8#vT4p-step").await;
    let attempt = &step["attempt"];
    let (token, _) = qr_token(attempt);
    let (s, c) = claim(&app, &dev, &token).await;
    assert_eq!(s, 201, "{c}");
    let id = attempt_id(attempt);
    {
        use fred::interfaces::KeysInterface;
        let _: Result<(), _> = app
            .state
            .redis
            .del::<(), _>(format!("mfa:totp_step:{user}"))
            .await;
    }
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let challenge: String = sqlx::query_scalar("SELECT state FROM approvals WHERE id = $1::uuid")
        .bind(c["challenge_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(challenge, "expired");
}

#[sqlx::test]
async fn probation_cannot_cancel_or_repudiate_without_part2(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, id, _) = started(&app).await;
    let (_, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&token),
        &[],
        json!({"reason": null, "part2": null}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{b}"
    );
    let event: Uuid = sqlx::query_scalar(
        "SELECT id FROM security_events WHERE subject_id = $1 AND event_type = 'request_started'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    // A sign-in event of the account to report.
    let mut conn = pool.acquire().await.unwrap();
    let signin = vc_server::security_events::record(
        &mut conn,
        vc_server::security_events::NewSecurityEvent::new(
            user,
            vc_server::security_events::EventType::PasswordChanged,
            vc_server::security_events::Actor::User(user),
            json!({}),
        ),
    )
    .await
    .unwrap();
    drop(conn);
    let req = peer_request(
        Method::POST,
        &format!("/api/me/security-events/{signin}/repudiate"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from("{}"))
    .unwrap();
    let resp = app.oneshot(req).await;
    let s = resp.status().as_u16();
    let b: Value = body_to_json_lenient(resp).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{b}"
    );
    let _ = event;
}
