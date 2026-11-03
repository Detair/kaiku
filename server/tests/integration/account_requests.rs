//! Account requests (SM3), bootstrap and Cancel (C27, B.2 owner rule).
//!
//! Run with: `cargo test --test integration account_requests`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::step_up::StepUpAction;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{device_grant, enable_totp, password_user, peer_request};
use super::helpers::{body_to_json_lenient, create_test_user, make_admin, TestApp};

pub(super) const PASSWORD: &str = "zq8#vT4p-request";

pub(super) async fn post(
    app: &TestApp,
    uri: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: Value,
) -> (u16, Value) {
    let mut req = peer_request(Method::POST, uri).header("Content-Type", "application/json");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// An account with a password, TOTP and `bootstrap_open`.
pub(super) async fn bootstrap_user(app: &TestApp) -> (Uuid, String, totp_rs::TOTP) {
    let (user, name) = password_user(&app.pool, PASSWORD).await;
    let totp = enable_totp(&app.pool, &app.config, user, &name).await;
    sqlx::query("UPDATE users SET bootstrap_open = true WHERE id = $1")
        .bind(user)
        .execute(&app.pool)
        .await
        .unwrap();
    (user, name, totp)
}

/// An admin with an active key device (a canceller route).
pub(super) async fn admin_with_device(app: &TestApp) -> (Uuid, KeyDeviceFixture) {
    let (admin, _) = create_test_user(&app.pool).await;
    make_admin(&app.pool, admin).await;
    let dev = KeyDeviceFixture::insert(&app.pool, &app.config, admin, "active").await;
    (admin, dev)
}

/// Start a bootstrap request (native); clears the TOTP replay entry.
pub(super) async fn start_bootstrap(
    app: &TestApp,
    user: Uuid,
    name: &str,
    totp: &totp_rs::TOTP,
) -> (u16, Value) {
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
    post(
        app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({
            "username": name,
            "purpose": "bootstrap",
            "first": {"type": "password", "password": PASSWORD},
            "totp_code": totp.generate_current().unwrap(),
        }),
    )
    .await
}

async fn state_of(pool: &PgPool, id: &Value) -> String {
    sqlx::query_scalar("SELECT state FROM account_requests WHERE id = $1::uuid")
        .bind(id.as_str().unwrap())
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn events(pool: &PgPool, user: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT event_type FROM security_events WHERE subject_id = $1 ORDER BY created_at, id",
    )
    .bind(user)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn bootstrap_start_needs_both_factors_active(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;

    // A wrong TOTP code: the neutral 401.
    let (s, b) = post(
        &app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "bootstrap",
               "first": {"type": "password", "password": PASSWORD}, "totp_code": "000000"}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_CREDENTIALS")));
    // A compromised password: the same.
    sqlx::query("UPDATE users SET password_state = 'compromised' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_CREDENTIALS")));
    sqlx::query("UPDATE users SET password_state = 'active' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(b["request"]["state"], "waiting");
    assert_eq!(b["request"]["purpose"], "bootstrap");
    assert_eq!(b["request_secret"].as_str().unwrap().len(), 43);
    let ready: chrono::DateTime<chrono::Utc> =
        b["request"]["ready_at"].as_str().unwrap().parse().unwrap();
    let wait = ready - chrono::Utc::now();
    assert!(wait > chrono::Duration::hours(23) && wait <= chrono::Duration::hours(24));
    assert!(events(&pool, user)
        .await
        .contains(&"request_started".to_string()));
}

#[sqlx::test]
async fn bootstrap_needs_the_entitlement(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    sqlx::query("UPDATE users SET bootstrap_open = false WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_CREDENTIALS")));
}

#[sqlx::test]
async fn no_route_means_no_start(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    // Nobody could cancel.
    let (user, name, totp) = bootstrap_user(&app).await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("NO_INDEPENDENT_CANCELLER"))
    );
    // The sole admin's own account: the admin is no route for itself.
    make_admin(&pool, user).await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("NO_INDEPENDENT_CANCELLER"))
    );
    // A non-admin's request counts the sole admin.
    let (other, oname, ototp) = bootstrap_user(&app).await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = start_bootstrap(&app, other, &oname, &ototp).await;
    assert_eq!(s, 201, "{b}");
}

#[sqlx::test]
async fn young_contact_is_notified_but_not_a_route(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (contact, _) = create_test_user(&pool).await;
    KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    sqlx::query(
        "UPDATE users SET recovery_contact_id = $2, recovery_contact_named_at = NOW(),
                recovery_contact_confirmed_at = NOW() WHERE id = $1",
    )
    .bind(user)
    .bind(contact)
    .execute(&pool)
    .await
    .unwrap();
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    let cancellers: Value =
        sqlx::query_scalar("SELECT cancellers FROM account_requests WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cancellers, json!([{"route": "admin", "id": admin}]));
    // The young contact still gets the high notice (delivered after commit).
    let mut got = 0i64;
    for _ in 0..50 {
        vc_server::security_events::notices::deliver_pending(&app.state)
            .await
            .unwrap();
        got = sqlx::query_scalar(
            "SELECT COUNT(*) FROM security_notices
              WHERE recipient_id = $1 AND subject_id = $2 AND event_type = 'request_started'",
        )
        .bind(contact)
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
        if got > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(got, 1);
}

#[sqlx::test]
async fn one_open_request_per_account(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    assert_eq!(start_bootstrap(&app, user, &name, &totp).await.0, 201);
    let (s, b) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("ACCOUNT_REQUEST_OPEN"))
    );
}

