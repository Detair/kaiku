//! Gemini Flash full review of L1b (round 5): one test per real finding.
//!
//! Run with: `cargo test --test integration flash_review`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::ratelimit::account::CounterKey;

use super::account_requests::{admin_with_device, bootstrap_user, post, start_bootstrap, PASSWORD};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, password_user, peer_request};
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_session_with_token,
    create_test_user, grant_step_up, make_admin, wait_for_lock_waiters, TestApp,
};
use super::login_attempts::{attempt_id, pc_call, Secret};
use super::login_password::login_native;

/// The plaintext of the timing-equaliser hash (public in the source).
const DUMMY_PLAINTEXT: &str = "kaiku-login-timing-equalizer";

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
    body: Value,
) -> (u16, Value) {
    let mut req = peer_request(method, uri).header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn open_registration(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO server_config (key, value) VALUES ('registration_policy', '\"open\"')
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Register natively (not the first account): `(user_id, ticket)`.
async fn register(app: &TestApp, username: &str, password: Option<&str>) -> (Uuid, String) {
    open_registration(&app.pool).await;
    create_test_user(&app.pool).await;
    let mut body = json!({"username": username});
    if let Some(p) = password {
        body["password"] = json!(p);
    }
    let (s, b) = call(
        app,
        Method::POST,
        "/auth/register",
        &[("X-Kaiku-Client", "android")],
        body,
    )
    .await;
    assert_eq!(s, 201, "{b}");
    (
        b["user_id"].as_str().unwrap().parse().unwrap(),
        b["registration_ticket"].as_str().unwrap().to_string(),
    )
}

/// Issue an admin setup link for `target` from `adev`.
async fn issue_admin_link(app: &TestApp, adev: &KeyDeviceFixture, target: Uuid) -> (u16, Value) {
    grant_step_up(
        &app.pool,
        adev.session_id.unwrap(),
        "admin.enrollment.issue",
        &target.to_string(),
    )
    .await;
    call(
        app,
        Method::POST,
        &format!("/api/admin/users/{target}/enrollments"),
        &[("Authorization", &bearer(&adev.access_token))],
        json!({"mode": "setup", "claimant_check": {"method": "in_person", "note": "at home"}}),
    )
    .await
}

async fn wait_for_state(pool: &PgPool, user: Uuid, column: &str, want: &str) -> String {
    let sql = match column {
        "totp_state" => "SELECT totp_state FROM users WHERE id = $1",
        _ => "SELECT password_state FROM users WHERE id = $1",
    };
    let mut got = String::new();
    for _ in 0..60 {
        got = sqlx::query_scalar(sql)
            .bind(user)
            .fetch_one(pool)
            .await
            .unwrap();
        if got == want {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    got
}

async fn set_count(app: &TestApp, key: &CounterKey, n: i64) {
    use fred::interfaces::{HashesInterface, KeysInterface};
    let k = key.redis_key();
    let _: () = app.state.redis.del(&k).await.unwrap();
    let _: () = app.state.redis.hset(&k, ("count", n)).await.unwrap();
}

async fn current_totp_key(pool: &PgPool, user: Uuid) -> CounterKey {
    let g: i64 = sqlx::query_scalar("SELECT totp_generation FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap();
    CounterKey::totp(user, g)
}

/// Part 1 B1: an account registered with a password (ticket still unbound,
/// no `first_activity_at` yet) is existing (B.0): an admin setup needs an
/// independent canceller.
#[sqlx::test]
async fn a_registered_password_account_is_existing_for_an_admin_setup(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (target, _) = register(&app, "flash_pw_reg", Some(PASSWORD)).await;
    let (a, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, a, "active").await;
    let (s, b) = issue_admin_link(&app, &adev, target).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("NO_INDEPENDENT_CANCELLER")),
        "{b}"
    );
}

/// Part 4: the timing-equaliser hash never verifies a passwordless
/// account's "password" (recovery second factor and bootstrap first factor).
#[sqlx::test]
async fn the_dummy_hash_is_no_password(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    // Recovery: a valid code plus the dummy plaintext.
    let (user, name) = password_user(&pool, PASSWORD).await;
    sqlx::query("UPDATE users SET password_hash = NULL WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (s, b) = post(
        &app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "recovery", "recovery_code": codes[0],
               "second": {"type": "password", "password": DUMMY_PLAINTEXT}, "lost": "none"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("REQUEST_FACTOR_INVALID")),
        "{b}"
    );
    // Bootstrap: the dummy plaintext is no first factor, so nothing counts.
    let (user, name, totp) = bootstrap_user(&app).await;
    sqlx::query("UPDATE users SET password_hash = NULL WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, _) = post(
        &app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "bootstrap",
               "first": {"type": "password", "password": DUMMY_PLAINTEXT},
               "totp_code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 401);
    let starts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_request_starts WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(starts, 0, "no start counted before a valid first factor");
}

/// Part 4: a correct bootstrap password is a password success (B.5: the
/// counter resets).
#[sqlx::test]
async fn a_correct_bootstrap_password_resets_its_counter(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, name, _) = bootstrap_user(&app).await;
    let key = CounterKey::password(&name);
    set_count(&app, &key, 3).await;
    let (s, _) = post(
        &app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "bootstrap",
               "first": {"type": "password", "password": PASSWORD}, "totp_code": "000000"}),
    )
    .await;
    assert_eq!(s, 401);
    let (count, _) = vc_server::ratelimit::account::AccountLimiter::new(&app.state.redis)
        .read(&key)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

/// Part 5 #2: the TOTP step authenticates the collection secret before it
/// consults the account's counter (C27: wrong secret → 404).
#[sqlx::test]
async fn a_wrong_collection_secret_is_unknown_while_totp_waits(pool: PgPool) {
    use fred::interfaces::HashesInterface;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (s, step, secret) = login_native(&app, &name, PASSWORD).await;
    assert_eq!(s, 202, "{step}");
    let id = attempt_id(&step["attempt"]);
    let key = current_totp_key(&pool, user).await;
    set_count(&app, &key, 9).await;
    let next = chrono::Utc::now().timestamp() + 600;
    let _: () = app
        .state
        .redis
        .hset(key.redis_key(), ("next", next))
        .await
        .unwrap();
    let uri = format!("/auth/login-attempts/{id}/totp");
    let wrong = Secret::Verifier("not-the-verifier-of-this-attempt-000000000".into());
    let (s, b, _) = pc_call(&app, &uri, &wrong, json!({"code": "000000"})).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("LOGIN_ATTEMPT_UNKNOWN")),
        "{b}"
    );
    let (s, _, _) = pc_call(&app, &uri, &secret, json!({"code": "000000"})).await;
    assert_eq!(s, 429, "the control: the right secret meets the delay");
}

