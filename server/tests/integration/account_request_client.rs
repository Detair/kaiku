//! The request client (C27 "Request client"): staging while `ready`, Finish.
//!
//! Run with: `cargo test --test integration account_request_client`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap, PASSWORD};
use super::enrollments::{claim_own, claim_with, good, Phone};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{peer_request, TEST_ORIGIN};
use super::helpers::passkeys::SoftAuthenticator;
use super::helpers::{body_to_json_lenient, create_test_user, insert_session_row, TestApp};

const HEADER: &str = "X-Kaiku-Request-Secret";

/// A started bootstrap request.
struct Req {
    user: Uuid,
    id: Uuid,
    secret: String,
}

async fn started(app: &TestApp) -> Req {
    admin_with_device(app).await;
    let (user, name, totp) = bootstrap_user(app).await;
    let (s, b) = start_bootstrap(app, user, &name, &totp).await;
    assert_eq!(s, 201, "{b}");
    Req {
        user,
        id: b["request"]["id"].as_str().unwrap().parse().unwrap(),
        secret: b["request_secret"].as_str().unwrap().to_string(),
    }
}

/// A bootstrap request made `ready` by the minute sweep.
async fn ready(app: &TestApp) -> Req {
    let r = started(app).await;
    sqlx::query("UPDATE account_requests SET ready_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(r.id)
        .execute(&app.pool)
        .await
        .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(state_of(&app.pool, r.id).await, "ready");
    r
}

async fn rc(
    app: &TestApp,
    method: Method,
    uri: &str,
    secret: Option<&str>,
    body: Value,
) -> (u16, Value) {
    let mut req = peer_request(method, uri).header("Content-Type", "application/json");
    if let Some(s) = secret {
        req = req.header(HEADER, s);
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn state_of(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM account_requests WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn current(app: &TestApp, r: &Req) -> Value {
    let (s, b) = rc(
        app,
        Method::GET,
        "/auth/account-requests/current",
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    b
}

/// Issue a setup link by the request and claim it with the app's own key.
async fn own_phone(app: &TestApp, r: &Req) -> (Phone, Value) {
    let (s, issued) = rc(
        app,
        Method::POST,
        "/auth/account-requests/current/enrollments",
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let phone = Phone::new();
    let (s, b) = claim_own(
        app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        (HEADER, &r.secret),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    (phone, b)
}

async fn generations(pool: &PgPool, user: Uuid) -> (i64, i64, String, String) {
    sqlx::query_as(
        "SELECT password_generation, totp_generation, password_state, totp_state
           FROM users WHERE id = $1",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn current_needs_the_request_secret(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    let garbage = "A".repeat(43);
    for secret in [None, Some(garbage.as_str())] {
        for (m, uri, body) in [
            (Method::GET, "/auth/account-requests/current", json!({})),
            (
                Method::PUT,
                "/auth/account-requests/current/password",
                json!({"password": "a perfectly fine passphrase 9"}),
            ),
            (
                Method::POST,
                "/auth/account-requests/current/totp",
                json!({}),
            ),
            (
                Method::POST,
                "/auth/account-requests/current/enrollments",
                json!({}),
            ),
            (
                Method::POST,
                "/auth/account-requests/current/finish",
                json!({}),
            ),
        ] {
            let (s, b) = rc(&app, m, uri, secret, body).await;
            assert_eq!(s, 404, "{uri} {secret:?}: {b}");
        }
    }
    assert_eq!(current(&app, &r).await["state"], "ready");
}

#[sqlx::test]
async fn staging_only_while_ready(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = started(&app).await;
    for (m, uri, body) in [
        (
            Method::PUT,
            "/auth/account-requests/current/password",
            json!({"password": "a perfectly fine passphrase 9"}),
        ),
        (
            Method::POST,
            "/auth/account-requests/current/totp",
            json!({}),
        ),
        (
            Method::POST,
            "/auth/account-requests/current/enrollments",
            json!({}),
        ),
        (
            Method::PUT,
            "/auth/account-requests/current/recovery-contact",
            json!({"username": "nobody"}),
        ),
        (
            Method::POST,
            "/auth/account-requests/current/finish",
            json!({}),
        ),
    ] {
        let (s, b) = rc(&app, m, uri, Some(&r.secret), body).await;
        assert_eq!(
            (s, b["error"].as_str(), b["state"].as_str()),
            (409, Some("ACCOUNT_REQUEST_NOT_READY"), Some("waiting")),
            "{uri}: {b}"
        );
    }
    let (s, _) = rc(
        &app,
        Method::DELETE,
        "/auth/account-requests/current",
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!(s, 204);
    let (s, b) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/finish",
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!((s, b["state"].as_str()), (409, Some("withdrawn")), "{b}");
}

#[sqlx::test]
async fn stage_a_phone_by_setup_link(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    let (_, first) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/enrollments",
        Some(&r.secret),
        json!({}),
    )
    .await;
    // A new setup link voids the old one (one live child).
    let (s, issued) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/enrollments",
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let old: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1::uuid")
        .bind(first["enrollment_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(old, "cancelled");
    let phone = Phone::new();
    let (s, claimed) = claim_with(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        "Pixel 8",
        good(),
        "per_use",
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    let id = issued["enrollment_id"].as_str().unwrap();
    let (s, got) = rc(
        &app,
        Method::GET,
        &format!("/auth/account-requests/current/enrollments/{id}"),
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!((s, got["state"].as_str()), (200, Some("claimed")), "{got}");
    let (s, done) = rc(
        &app,
        Method::POST,
        &format!("/auth/account-requests/current/enrollments/{id}/confirm"),
        Some(&r.secret),
        json!({"code": claimed["code"]}),
    )
    .await;
    assert_eq!((s, done["state"].as_str()), (200, Some("done")), "{done}");
    let state: String = sqlx::query_scalar("SELECT state FROM approval_devices WHERE user_id = $1")
        .bind(r.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "pending");
    let cur = current(&app, &r).await;
    assert_eq!(cur["staged"]["authenticator"]["type"], "key_device");
    assert_eq!(cur["staged"]["authenticator"]["label"], "Pixel 8");
}

#[sqlx::test]
async fn android_app_enrols_itself_with_the_request_secret(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    let (_, b) = own_phone(&app, &r).await;
    assert_eq!(b["state"], "done");
    assert_eq!(b["authenticator_state"], "pending");
    assert!(b.get("code").is_none(), "{b}");
    let ready_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT ready_at FROM account_requests WHERE id = $1")
            .bind(r.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let got: chrono::DateTime<chrono::Utc> = b["ready_at"].as_str().unwrap().parse().unwrap();
    assert_eq!(got.timestamp(), ready_at.timestamp());
    // Another request's secret is no issuer proof.
    let other = Phone::new();
    let (_, issued) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/enrollments",
        Some(&r.secret),
        json!({}),
    )
    .await;
    let wrong = vc_server::approvals::secrets::b64u_encode(&[7u8; 32]);
    let (s, _) = claim_own(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &other,
        (HEADER, &wrong),
    )
    .await;
    assert_ne!(s, 201);
}

#[sqlx::test]
async fn stage_a_passkey_pending(pool: PgPool) {
    let app = TestApp::with_passkeys(pool.clone()).await;
    let r = ready(&app).await;
    let (s, start) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/passkey/start",
        Some(&r.secret),
        json!({"label": "Laptop"}),
    )
    .await;
    assert_eq!(s, 200, "{start}");
    let mut soft = SoftAuthenticator::new();
    let credential = soft.register(TEST_ORIGIN, &start["options"]);
    let (s, pk) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/passkey/finish",
        Some(&r.secret),
        json!({"ceremony_id": start["ceremony_id"], "credential": credential}),
    )
    .await;
    assert_eq!(s, 201, "{pk}");
    assert_eq!(pk["state"], "pending");
    let cur = current(&app, &r).await;
    assert_eq!(cur["staged"]["authenticator"]["type"], "passkey");
    assert_eq!(cur["staged"]["authenticator"]["label"], "Laptop");
    // One strong authenticator: a phone afterwards replaces the passkey.
    own_phone(&app, &r).await;
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1")
        .bind(r.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(
        current(&app, &r).await["staged"]["authenticator"]["type"],
        "key_device"
    );
}

#[sqlx::test]
async fn stage_password_totp_and_contact(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    let before: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(r.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (s, b) = rc(
        &app,
        Method::PUT,
        "/auth/account-requests/current/password",
        Some(&r.secret),
        json!({"password": "password1"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("PASSWORD_REJECTED")),
        "{b}"
    );
    let (s, b) = rc(
        &app,
        Method::PUT,
        "/auth/account-requests/current/password",
        Some(&r.secret),
        json!({"password": "a perfectly fine passphrase 9"}),
    )
    .await;
    assert_eq!(
        (s, b["staged"]["password"].as_bool()),
        (200, Some(true)),
        "{b}"
    );

    let (s, setup) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/totp",
        Some(&r.secret),
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{setup}");
    let (s, b) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/totp/verify",
        Some(&r.secret),
        json!({"code": "000000"}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_MFA")), "{b}");
    let totp = totp_of(setup["secret"].as_str().unwrap());
    forget_totp_step(&app, r.user).await;
    let (s, b) = rc(
        &app,
        Method::POST,
        "/auth/account-requests/current/totp/verify",
        Some(&r.secret),
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!((s, b["staged"]["totp"].as_bool()), (200, Some(true)), "{b}");

    let (s, b) = rc(
        &app,
        Method::PUT,
        "/auth/account-requests/current/recovery-contact",
        Some(&r.secret),
        json!({"username": "no_such_user"}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("RECOVERY_CONTACT_INVALID")),
        "{b}"
    );
    let (contact, contact_name) = create_test_user(&pool).await;
    let (s, b) = rc(
        &app,
        Method::PUT,
        "/auth/account-requests/current/recovery-contact",
        Some(&r.secret),
        json!({"username": contact_name}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["staged"]["recovery_contact"], contact_name.as_str());
    // Nothing applies before Finish.
    let (hash, rc_id): (String, Option<Uuid>) =
        sqlx::query_as("SELECT password_hash, recovery_contact_id FROM users WHERE id = $1")
            .bind(r.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hash, before);
    assert_ne!(rc_id, Some(contact));
}

/// The bootstrap start used a TOTP code of the old secret in this step;
/// a real request client stages a day later.
async fn forget_totp_step(app: &TestApp, user: Uuid) {
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
}

fn totp_of(secret: &str) -> totp_rs::TOTP {
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(secret.to_string())
            .to_bytes()
            .unwrap(),
        Some("Kaiku".into()),
        "x".into(),
    )
    .unwrap()
}

/// Stage a phone (own key), a password, TOTP and a contact.
async fn stage_everything(app: &TestApp, r: &Req) -> (Phone, Uuid) {
    let (phone, _) = own_phone(app, r).await;
    let (s, _) = rc(
        app,
        Method::PUT,
        "/auth/account-requests/current/password",
        Some(&r.secret),
        json!({"password": "a perfectly fine passphrase 9"}),
    )
    .await;
    assert_eq!(s, 200);
    let (_, setup) = rc(
        app,
        Method::POST,
        "/auth/account-requests/current/totp",
        Some(&r.secret),
        json!({}),
    )
    .await;
    let totp = totp_of(setup["secret"].as_str().unwrap());
    forget_totp_step(app, r.user).await;
    let (s, _) = rc(
        app,
        Method::POST,
        "/auth/account-requests/current/totp/verify",
        Some(&r.secret),
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200);
    let (contact, contact_name) = create_test_user(&app.pool).await;
    let (s, _) = rc(
        app,
        Method::PUT,
        "/auth/account-requests/current/recovery-contact",
        Some(&r.secret),
        json!({"username": contact_name}),
    )
    .await;
    assert_eq!(s, 200);
    (phone, contact)
}

async fn finish(app: &TestApp, r: &Req) -> (u16, Value) {
    rc(
        app,
        Method::POST,
        "/auth/account-requests/current/finish",
        Some(&r.secret),
        json!({}),
    )
    .await
}

async fn live_sessions(pool: &PgPool, user: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar("SELECT id FROM sessions WHERE user_id = $1 AND ended_at IS NULL")
        .bind(user)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn finish_applies_everything_in_one_transaction(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    // A key device kept, one named lost, and a PC session.
    let kept = KeyDeviceFixture::insert(&pool, &app.config, r.user, "active").await;
    let lost = KeyDeviceFixture::insert(&pool, &app.config, r.user, "active").await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, r.user, false).await;
    sqlx::query("UPDATE account_requests SET lost_device_ids = ARRAY[$2] WHERE id = $1")
        .bind(r.id)
        .bind(lost.row_id)
        .execute(&pool)
        .await
        .unwrap();
    let (pw0, totp0, _, _) = generations(&pool, r.user).await;
    let (phone, contact) = stage_everything(&app, &r).await;

    let (s, b) = finish(&app, &r).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["request"]["state"], "completed");
    assert_eq!(b["recovery_codes"].as_array().unwrap().len(), 10);

    let staged: String =
        sqlx::query_scalar("SELECT state FROM approval_devices WHERE device_id = $1")
            .bind(phone.device_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(staged, "active");
    let lost_state: String = sqlx::query_scalar("SELECT state FROM approval_devices WHERE id = $1")
        .bind(lost.row_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(lost_state, "revoked");
    let (pw1, totp1, pws, totps) = generations(&pool, r.user).await;
    assert_eq!((pw1, totp1), (pw0 + 1, totp0 + 1));
    assert_eq!((pws.as_str(), totps.as_str()), ("active", "active"));
    let (ok,): (bool,) =
        sqlx::query_as("SELECT password_hash IS NOT NULL FROM users WHERE id = $1")
            .bind(r.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ok);
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(r.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(vc_server::auth::verify_password("a perfectly fine passphrase 9", &hash).unwrap());
    assert!(!vc_server::auth::verify_password(PASSWORD, &hash).unwrap());
    let (contact_id, bootstrap_open): (Option<Uuid>, bool) =
        sqlx::query_as("SELECT recovery_contact_id, bootstrap_open FROM users WHERE id = $1")
            .bind(r.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(contact_id, Some(contact));
    assert!(!bootstrap_open);
    let live = live_sessions(&pool, r.user).await;
    assert!(
        live.contains(&kept.session_id.unwrap()),
        "kept device stays"
    );
    assert!(
        !live.contains(&lost.session_id.unwrap()),
        "lost device ends"
    );
    assert!(!live.contains(&pc), "other sessions end");
    let codes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM recovery_codes rc JOIN users u ON u.id = rc.user_id
          WHERE rc.user_id = $1 AND rc.used_at IS NULL
            AND rc.generation = u.recovery_codes_generation",
    )
    .bind(r.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(codes, 10);
    let completed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'request_completed'",
    )
    .bind(r.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(completed, 1);
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM enrollments WHERE issuer_request_id = $1 AND state IN ('issued', 'claimed')",
    )
    .bind(r.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(open, 0);
}

#[sqlx::test]
async fn finish_rechecks_routes_and_generations(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    // A changed generation voids.
    let r = ready(&app).await;
    own_phone(&app, &r).await;
    sqlx::query("UPDATE users SET password_generation = password_generation + 1 WHERE id = $1")
        .bind(r.user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = finish(&app, &r).await;
    assert_eq!(
        (s, b["error"].as_str(), b["state"].as_str()),
        (409, Some("ACCOUNT_REQUEST_NOT_READY"), Some("void")),
        "{b}"
    );
    let pending: String =
        sqlx::query_scalar("SELECT state FROM approval_devices WHERE user_id = $1")
            .bind(r.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(pending, "active", "nothing applied");

    // No route left voids.
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    sqlx::query("UPDATE approval_devices SET state = 'revoked', revoked_at = NOW(), revoke_reason = 'owner' WHERE user_id <> $1")
        .bind(r.user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = finish(&app, &r).await;
    assert_eq!((s, b["state"].as_str()), (409, Some("void")), "{b}");
    assert_eq!(state_of(&pool, r.id).await, "void");
}

#[sqlx::test]
async fn finish_deadline_is_24h_after_ready(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    let expires: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT expires_at FROM account_requests WHERE id = $1")
            .bind(r.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let left = expires - chrono::Utc::now();
    assert!(
        left <= chrono::Duration::hours(24)
            && left > chrono::Duration::hours(24) - chrono::Duration::minutes(1),
        "{left}"
    );
    assert_eq!(
        current(&app, &r).await["expires_at"].as_str().is_some(),
        true
    );
    sqlx::query(
        "UPDATE account_requests SET expires_at = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(r.id)
    .execute(&pool)
    .await
    .unwrap();
    let (s, b) = finish(&app, &r).await;
    assert_eq!((s, b["state"].as_str()), (409, Some("expired")), "{b}");
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    assert_eq!(state_of(&pool, r.id).await, "expired");
}

#[sqlx::test]
async fn finish_mints_no_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    own_phone(&app, &r).await;
    let before = live_sessions(&pool, r.user).await.len();
    let (s, b) = finish(&app, &r).await;
    assert_eq!(s, 200, "{b}");
    assert!(
        b.get("access_token").is_none() && b.get("refresh_token").is_none(),
        "{b}"
    );
    assert_eq!(live_sessions(&pool, r.user).await.len(), before);
}

#[sqlx::test]
async fn finish_retry_does_not_reapply(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    own_phone(&app, &r).await;
    let (s, first) = finish(&app, &r).await;
    assert_eq!(s, 200, "{first}");
    let generation: i64 =
        sqlx::query_scalar("SELECT recovery_codes_generation FROM users WHERE id = $1")
            .bind(r.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    let (s, b) = finish(&app, &r).await;
    assert_eq!(
        (s, b["error"].as_str(), b["state"].as_str()),
        (409, Some("ACCOUNT_REQUEST_NOT_READY"), Some("completed")),
        "{b}"
    );
    assert!(b.get("recovery_codes").is_none());
    let again: i64 =
        sqlx::query_scalar("SELECT recovery_codes_generation FROM users WHERE id = $1")
            .bind(r.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(again, generation);
}

#[sqlx::test]
async fn pending_authenticators_cannot_sign_in(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let r = ready(&app).await;
    let (phone, _) = own_phone(&app, &r).await;
    let (s, b) = rc(
        &app,
        Method::POST,
        "/auth/device-login/challenge",
        None,
        json!({"user_id": r.user, "device_id": phone.device_id}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("KEY_DEVICE_PENDING")),
        "{b}"
    );
    let ready_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT ready_at FROM account_requests WHERE id = $1")
            .bind(r.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let got: chrono::DateTime<chrono::Utc> = b["ready_at"].as_str().unwrap().parse().unwrap();
    assert_eq!(got.timestamp(), ready_at.timestamp());
    // After Finish the same device signs in.
    assert_eq!(finish(&app, &r).await.0, 200);
    let (s, b) = rc(
        &app,
        Method::POST,
        "/auth/device-login/challenge",
        None,
        json!({"user_id": r.user, "device_id": phone.device_id}),
    )
    .await;
    assert_eq!(s, 201, "{b}");
}