#[sqlx::test]
async fn timers_ready_soon_ready_and_expiry(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].clone();
    let shift = |sql: &'static str| {
        let pool = pool.clone();
        let id = id.as_str().unwrap().to_string();
        async move {
            sqlx::query(sql).bind(id).execute(&pool).await.unwrap();
        }
    };
    shift(
        "UPDATE account_requests SET ready_at = NOW() + INTERVAL '30 minutes' WHERE id = $1::uuid",
    )
    .await;
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(state_of(&pool, &id).await, "waiting");
    shift("UPDATE account_requests SET ready_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid")
        .await;
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(state_of(&pool, &id).await, "ready");
    shift(
        "UPDATE account_requests SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1::uuid",
    )
    .await;
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(state_of(&pool, &id).await, "expired");
    let ev = events(&pool, user).await;
    for t in ["request_ready_soon", "request_ready", "request_expired"] {
        assert_eq!(ev.iter().filter(|e| *e == t).count(), 1, "{t}: {ev:?}");
    }
}

#[sqlx::test]
async fn cancel_by_admin_contact_and_owner_part1(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, adev) = admin_with_device(&app).await;

    // Admin.
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    let uri = format!("/api/account-requests/{id}/cancel");
    let (s, e) = post(&app, &uri, Some(&adev.access_token), &[], json!({})).await;
    assert_eq!((s, e["error"].as_str()), (403, Some("STEP_UP_REQUIRED")));
    device_grant(&app, &adev, StepUpAction::RequestCancel, &id).await;
    let (s, sum) = post(
        &app,
        &uri,
        Some(&adev.access_token),
        &[],
        json!({"reason": "not her"}),
    )
    .await;
    assert_eq!(
        (s, sum["state"].as_str()),
        (200, Some("cancelled")),
        "{sum}"
    );

    // The confirmed contact.
    let (user, name, totp) = bootstrap_user(&app).await;
    let (contact, _) = create_test_user(&pool).await;
    let cdev = KeyDeviceFixture::insert(&pool, &app.config, contact, "active").await;
    sqlx::query(
        "UPDATE users SET recovery_contact_id = $2, recovery_contact_named_at = NOW(),
                recovery_contact_confirmed_at = NOW() WHERE id = $1",
    )
    .bind(user)
    .bind(contact)
    .execute(&pool)
    .await
    .unwrap();
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    device_grant(&app, &cdev, StepUpAction::RequestCancel, &id).await;
    let (s, sum) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&cdev.access_token),
        &[],
        json!({}),
    )
    .await;
    assert_eq!(
        (s, sum["state"].as_str()),
        (200, Some("cancelled")),
        "{sum}"
    );

    // The owner, part 1 (its own key device).
    let (user, name, totp) = bootstrap_user(&app).await;
    let own = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    device_grant(&app, &own, StepUpAction::RequestCancel, &id).await;
    let (s, sum) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&own.access_token),
        &[],
        json!({}),
    )
    .await;
    assert_eq!(
        (s, sum["state"].as_str()),
        (200, Some("cancelled")),
        "{sum}"
    );

    // A stranger may not.
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    let (stranger, _) = create_test_user(&pool).await;
    let sdev = KeyDeviceFixture::insert(&pool, &app.config, stranger, "active").await;
    device_grant(&app, &sdev, StepUpAction::RequestCancel, &id).await;
    let (s, e) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&sdev.access_token),
        &[],
        json!({}),
    )
    .await;
    assert_eq!((s, e["error"].as_str()), (403, Some("CANCEL_NOT_ALLOWED")));
}

#[sqlx::test]
async fn self_cancel_compromises_only_the_used_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, adev) = admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    // TOTP was renewed since the start; the password was not.
    sqlx::query("UPDATE users SET totp_generation = totp_generation + 1 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    device_grant(&app, &adev, StepUpAction::RequestCancel, &id).await;
    let (s, _) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&adev.access_token),
        &[],
        json!({}),
    )
    .await;
    assert_eq!(s, 200);
    let (pw, tp): (String, String) =
        sqlx::query_as("SELECT password_state, totp_state FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((pw.as_str(), tp.as_str()), ("compromised", "active"));
}