/// Part 1 #2: an admin demoted while the setup link waited for the account
/// locks issues nothing (Codex M8 pattern).
#[sqlx::test]
async fn a_demoted_admin_issues_no_setup_link(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (target, _) = create_test_user(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, a, "active").await;
    let mut hold = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut hold, &[a])
        .await
        .unwrap();
    sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(a)
        .execute(&mut *hold)
        .await
        .unwrap();
    let ((s, b), ()) = tokio::join!(issue_admin_link(&app, &adev, target), async {
        wait_for_lock_waiters(&pool, 1).await;
        hold.commit().await.unwrap();
    });
    assert_eq!((s, b["error"].as_str()), (403, Some("NOT_ADMIN")), "{b}");
}

/// Part 7 #5: the same for an admin naming a recovery contact.
#[sqlx::test]
async fn a_demoted_admin_names_no_recovery_contact(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (account, _) = create_test_user(&pool).await;
    let (contact, _) = create_test_user(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, a).await;
    grant_step_up(
        &pool,
        sid,
        "admin.recovery_contact.set",
        &account.to_string(),
    )
    .await;
    let mut hold = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut hold, &[a])
        .await
        .unwrap();
    sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(a)
        .execute(&mut *hold)
        .await
        .unwrap();
    let uri = format!("/api/admin/users/{account}/recovery-contact");
    let auth = bearer(&token);
    let hdrs = [("Authorization", auth.as_str())];
    let ((s, b), ()) = tokio::join!(
        call(&app, Method::PUT, &uri, &hdrs, json!({"user_id": contact}),),
        async {
            wait_for_lock_waiters(&pool, 1).await;
            hold.commit().await.unwrap();
        }
    );
    assert_eq!((s, b["error"].as_str()), (403, Some("NOT_ADMIN")), "{b}");
}

/// Part 2 #1: cancelling an admin setup link tells its issuer (C26).
#[sqlx::test]
async fn cancelling_an_admin_link_tells_its_issuer(pool: PgPool) {
    use fred::interfaces::{ClientLike, EventInterface, PubsubInterface};
    let app = TestApp::with_pool(pool.clone()).await;
    let (target, _) = create_test_user(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, a, "active").await;
    // A second admin with a key: the canceller route, and who cancels.
    let (_, bdev) = admin_with_device(&app).await;
    let (s, issued) = issue_admin_link(&app, &adev, target).await;
    assert_eq!(s, 201, "{issued}");
    let id = issued["enrollment_id"].as_str().unwrap().to_string();
    let subscriber = app.state.redis.clone_new();
    let _ = subscriber.connect();
    subscriber.wait_for_connect().await.unwrap();
    let () = subscriber
        .subscribe(vc_server::ws::channels::user_events(a))
        .await
        .unwrap();
    let mut rx = subscriber.message_rx();
    let (s, b) = call(
        &app,
        Method::DELETE,
        &format!("/auth/enrollments/{id}"),
        &[("Authorization", &bearer(&bdev.access_token))],
        json!({}),
    )
    .await;
    assert_eq!(s, 204, "{b}");
    let hint = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let msg = rx.recv().await.unwrap();
            let v: Value = serde_json::from_str(&msg.value.convert::<String>().unwrap()).unwrap();
            if v["type"] == "enrollment_updated" {
                return v;
            }
        }
    })
    .await
    .expect("an enrollment_updated hint for the issuing admin");
    assert_eq!(
        (hint["enrollment_id"].as_str(), hint["state"].as_str()),
        (Some(id.as_str()), Some("cancelled"))
    );
}

