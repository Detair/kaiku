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
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, "zq8#vT4p-reset").await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    for _ in 0..4 {
        assert_eq!(login_native(&app, &name, "wrong-password-1").await.0, 401);
    }
    assert_eq!(login_native(&app, &name, "zq8#vT4p-reset").await.0, 202);
    // A success starts a new epoch (count 0, no delay).
    let (count, _) = vc_server::ratelimit::account::AccountLimiter::new(&app.state.redis)
        .read(&vc_server::ratelimit::account::CounterKey::password(&name))
        .await
        .unwrap();
    assert_eq!(count, 0, "a started attempt resets the counter");
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

/// Fable r2 M-B: an admin whose only key re-enrols on the same install
/// keeps being a route; another account's request that records only that
/// admin stays open (the new key exists before the old one's cascade).
#[sqlx::test]
async fn same_install_re_enrolment_keeps_the_admin_route(pool: PgPool) {
    use super::enrollments::{claim_with, good, Phone};
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, adev) = admin_with_device(&app).await;
    let (_, id, _) = started(&app).await;
    // The admin re-enrols the same install (a host CLI setup: the issuing
    // device itself may not, L1a ENROLLMENT_DEVICE_IN_USE).
    let same = Phone {
        key: p256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng),
        device_id: adev.device_id,
    };
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let cli = vc_server::approvals::enrollments::issue_tx(
        &mut tx,
        &ctx,
        vc_server::approvals::enrollments::IssueParams {
            variant: vc_server::approvals::enrollments::EnrollmentVariant::Cli,
            subject_id: admin,
            issuer: vc_server::approvals::enrollments::Issuer::Cli,
            claimant: vc_server::approvals::context::ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let (s, claimed) =
        claim_with(&app, &cli.setup_token, &same, "Pixel 9", good(), "per_use").await;
    assert_eq!(s, 201, "{claimed}");
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::enrollments::confirm_tx(
        &mut tx,
        &ctx,
        cli.enrollment_id,
        vc_server::approvals::enrollments::Confirmer::Cli,
        claimed["code"].as_str().unwrap().as_bytes(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approval_devices WHERE user_id = $1 AND state = 'active'",
    )
    .bind(admin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(live, 1, "the new key replaced the old one");
    assert_eq!(
        state(&pool, id).await,
        "waiting",
        "the request keeps its route"
    );
}

/// Codex r3 (Major): a phone confirmation of an `admin` enrolment takes
/// its whole account scope (subject, every possible canceller) in one
/// ascending statement before any device, challenge or enrolment row, so
/// a transition that holds a lower canceller and then wants the subject
/// never deadlocks with it (40P01 before the fix).
#[sqlx::test]
async fn an_admin_confirmation_and_a_canceller_transition_never_deadlock(pool: PgPool) {
    use super::enrollments::{claim_with, good, Phone};
    use super::helpers::make_admin;
    let app = TestApp::with_pool(pool.clone()).await;
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(create_test_user(&pool).await.0);
    }
    ids.sort();
    let (a, x, b) = (ids[0], ids[1], ids[2]);
    make_admin(&pool, a).await;
    make_admin(&pool, x).await;
    KeyDeviceFixture::insert(&pool, &app.config, a, "active").await;
    let xdev = KeyDeviceFixture::insert(&pool, &app.config, x, "active").await;
    sqlx::query("UPDATE users SET first_activity_at = NOW() - INTERVAL '1 day' WHERE id = $1")
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
    grant_step_up(
        &pool,
        xdev.session_id.unwrap(),
        "admin.enrollment.issue",
        &b.to_string(),
    )
    .await;
    let (s, issued) = post(
        &app,
        &format!("/api/admin/users/{b}/enrollments"),
        Some(&xdev.access_token),
        &[],
        json!({"mode": "setup", "claimant_check": {"method": "in_person", "note": "x"}}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let phone = Phone::new();
    let (s, claimed) = claim_with(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    let id = issued["enrollment_id"].as_str().unwrap();
    let (s, c) = post(
        &app,
        &format!("/auth/enrollments/{id}/challenge"),
        Some(&xdev.access_token),
        &[],
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{c}");
    // A canceller transition: holds A, then wants B (ascending, B.7).
    let mut t1 = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut t1, &[a])
        .await
        .unwrap();
    let code = claimed["code"].as_str().unwrap().to_string();
    let answer = {
        let app = &app;
        let xdev = &xdev;
        let c = c.clone();
        async move { super::approvals_core::answer(app, xdev, &c, code.as_bytes()).await }
    };
    let other = async {
        super::helpers::wait_for_lock_waiters(&pool, 1).await;
        let r = vc_server::approvals::transition::lock_accounts(&mut t1, &[b]).await;
        let ok = r.is_ok();
        t1.commit().await.ok();
        ok
    };
    let ((s, body), t1_ok) = tokio::join!(answer, other);
    assert!(t1_ok, "the canceller transition must not deadlock (40P01)");
    assert_eq!(s, 200, "{body}");
}

/// Fable r3: part 2 re-checks the whole live-session predicate under the
/// account lock (an expired absolute cap with a valid expiry is refused).
#[sqlx::test]
async fn part2_refuses_a_session_past_its_absolute_cap(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, "zq8#vT4p-cap").await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (sid, _) = create_probation_session_with_token(&pool, &app.config, user).await;
    let auth = super::helpers::login::auth_user(&app, sid).await;
    // The control: the same session within its cap passes part 2.
    let code = totp.generate_current().unwrap();
    let ok = part2_with_cap(&app, &auth, &code, 3600.0).await;
    assert!(
        matches!(
            ok,
            Ok(vc_server::auth::owner_rule::OwnerAuthority::Part2 { factor: "totp" })
        ),
        "{ok:?}"
    );
    let r = part2_with_cap(&app, &auth, &code, -1.0).await;
    assert!(
        matches!(r, Err(vc_server::auth::AuthError::InvalidToken)),
        "a session past its absolute cap acts for no one: {r:?}"
    );
}

/// Part 2 by TOTP for `auth`'s own session, its absolute cap set `cap_secs`
/// from now (`expires_at` a day ahead).
async fn part2_with_cap(
    app: &TestApp,
    auth: &vc_server::auth::AuthUser,
    code: &str,
    cap_secs: f64,
) -> vc_server::auth::AuthResult<vc_server::auth::owner_rule::OwnerAuthority> {
    use fred::interfaces::KeysInterface;
    sqlx::query(
        "UPDATE sessions SET absolute_expires_at = NOW() + make_interval(secs => $2),
                expires_at = NOW() + INTERVAL '1 day' WHERE id = $1",
    )
    .bind(auth.session_id)
    .bind(cap_secs)
    .execute(&app.pool)
    .await
    .unwrap();
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{}", auth.id))
        .await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut tx, &[auth.id])
        .await
        .unwrap();
    vc_server::auth::owner_rule::authorize(
        &mut tx,
        &ctx,
        auth,
        &vc_server::auth::owner_rule::OwnerTarget {
            subject: auth.id,
            action: vc_server::auth::step_up::StepUpAction::RequestCancel,
            target: vc_server::auth::step_up::StepUpTarget::Id(Uuid::new_v4()),
            event_at: None,
            created_by_event: Vec::new(),
        },
        Some(vc_server::auth::owner_rule::Part2Factor::Totp(
            code.to_string(),
        )),
        || vc_server::auth::AuthError::CancelNotAllowed,
    )
    .await
}