#[sqlx::test]
async fn part2_needs_no_strong_authenticator_and_a_fresh_factor(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    let (_, probation) =
        super::helpers::create_probation_session_with_token(&pool, &app.config, user).await;
    let uri = format!("/api/account-requests/{id}/cancel");
    // The code used for the start is a replay.
    let (s, e) = post(
        &app,
        &uri,
        Some(&probation),
        &[],
        json!({"part2": {"factor": {"type": "totp", "code": totp.generate_current().unwrap()}}}),
    )
    .await;
    assert_eq!((s, e["error"].as_str()), (401, Some("INVALID_MFA")), "{e}");
    // A recovery code works (and is consumed).
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (s, sum) = post(
        &app,
        &uri,
        Some(&probation),
        &[],
        json!({"part2": {"factor": {"type": "recovery_code", "code": codes[0]}}}),
    )
    .await;
    assert_eq!(
        (s, sum["state"].as_str()),
        (200, Some("cancelled")),
        "{sum}"
    );
    assert_eq!(
        vc_server::auth::recovery_codes::left(&pool, user)
            .await
            .unwrap(),
        9
    );
    // With a strong authenticator, part 2 is refused.
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let id = b["request"]["id"].as_str().unwrap().to_string();
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, probation) =
        super::helpers::create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, e) = post(
        &app,
        &format!("/api/account-requests/{id}/cancel"),
        Some(&probation),
        &[],
        json!({"part2": {"factor": {"type": "recovery_code", "code": "AAAAA-AAAAA-AAA"}}}),
    )
    .await;
    assert_eq!((s, e["error"].as_str()), (403, Some("CANCEL_NOT_ALLOWED")));
}

#[sqlx::test]
async fn starting_client_can_only_withdraw(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let secret = b["request_secret"].as_str().unwrap().to_string();
    let req = peer_request(Method::DELETE, "/auth/account-requests/current")
        .header("X-Kaiku-Request-Secret", &secret)
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status().as_u16(), 204);
    assert_eq!(state_of(&pool, &b["request"]["id"]).await, "withdrawn");
    let (pw,): (String,) = sqlx::query_as("SELECT password_state FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pw, "active", "withdraw has no effects");
    let ev = events(&pool, user).await;
    let started = ev.iter().position(|e| e == "request_started").unwrap();
    let withdrawn = ev.iter().position(|e| e == "request_withdrawn").unwrap();
    assert!(started < withdrawn);
}

#[sqlx::test]
async fn generation_change_voids(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::auth::factors::replace_tx(
        &mut tx,
        &ctx,
        user,
        vc_server::auth::factors::Factor::Password,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state_of(&pool, &b["request"]["id"]).await, "void");
    assert!(events(&pool, user)
        .await
        .contains(&"request_void".to_string()));
}

#[sqlx::test]
async fn last_route_gone_voids(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, adev) = admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    // The only route (the admin's only key) is revoked.
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::devices::revoke_tx(
        &mut tx,
        &ctx,
        adev.row_id,
        vc_server::approvals::devices::RevokeReason::Owner,
        vc_server::security_events::Actor::User(adev.user_id),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state_of(&pool, &b["request"]["id"]).await, "void");
}

#[sqlx::test]
async fn bootstrap_entitlement_consumed_by_cli_device_voids_request(pool: PgPool) {
    use vc_server::approvals::enrollments::{issue_tx, EnrollmentVariant, IssueParams, Issuer};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, b) = start_bootstrap(&app, user, &name, &totp).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    let issued = issue_tx(
        &mut tx,
        &ctx,
        IssueParams {
            variant: EnrollmentVariant::Cli,
            subject_id: user,
            issuer: Issuer::Cli,
            claimant: vc_server::approvals::context::ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let phone = super::enrollments::Phone::new();
    let (s, claimed) = super::enrollments::claim_with(
        &app,
        &issued.setup_token,
        &phone,
        "Pixel 8",
        super::enrollments::good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::enrollments::confirm_tx(
        &mut tx,
        &ctx,
        issued.enrollment_id,
        vc_server::approvals::enrollments::Confirmer::Cli,
        claimed["code"].as_str().unwrap().as_bytes(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(state_of(&pool, &b["request"]["id"]).await, "void");
    let open: bool = sqlx::query_scalar("SELECT bootstrap_open FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!open);
}

#[sqlx::test]
async fn concurrent_starts_one_open(pool: PgPool) {
    let app = std::sync::Arc::new(TestApp::with_pool(pool.clone()).await);
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    // One secret and one code for all: the account lock serialises the
    // starts; the replay store lets only one code through.
    let code = totp.generate_current().unwrap();
    let mut tasks = Vec::new();
    for _ in 0..10 {
        let app = app.clone();
        let name = name.clone();
        let code = code.clone();
        tasks.push(tokio::spawn(async move {
            post(
                &app,
                "/auth/account-requests",
                None,
                &[("X-Kaiku-Client", "android")],
                json!({"username": name, "purpose": "bootstrap",
                       "first": {"type": "password", "password": PASSWORD}, "totp_code": code}),
            )
            .await
        }));
    }
    let mut created = 0;
    for t in tasks {
        let (s, b) = t.await.unwrap();
        match s {
            201 => created += 1,
            401 | 409 | 429 => {}
            _ => panic!("unexpected {s}: {b}"),
        }
    }
    assert_eq!(created, 1);
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_requests WHERE user_id = $1 AND state IN ('waiting', 'ready')",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open, 1);
}