/// Part 3 #1: staging a password is refused before any work while the
/// request is not `ready` (409, not a policy answer).
#[sqlx::test]
async fn a_waiting_request_stages_no_password(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let secret = b["request_secret"].as_str().unwrap().to_string();
    let (s, b) = call(
        &app,
        Method::PUT,
        "/auth/account-requests/current/password",
        &[("x-kaiku-request-secret", &secret)],
        json!({"password": "short"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("ACCOUNT_REQUEST_NOT_READY")),
        "{b}"
    );
}

/// Part 7 #1 (hardening): a contact or an admin acts for an owner only to
/// cancel a request (B.2), never for "That wasn't me".
#[sqlx::test]
async fn an_admin_is_no_owner_for_a_repudiation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (subject, _) = create_test_user(&pool).await;
    let (a, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    let (sid, _) = create_session_with_token(&pool, &app.config, a).await;
    let event = Uuid::new_v4();
    grant_step_up(&pool, sid, "event.repudiate", &event.to_string()).await;
    let auth = super::helpers::login::auth_user(&app, sid).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut tx, &[subject, a])
        .await
        .unwrap();
    let r = vc_server::auth::owner_rule::authorize(
        &mut tx,
        &ctx,
        &auth,
        &vc_server::auth::owner_rule::OwnerTarget {
            subject,
            action: vc_server::auth::step_up::StepUpAction::EventRepudiate,
            target: vc_server::auth::step_up::StepUpTarget::Id(event),
            event_at: None,
            created_by_event: Vec::new(),
        },
        None,
        || vc_server::auth::AuthError::RepudiateNotAllowed,
    )
    .await;
    assert!(
        matches!(r, Err(vc_server::auth::AuthError::RepudiateNotAllowed)),
        "{r:?}"
    );
}

/// Part 6 #3 / part 8 M-01: a 100th failure whose effects never ran (or a
/// count past it) still throttles at the next failure, once per epoch.
#[sqlx::test]
async fn throttling_applies_past_a_missed_hundredth(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    // TOTP.
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    set_count(&app, &current_totp_key(&pool, user).await, 100).await;
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": "000000"}),
    )
    .await;
    assert_eq!(s, 401, "{b}");
    assert_eq!(
        wait_for_state(&pool, user, "totp_state", "throttled").await,
        "throttled"
    );
    // Password.
    let (user, name) = password_user(&pool, PASSWORD).await;
    set_count(&app, &CounterKey::password(&name), 100).await;
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/login",
        &[],
        json!({"username": name, "password": "not-the-password"}),
    )
    .await;
    assert_eq!(s, 401, "{b}");
    assert_eq!(
        wait_for_state(&pool, user, "password_state", "throttled").await,
        "throttled"
    );
}

/// Part 8 M-02: a setup link from a ticket that bound meanwhile is refused
/// (the account lock, then the ticket re-checked).
#[sqlx::test]
async fn a_ticket_bound_meanwhile_issues_no_setup_link(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, ticket) = register(&app, "flash_bound_link", None).await;
    let mut hold = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut hold, &[user])
        .await
        .unwrap();
    sqlx::query("UPDATE registration_tickets SET bound_at = NOW() WHERE user_id = $1")
        .bind(user)
        .execute(&mut *hold)
        .await
        .unwrap();
    let hdrs = [("X-Kaiku-Ticket", ticket.as_str())];
    let ((s, b), ()) = tokio::join!(
        call(
            &app,
            Method::POST,
            "/auth/registration/enrollments",
            &hdrs,
            json!({}),
        ),
        async {
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                wait_for_lock_waiters(&pool, 1),
            )
            .await;
            hold.commit().await.unwrap();
        }
    );
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("REGISTRATION_TICKET_INVALID")),
        "{b}"
    );
}

/// Part 8 M-03: a ticket that expires while its TOTP verify waits for the
/// account lock binds nothing.
#[sqlx::test]
async fn a_ticket_expired_meanwhile_binds_no_totp(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, ticket) = register(&app, "flash_expiring", Some(PASSWORD)).await;
    let (s, setup) = call(
        &app,
        Method::POST,
        "/auth/registration/totp",
        &[("X-Kaiku-Ticket", &ticket)],
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{setup}");
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(setup["secret"].as_str().unwrap().to_string())
            .to_bytes()
            .unwrap(),
        Some("Kaiku".into()),
        "flash_expiring".into(),
    )
    .unwrap();
    let mut hold = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut hold, &[user])
        .await
        .unwrap();
    sqlx::query(
        "UPDATE registration_tickets SET expires_at = NOW() - INTERVAL '1 second' WHERE user_id = $1",
    )
    .bind(user)
    .execute(&mut *hold)
    .await
    .unwrap();
    let hdrs = [("X-Kaiku-Ticket", ticket.as_str())];
    let ((s, b), ()) = tokio::join!(
        call(
            &app,
            Method::POST,
            "/auth/registration/totp/verify",
            &hdrs,
            json!({"code": totp.generate_current().unwrap()}),
        ),
        async {
            wait_for_lock_waiters(&pool, 1).await;
            hold.commit().await.unwrap();
        }
    );
    assert_eq!(
        (s, b["error"].as_str()),
        (404, Some("REGISTRATION_TICKET_INVALID")),
        "{b}"
    );
}

/// Part 8 m-01: a passwordless registration cannot bind TOTP alone (F8: the
/// account would then sign in by F4, which needs a first factor).
#[sqlx::test]
async fn a_passwordless_registration_binds_no_totp(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, ticket) = register(&app, "flash_no_pw", None).await;
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/registration/totp",
        &[("X-Kaiku-Ticket", &ticket)],
        json!({}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("TOTP_UNAVAILABLE")),
        "{b}"
    );
}

/// Part 9 #1: an open request's events offer Cancel to the owner (C28).
#[sqlx::test]
async fn a_request_event_offers_cancel(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let (_, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, log) = call(
        &app,
        Method::GET,
        "/api/me/security-events",
        &[("Authorization", &bearer(&token))],
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{log}");
    let started = log["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["event_type"] == "request_started")
        .expect("a request_started event");
    assert!(
        started["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["action"] == "cancel_request" && a["via"] == "part2"),
        "{started}"
    );
}

/// Part 9 #4: `throttle reset` holds its transaction's connection and needs
/// no second one (a one-connection pool does not hang).
#[sqlx::test]
async fn throttle_reset_runs_on_one_connection(pool: PgPool) {
    use vc_server::cli::{run, CliCtx, CliIo, Command};
    struct Quiet;
    impl CliIo for Quiet {
        fn print(&mut self, _s: &str) {}
        async fn read_line(&mut self, _p: &str) -> std::io::Result<String> {
            Err(std::io::ErrorKind::UnexpectedEof.into())
        }
    }
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, name) = create_test_user(&pool).await;
    let one = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(30))
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let ctx = CliCtx {
        db: one,
        redis: app.state.redis.clone(),
        config: (*app.config).clone(),
    };
    let args: Vec<String> = ["throttle", "reset", name.as_str()]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let done = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run(&ctx, Command::parse(&args).unwrap(), &mut Quiet),
    )
    .await;
    assert!(
        done.is_ok_and(|r| r.is_ok()),
        "throttle reset hung or failed"
    );
}

/// Part 7 #8: a password change tells the account's sockets (C26
/// `authenticators_changed`).
#[sqlx::test]
async fn a_password_change_hints_authenticators_changed(pool: PgPool) {
    use fred::interfaces::{ClientLike, EventInterface, PubsubInterface};
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    grant_step_up(&pool, sid, "password.set", "").await;
    let subscriber = app.state.redis.clone_new();
    let _ = subscriber.connect();
    subscriber.wait_for_connect().await.unwrap();
    let () = subscriber
        .subscribe(vc_server::ws::channels::user_events(user))
        .await
        .unwrap();
    let mut rx = subscriber.message_rx();
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/me/password",
        &[("Authorization", &bearer(&token))],
        json!({"new_password": "a perfectly fine passphrase 7"}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let msg = rx.recv().await.unwrap();
            let v: Value = serde_json::from_str(&msg.value.convert::<String>().unwrap()).unwrap();
            if v["type"] == "authenticators_changed" {
                return;
            }
        }
    })
    .await
    .expect("an authenticators_changed hint");
}

/// Part 7 #7: creating recovery codes records the consumed grant's
/// `step_up_action` (C28), as every other step-up action does.
#[sqlx::test]
async fn new_recovery_codes_record_the_step_up_action(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    grant_step_up(&pool, sid, "recovery_codes.create", "").await;
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/recovery-codes",
        &[("Authorization", &bearer(&token))],
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'step_up_action'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
