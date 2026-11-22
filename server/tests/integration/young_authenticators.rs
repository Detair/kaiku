//! A newly added key phone or passkey may not act against older ones until
//! the account's waiting time (`ACCOUNT_REQUEST_DELAY_HOURS`) has passed since
//! it was added: removing an older phone or passkey, replacing the recovery
//! codes, setting or removing the password, setting or removing TOTP. 403
//! `AUTHENTICATOR_TOO_NEW {allowed_at}`; the step-up stays unused.
//!
//! Run with: `cargo test --test integration young_authenticators`

use axum::body::Body;
use axum::http::Method;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::passkeys::queries::PasskeyState;
use vc_server::auth::step_up::{grant_tx, StepUpAction, StepUpTarget, StepUpVia};
use vc_server::auth::strong::AuthenticatorRef;

use crate::enrollments::{claim_with, good, Phone};
use crate::helpers::approvals::KeyDeviceFixture;
use crate::helpers::login::{auth_user, login_config, peer_request};
use crate::helpers::passkeys::{passkey_session, seed_passkey};
use crate::helpers::{create_test_user, TestApp};

const STRONG_PASSWORD: &str = "quartz-Ember-lantern-8841";

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let req = peer_request(method, path)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json");
    let req = match body {
        Some(b) => req.body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Store a step-up grant on `sid` produced by `by` (the real `grant_tx`).
async fn grant(app: &TestApp, sid: Uuid, action: StepUpAction, target: &str, by: AuthenticatorRef) {
    let auth = auth_user(app, sid).await;
    let target = StepUpTarget::parse(action, target).unwrap();
    let via = match by.kind {
        vc_server::auth::strong::AuthenticatorKind::Passkey => StepUpVia::Passkey,
        vc_server::auth::strong::AuthenticatorKind::KeyDevice => StepUpVia::Device,
    };
    let mut tx = app.pool.begin().await.unwrap();
    grant_tx(&mut tx, &auth, action, &target, via, by)
        .await
        .expect("grant");
    tx.commit().await.unwrap();
}

/// Set the authenticator's creation time to `ago` before the database's
/// clock (the test's way of moving time).
async fn added_ago(pool: &PgPool, a: AuthenticatorRef, ago: Duration) {
    let table = match a.kind {
        vc_server::auth::strong::AuthenticatorKind::KeyDevice => "approval_devices",
        vc_server::auth::strong::AuthenticatorKind::Passkey => "passkeys",
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE {table} SET created_at = NOW() - make_interval(secs => $2) WHERE id = $1"
    )))
    .bind(a.id)
    .bind(ago.num_seconds() as f64)
    .execute(pool)
    .await
    .unwrap();
}

async fn created_at(pool: &PgPool, a: AuthenticatorRef) -> DateTime<Utc> {
    let table = match a.kind {
        vc_server::auth::strong::AuthenticatorKind::KeyDevice => "approval_devices",
        vc_server::auth::strong::AuthenticatorKind::Passkey => "passkeys",
    };
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT created_at FROM {table} WHERE id = $1"
    )))
    .bind(a.id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Open (unused, not voided) grants of the session.
async fn open_grants(pool: &PgPool, sid: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM step_up_grants
          WHERE session_id = $1 AND used_at IS NULL AND voided_at IS NULL",
    )
    .bind(sid)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn device_state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM approval_devices WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn kd(d: &KeyDeviceFixture) -> AuthenticatorRef {
    AuthenticatorRef::key_device(d.row_id)
}

/// The refusal: 403 `AUTHENTICATOR_TOO_NEW` with `allowed_at` = the
/// producer's creation time plus the waiting time.
async fn assert_too_new(
    pool: &PgPool,
    (status, body): (u16, Value),
    producer: AuthenticatorRef,
    hours: i64,
) {
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("AUTHENTICATOR_TOO_NEW")),
        "{body}"
    );
    let allowed_at: DateTime<Utc> = body["allowed_at"]
        .as_str()
        .expect("allowed_at is a string")
        .parse()
        .expect("allowed_at is RFC 3339");
    assert_eq!(
        allowed_at,
        created_at(pool, producer).await + Duration::hours(hours),
        "{body}"
    );
}

/// An account with an older phone (`old`, added 30 days ago) and a phone
/// added just now (`new`).
async fn old_and_new_phone(app: &TestApp) -> (Uuid, KeyDeviceFixture, KeyDeviceFixture) {
    let (user, _) = create_test_user(&app.pool).await;
    let old = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    added_ago(&app.pool, kd(&old), Duration::days(30)).await;
    let new = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    (user, old, new)
}

async fn remove_device(app: &TestApp, by: &KeyDeviceFixture, target: Uuid) -> (u16, Value) {
    send(
        app,
        Method::DELETE,
        &format!("/api/approval-devices/{target}"),
        &by.access_token,
        None,
    )
    .await
}

#[sqlx::test]
async fn fresh_device_cannot_remove_older(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, old, new) = old_and_new_phone(&app).await;
    let sid = new.session_id.unwrap();
    grant(
        &app,
        sid,
        StepUpAction::KeyDeviceRemove,
        &old.row_id.to_string(),
        kd(&new),
    )
    .await;
    let r = remove_device(&app, &new, old.row_id).await;
    assert_too_new(&app.pool, r, kd(&new), 24).await;
    // Nothing changed and the step-up is still there.
    assert_eq!(device_state(&app.pool, old.row_id).await, "active");
    assert_eq!(open_grants(&app.pool, sid).await, 1);

    // Once the waiting time has passed, the same step-up removes it.
    added_ago(&app.pool, kd(&new), Duration::hours(24)).await;
    let (s, b) = remove_device(&app, &new, old.row_id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, old.row_id).await, "revoked");
    assert_eq!(open_grants(&app.pool, sid).await, 0);
}

#[sqlx::test]
async fn fresh_device_may_remove_itself(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, _old, new) = old_and_new_phone(&app).await;
    grant(
        &app,
        new.session_id.unwrap(),
        StepUpAction::KeyDeviceRemove,
        &new.row_id.to_string(),
        kd(&new),
    )
    .await;
    let (s, b) = remove_device(&app, &new, new.row_id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, new.row_id).await, "revoked");
}

#[sqlx::test]
async fn fresh_device_may_remove_younger(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _old, new) = old_and_new_phone(&app).await;
    added_ago(&app.pool, kd(&new), Duration::hours(1)).await;
    let newer = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    grant(
        &app,
        new.session_id.unwrap(),
        StepUpAction::KeyDeviceRemove,
        &newer.row_id.to_string(),
        kd(&new),
    )
    .await;
    let (s, b) = remove_device(&app, &new, newer.row_id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, newer.row_id).await, "revoked");
}

#[sqlx::test]
async fn old_device_removes_old_and_new(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, new) = old_and_new_phone(&app).await;
    let older = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    added_ago(&app.pool, kd(&older), Duration::days(60)).await;
    let sid = old.session_id.unwrap();
    for target in [new.row_id, older.row_id] {
        grant(
            &app,
            sid,
            StepUpAction::KeyDeviceRemove,
            &target.to_string(),
            kd(&old),
        )
        .await;
        let (s, b) = remove_device(&app, &old, target).await;
        assert_eq!(s, 200, "{b}");
        assert_eq!(device_state(&app.pool, target).await, "revoked");
    }
    // "Sign out and remove this phone" on the old phone works too.
    let other = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    grant(
        &app,
        sid,
        StepUpAction::KeyDeviceRemove,
        &old.row_id.to_string(),
        kd(&old),
    )
    .await;
    let (s, b) = remove_device(&app, &old, old.row_id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, other.row_id).await, "active");
}

#[sqlx::test]
async fn fresh_passkey_cannot_remove_older_passkey(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let (_, old) = seed_passkey(&app, user, PasskeyState::Active).await;
    added_ago(
        &app.pool,
        AuthenticatorRef::passkey(old.id),
        Duration::days(3),
    )
    .await;
    let (_, new) = seed_passkey(&app, user, PasskeyState::Active).await;
    let (sid, token) = passkey_session(&app.pool, &app.config, user, new.id).await;
    let by = AuthenticatorRef::passkey(new.id);
    grant(
        &app,
        sid,
        StepUpAction::PasskeyRemove,
        &old.id.to_string(),
        by,
    )
    .await;
    let path = format!("/auth/passkeys/{}", old.id);
    let r = send(&app, Method::DELETE, &path, &token, None).await;
    assert_too_new(&app.pool, r, by, 24).await;
    assert_eq!(open_grants(&app.pool, sid).await, 1);
    // Removing itself stays allowed.
    grant(
        &app,
        sid,
        StepUpAction::PasskeyRemove,
        &new.id.to_string(),
        by,
    )
    .await;
    let (s, b) = send(
        &app,
        Method::DELETE,
        &format!("/auth/passkeys/{}", new.id),
        &token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

/// A fresh passkey on an account whose phone is older.
async fn old_phone_and_new_passkey(app: &TestApp) -> (Uuid, Uuid, String, AuthenticatorRef) {
    let (user, _) = create_test_user(&app.pool).await;
    let old = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    added_ago(&app.pool, kd(&old), Duration::days(30)).await;
    let (_, pk) = seed_passkey(app, user, PasskeyState::Active).await;
    let (sid, token) = passkey_session(&app.pool, &app.config, user, pk.id).await;
    (user, sid, token, AuthenticatorRef::passkey(pk.id))
}

#[sqlx::test]
async fn fresh_passkey_cannot_replace_recovery_codes(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, sid, token, by) = old_phone_and_new_passkey(&app).await;
    grant(&app, sid, StepUpAction::RecoveryCodesCreate, "", by).await;
    let r = send(&app, Method::POST, "/auth/recovery-codes", &token, None).await;
    assert_too_new(&app.pool, r, by, 24).await;
    assert_eq!(open_grants(&app.pool, sid).await, 1);
    added_ago(&app.pool, by, Duration::hours(25)).await;
    let (s, b) = send(&app, Method::POST, "/auth/recovery-codes", &token, None).await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn fresh_passkey_cannot_set_password(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, sid, token, by) = old_phone_and_new_passkey(&app).await;
    let before: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&app.pool)
            .await
            .unwrap();
    grant(&app, sid, StepUpAction::PasswordSet, "", by).await;
    let body = json!({ "new_password": STRONG_PASSWORD });
    let r = send(&app, Method::POST, "/auth/me/password", &token, Some(body)).await;
    assert_too_new(&app.pool, r, by, 24).await;
    let after: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(open_grants(&app.pool, sid).await, 1);
}

#[sqlx::test]
async fn fresh_device_cannot_remove_password(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _old, new) = old_and_new_phone(&app).await;
    let sid = new.session_id.unwrap();
    grant(&app, sid, StepUpAction::PasswordRemove, "", kd(&new)).await;
    let r = send(
        &app,
        Method::DELETE,
        "/auth/me/password",
        &new.access_token,
        None,
    )
    .await;
    assert_too_new(&app.pool, r, kd(&new), 24).await;
    let has: bool = sqlx::query_scalar("SELECT password_hash IS NOT NULL FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(has, "the password stays");
    assert_eq!(open_grants(&app.pool, sid).await, 1);
}

#[sqlx::test]
async fn fresh_device_totp_setup_refused_at_peek(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _old, new) = old_and_new_phone(&app).await;
    let sid = new.session_id.unwrap();
    grant(&app, sid, StepUpAction::TotpSet, "", kd(&new)).await;
    // The first call already says so, before a secret is shown.
    let r = send(
        &app,
        Method::POST,
        "/auth/mfa/setup",
        &new.access_token,
        None,
    )
    .await;
    assert_too_new(&app.pool, r, kd(&new), 24).await;

    // A setup started while the phone counted as settled cannot be finished
    // by a young one either (the check runs again when the grant is used).
    added_ago(&app.pool, kd(&new), Duration::hours(30)).await;
    let (s, b) = send(
        &app,
        Method::POST,
        "/auth/mfa/setup",
        &new.access_token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{b}");
    added_ago(&app.pool, kd(&new), Duration::minutes(5)).await;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(b["secret"].as_str().unwrap().to_string())
            .to_bytes()
            .unwrap(),
        Some("Kaiku".to_string()),
        username,
    )
    .unwrap();
    let code = totp.generate_current().unwrap();
    let r = send(
        &app,
        Method::POST,
        "/auth/mfa/verify",
        &new.access_token,
        Some(json!({ "code": code })),
    )
    .await;
    assert_too_new(&app.pool, r, kd(&new), 24).await;
    let has: bool = sqlx::query_scalar("SELECT mfa_secret IS NOT NULL FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(!has, "TOTP stays off");
    assert_eq!(open_grants(&app.pool, sid).await, 1);

    // The refusal came before the code was checked: the same code and the
    // same pending secret are accepted once the phone counts as settled.
    added_ago(&app.pool, kd(&new), Duration::hours(30)).await;
    let (s, b) = send(
        &app,
        Method::POST,
        "/auth/mfa/verify",
        &new.access_token,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn fresh_device_cannot_disable_totp(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _old, new) = old_and_new_phone(&app).await;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    crate::helpers::login::enable_totp(&app.pool, &app.config, user, &username).await;
    let sid = new.session_id.unwrap();
    grant(&app, sid, StepUpAction::TotpRemove, "", kd(&new)).await;
    let r = send(
        &app,
        Method::POST,
        "/auth/mfa/disable",
        &new.access_token,
        None,
    )
    .await;
    assert_too_new(&app.pool, r, kd(&new), 24).await;
    let has: bool = sqlx::query_scalar("SELECT mfa_secret IS NOT NULL FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(has, "TOTP stays on");
    assert_eq!(open_grants(&app.pool, sid).await, 1);
}

#[sqlx::test]
async fn first_authenticator_of_new_account_not_blocked(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    // The account's only phone, added just now; an older phone that was
    // removed does not count.
    let gone = KeyDeviceFixture::insert(&app.pool, &app.config, user, "revoked").await;
    added_ago(&app.pool, kd(&gone), Duration::days(30)).await;
    let only = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    let sid = only.session_id.unwrap();
    grant(&app, sid, StepUpAction::RecoveryCodesCreate, "", kd(&only)).await;
    let (s, b) = send(
        &app,
        Method::POST,
        "/auth/recovery-codes",
        &only.access_token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{b}");
    grant(&app, sid, StepUpAction::PasswordSet, "", kd(&only)).await;
    let (s, b) = send(
        &app,
        Method::POST,
        "/auth/me/password",
        &only.access_token,
        Some(json!({ "new_password": STRONG_PASSWORD })),
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn request_device_is_settled_when_active(pool: PgPool) {
    // A phone that waited as pending (an account request or an admin's
    // set-up of an existing account) counts from when it was added, not from
    // when it became active.
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, _) = old_and_new_phone(&app).await;
    let waited = KeyDeviceFixture::insert(&app.pool, &app.config, user, "pending").await;
    added_ago(&app.pool, kd(&waited), Duration::hours(25)).await;
    sqlx::query("UPDATE approval_devices SET state = 'active', activated_at = NOW() WHERE id = $1")
        .bind(waited.row_id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (sid, token) =
        crate::helpers::approvals::device_session(&app.pool, &app.config, user, waited.row_id)
            .await;
    grant(
        &app,
        sid,
        StepUpAction::KeyDeviceRemove,
        &old.row_id.to_string(),
        kd(&waited),
    )
    .await;
    let (s, b) = send(
        &app,
        Method::DELETE,
        &format!("/api/approval-devices/{}", old.row_id),
        &token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn delay_config_72h_respected(pool: PgPool) {
    let mut config = login_config();
    config.passkeys = Some(std::sync::Arc::new(
        crate::helpers::passkeys::test_passkey_rp(),
    ));
    config.account_request_delay_hours = 72;
    let app = TestApp::with_pool_and_config(pool, config).await;
    let (_, old, new) = old_and_new_phone(&app).await;
    added_ago(&app.pool, kd(&new), Duration::hours(48)).await;
    let sid = new.session_id.unwrap();
    grant(
        &app,
        sid,
        StepUpAction::KeyDeviceRemove,
        &old.row_id.to_string(),
        kd(&new),
    )
    .await;
    let r = remove_device(&app, &new, old.row_id).await;
    assert_too_new(&app.pool, r, kd(&new), 72).await;
    added_ago(&app.pool, kd(&new), Duration::hours(72)).await;
    let (s, b) = remove_device(&app, &new, old.row_id).await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn settled_exactly_at_the_waiting_time(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, old, new) = old_and_new_phone(&app).await;
    let sid = new.session_id.unwrap();
    grant(
        &app,
        sid,
        StepUpAction::KeyDeviceRemove,
        &old.row_id.to_string(),
        kd(&new),
    )
    .await;
    // Ten seconds short of the waiting time: still refused.
    added_ago(
        &app.pool,
        kd(&new),
        Duration::hours(24) - Duration::seconds(10),
    )
    .await;
    let r = remove_device(&app, &new, old.row_id).await;
    assert_too_new(&app.pool, r, kd(&new), 24).await;
    // Exactly the waiting time (the database's clock has moved on since,
    // never back): allowed.
    sqlx::query(
        "UPDATE approval_devices SET created_at = NOW() - INTERVAL '24 hours' WHERE id = $1",
    )
    .bind(new.row_id)
    .execute(&app.pool)
    .await
    .unwrap();
    let (s, b) = remove_device(&app, &new, old.row_id).await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn concurrent_refusals_keep_the_step_up(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, old, new) = old_and_new_phone(&app).await;
    let sid = new.session_id.unwrap();
    grant(
        &app,
        sid,
        StepUpAction::KeyDeviceRemove,
        &old.row_id.to_string(),
        kd(&new),
    )
    .await;
    let (a, b) = tokio::join!(
        remove_device(&app, &new, old.row_id),
        remove_device(&app, &new, old.row_id)
    );
    for (s, body) in [a, b] {
        assert_eq!(
            (s, body["error"].as_str()),
            (403, Some("AUTHENTICATOR_TOO_NEW")),
            "{body}"
        );
    }
    assert_eq!(device_state(&app.pool, old.row_id).await, "active");
    assert_eq!(open_grants(&app.pool, sid).await, 1);
}
#[sqlx::test]
async fn unknown_producer_is_never_old(pool: PgPool) {
    // A producer that is not an active authenticator of the account is
    // refused as a missing step-up, never treated as settled.
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, _) = old_and_new_phone(&app).await;
    let mut conn = app.pool.acquire().await.unwrap();
    for by in [
        AuthenticatorRef::key_device(Uuid::new_v4()),
        AuthenticatorRef::passkey(Uuid::new_v4()),
    ] {
        let r =
            vc_server::auth::strong::ensure_settled(&mut conn, user, by, Some(kd(&old)), 24).await;
        assert!(
            matches!(r, Err(vc_server::auth::AuthError::StepUpRequired)),
            "{r:?}"
        );
    }
}

// "That wasn't me": a newly added authenticator cannot have an older one
// revoked by reporting the older one's sign-in or addition.

/// Record an event of `user` now.
async fn record_event(
    pool: &PgPool,
    user: Uuid,
    t: vc_server::security_events::EventType,
    session: Option<Uuid>,
    a: Option<AuthenticatorRef>,
    details: Value,
) -> Uuid {
    use vc_server::security_events::{record, Actor, NewSecurityEvent};
    let mut conn = pool.acquire().await.unwrap();
    let mut ev = NewSecurityEvent::new(user, t, Actor::User(user), details);
    ev.session_id = session;
    ev.authenticator = a;
    record(&mut conn, ev).await.unwrap()
}

/// "That wasn't me" on `event` from `by`'s session, with a step-up `by` made.
async fn report(app: &TestApp, by: &KeyDeviceFixture, event: Uuid) -> (u16, Value) {
    grant(
        app,
        by.session_id.unwrap(),
        StepUpAction::EventRepudiate,
        &event.to_string(),
        kd(by),
    )
    .await;
    send(
        app,
        Method::POST,
        &format!("/api/me/security-events/{event}/repudiate"),
        &by.access_token,
        Some(json!({})),
    )
    .await
}

/// A device-key sign-in of `dev` (its own session), recorded now.
async fn device_sign_in(pool: &PgPool, user: Uuid, dev: &KeyDeviceFixture, factors: Value) -> Uuid {
    record_event(
        pool,
        user,
        vc_server::security_events::EventType::SignIn,
        dev.session_id,
        Some(kd(dev)),
        json!({"auth_methods": ["device_key"], "assurance": "full", "factors": factors}),
    )
    .await
}

async fn session_live(pool: &PgPool, sid: Uuid) -> bool {
    sqlx::query_scalar("SELECT ended_at IS NULL FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn password_state(pool: &PgPool, user: Uuid) -> String {
    sqlx::query_scalar("SELECT password_state FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn repudiated(pool: &PgPool, event: Uuid) -> bool {
    sqlx::query_scalar("SELECT repudiated_at IS NOT NULL FROM security_events WHERE id = $1")
        .bind(event)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Older phone (30 days) and a phone added a minute ago.
async fn old_and_young_phone(app: &TestApp) -> (Uuid, KeyDeviceFixture, KeyDeviceFixture) {
    let (user, old, young) = old_and_new_phone(app).await;
    added_ago(&app.pool, kd(&young), Duration::minutes(1)).await;
    (user, old, young)
}

#[sqlx::test]
async fn fresh_device_cannot_have_older_phone_revoked_by_report(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, young) = old_and_young_phone(&app).await;
    // The older phone signs in after the young one was added.
    let ev = device_sign_in(&app.pool, user, &old, json!([])).await;
    let r = report(&app, &young, ev).await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    assert_eq!(device_state(&app.pool, old.row_id).await, "active");
    assert!(session_live(&app.pool, old.session_id.unwrap()).await);
    assert!(!repudiated(&app.pool, ev).await);
    assert_eq!(password_state(&app.pool, user).await, "active");
    assert_eq!(open_grants(&app.pool, young.session_id.unwrap()).await, 1);

    // After the waiting time the same report is carried out.
    added_ago(&app.pool, kd(&young), Duration::hours(25)).await;
    let (s, b) = send(
        &app,
        Method::POST,
        &format!("/api/me/security-events/{ev}/repudiate"),
        &young.access_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, old.row_id).await, "revoked");
}

#[sqlx::test]
async fn fresh_device_cannot_have_older_passkey_revoked_by_report(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _old, young) = old_and_young_phone(&app).await;
    let (_, pk) = seed_passkey(&app, user, PasskeyState::Active).await;
    let passkey = AuthenticatorRef::passkey(pk.id);
    added_ago(&app.pool, passkey, Duration::days(3)).await;
    let (sid, _) = passkey_session(&app.pool, &app.config, user, pk.id).await;
    let ev = record_event(
        &app.pool,
        user,
        vc_server::security_events::EventType::SignIn,
        Some(sid),
        Some(passkey),
        json!({"auth_methods": ["passkey"], "assurance": "full"}),
    )
    .await;
    let r = report(&app, &young, ev).await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    let state: String = sqlx::query_scalar("SELECT state FROM passkeys WHERE id = $1")
        .bind(pk.id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
    assert!(session_live(&app.pool, sid).await);
}

#[sqlx::test]
async fn older_phone_reports_young_one(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, young) = old_and_young_phone(&app).await;
    let younger = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    // Its addition.
    let added = record_event(
        &app.pool,
        user,
        vc_server::security_events::EventType::AuthenticatorAdded,
        None,
        Some(kd(&young)),
        json!({"type": "key_device", "label": "Pixel 8"}),
    )
    .await;
    let (s, b) = report(&app, &old, added).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, young.row_id).await, "revoked");
    // A sign-in of another young one.
    let ev = device_sign_in(&app.pool, user, &younger, json!([])).await;
    let (s, b) = report(&app, &old, ev).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, younger.row_id).await, "revoked");
}

#[sqlx::test]
async fn fresh_device_reports_password_sign_in(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, young) = old_and_young_phone(&app).await;
    let pc = Uuid::new_v4();
    crate::helpers::insert_session_row(&app.pool, pc, user, false).await;
    let ev = record_event(
        &app.pool,
        user,
        vc_server::security_events::EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "totp"], "assurance": "probation",
               "factors": [{"factor": "password", "generation": 1}]}),
    )
    .await;
    let (s, b) = report(&app, &young, ev).await;
    assert_eq!(s, 200, "{b}");
    assert!(!session_live(&app.pool, pc).await);
    assert_eq!(b["compromised"], json!(["password"]));
    assert_eq!(device_state(&app.pool, old.row_id).await, "active");
}

#[sqlx::test]
async fn fresh_device_reports_younger_one(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, _old, young) = old_and_young_phone(&app).await;
    added_ago(&app.pool, kd(&young), Duration::hours(1)).await;
    let younger = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    let ev = device_sign_in(&app.pool, user, &younger, json!([])).await;
    let (s, b) = report(&app, &young, ev).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, younger.row_id).await, "revoked");
}

#[sqlx::test]
async fn only_young_authenticator_reports_freely(pool: PgPool) {
    // The only active authenticator is young; an older phone that waits as
    // pending (staged long ago) can still be reported.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let staged = KeyDeviceFixture::insert(&app.pool, &app.config, user, "pending").await;
    added_ago(&app.pool, kd(&staged), Duration::days(2)).await;
    let young = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    added_ago(&app.pool, kd(&young), Duration::minutes(1)).await;
    let added = record_event(
        &app.pool,
        user,
        vc_server::security_events::EventType::AuthenticatorAdded,
        None,
        Some(kd(&staged)),
        json!({"type": "key_device", "label": "Pixel 8"}),
    )
    .await;
    let (s, b) = report(&app, &young, added).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, staged.row_id).await, "revoked");
}

#[sqlx::test]
async fn refused_report_changes_nothing(pool: PgPool) {
    // A sign-in whose report would revoke the older phone and also end its
    // session and mark the password: refused as a whole.
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, young) = old_and_young_phone(&app).await;
    let ev = device_sign_in(
        &app.pool,
        user,
        &old,
        json!([{"factor": "password", "generation": 1}]),
    )
    .await;
    let r = report(&app, &young, ev).await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    assert_eq!(device_state(&app.pool, old.row_id).await, "active");
    assert!(session_live(&app.pool, old.session_id.unwrap()).await);
    assert_eq!(password_state(&app.pool, user).await, "active");
    assert!(!repudiated(&app.pool, ev).await);
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_events WHERE subject_id = $1 AND event_type = 'repudiated'",
    )
    .bind(user)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

// Setting up a phone that takes over an existing phone's install id replaces
// (revokes) that phone: a newly added issuer cannot replace an older one.

/// `POST /auth/enrollments` from `sid` with an `enrollment.issue` step-up
/// made by `by` → `(enrollment id, setup token)`.
async fn issue_setup(
    app: &TestApp,
    sid: Uuid,
    token: &str,
    by: AuthenticatorRef,
) -> (String, String) {
    grant(app, sid, StepUpAction::EnrollmentIssue, "", by).await;
    let (s, b) = send(
        app,
        Method::POST,
        "/auth/enrollments",
        token,
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    (
        b["enrollment_id"].as_str().unwrap().to_string(),
        b["setup_token"].as_str().unwrap().to_string(),
    )
}

/// The claimant asks for its challenge (`pop-challenge`).
async fn pop(app: &TestApp, setup_token: &str, phone: &Phone) -> (u16, Value) {
    let req = peer_request(Method::POST, "/auth/enrollments/pop-challenge")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"setup_token": setup_token, "device_id": phone.device_id,
                   "public_key": phone.public_key()})
            .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, crate::helpers::body_to_json_lenient(resp).await)
}

fn same_install_as(d: &KeyDeviceFixture) -> Phone {
    Phone {
        key: p256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng),
        device_id: d.device_id,
    }
}

async fn claim(app: &TestApp, setup_token: &str, phone: &Phone) -> String {
    let (s, b) = claim_with(app, setup_token, phone, "Pixel 9", good(), "per_use").await;
    assert_eq!(s, 201, "{b}");
    b["code"].as_str().unwrap().to_string()
}

/// The older phone is untouched: active, its session live.
async fn assert_untouched(pool: &PgPool, d: &KeyDeviceFixture) {
    assert_eq!(device_state(pool, d.row_id).await, "active");
    assert!(session_live(pool, d.session_id.unwrap()).await);
}

#[sqlx::test]
async fn fresh_phone_setup_cannot_replace_older_phone(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, old, young) = old_and_young_phone(&app).await;
    let (id, setup) = issue_setup(
        &app,
        young.session_id.unwrap(),
        &young.access_token,
        kd(&young),
    )
    .await;
    // Refused when the claimant asks for its challenge, before any code.
    let phone = same_install_as(&old);
    let r = pop(&app, &setup, &phone).await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    assert_untouched(&app.pool, &old).await;

    // A claim that got past that step (the issuer counted as settled then)
    // is refused again when the code is confirmed; nothing is revoked.
    added_ago(&app.pool, kd(&young), Duration::hours(25)).await;
    let code = claim(&app, &setup, &phone).await;
    added_ago(&app.pool, kd(&young), Duration::minutes(1)).await;
    let r = crate::enrollment_issuers::type_code_on(&app, &young, &id, &code).await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    assert_untouched(&app.pool, &old).await;

    // After the waiting time the same set-up replaces the older phone.
    added_ago(&app.pool, kd(&young), Duration::hours(25)).await;
    let (s, b) = crate::enrollment_issuers::type_code_on(&app, &young, &id, &code).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, old.row_id).await, "revoked");
}

#[sqlx::test]
async fn fresh_passkey_setup_cannot_replace_older_phone(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, _) = old_and_young_phone(&app).await;
    let (_, pk) = seed_passkey(&app, user, PasskeyState::Active).await;
    let by = AuthenticatorRef::passkey(pk.id);
    let (sid, token) = passkey_session(&app.pool, &app.config, user, pk.id).await;
    let (id, setup) = issue_setup(&app, sid, &token, by).await;
    let phone = same_install_as(&old);
    let r = pop(&app, &setup, &phone).await;
    assert_too_new(&app.pool, r, by, 24).await;
    assert_untouched(&app.pool, &old).await;

    added_ago(&app.pool, by, Duration::hours(25)).await;
    let code = claim(&app, &setup, &phone).await;
    added_ago(&app.pool, by, Duration::minutes(1)).await;
    let path = format!("/auth/enrollments/{id}/confirm");
    let r = send(
        &app,
        Method::POST,
        &path,
        &token,
        Some(json!({ "code": code })),
    )
    .await;
    assert_too_new(&app.pool, r, by, 24).await;
    assert_untouched(&app.pool, &old).await;
}

#[sqlx::test]
async fn settled_issuer_replaces_older_phone(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (_, old, issuer) = old_and_young_phone(&app).await;
    added_ago(&app.pool, kd(&issuer), Duration::days(10)).await;
    let (id, setup) = issue_setup(
        &app,
        issuer.session_id.unwrap(),
        &issuer.access_token,
        kd(&issuer),
    )
    .await;
    let code = claim(&app, &setup, &same_install_as(&old)).await;
    let (s, b) = crate::enrollment_issuers::type_code_on(&app, &issuer, &id, &code).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, old.row_id).await, "revoked");
}

#[sqlx::test]
async fn fresh_issuer_own_install_and_younger_and_new(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, young) = old_and_young_phone(&app).await;
    added_ago(&app.pool, kd(&young), Duration::hours(1)).await;
    let younger = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    let sid = young.session_id.unwrap();

    // Its own install: refused as before ("this phone is already a key").
    let (id, setup) = issue_setup(&app, sid, &young.access_token, kd(&young)).await;
    let (s, b) = pop(&app, &setup, &same_install_as(&young)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (400, Some("ENROLLMENT_DEVICE_IN_USE")),
        "{b}"
    );

    // A younger phone's install: replaced.
    let code = claim(&app, &setup, &same_install_as(&younger)).await;
    let (s, b) = crate::enrollment_issuers::type_code_on(&app, &young, &id, &code).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&app.pool, younger.row_id).await, "revoked");

    // A new install: added, nothing replaced.
    let (id, setup) = issue_setup(&app, sid, &young.access_token, kd(&young)).await;
    let phone = Phone::new();
    let code = claim(&app, &setup, &phone).await;
    let (s, b) = crate::enrollment_issuers::type_code_on(&app, &young, &id, &code).await;
    assert_eq!(s, 200, "{b}");
    assert_untouched(&app.pool, &old).await;
}

#[sqlx::test]
async fn command_line_setup_still_replaces(pool: PgPool) {
    use vc_server::approvals::enrollments::{
        confirm_tx, issue_tx, Confirmer, EnrollmentVariant, IssueParams, Issuer,
    };
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, _young) = old_and_young_phone(&app).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
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
    let code = claim(&app, &issued.setup_token, &same_install_as(&old)).await;
    let mut tx = app.pool.begin().await.unwrap();
    confirm_tx(
        &mut tx,
        &ctx,
        issued.enrollment_id,
        Confirmer::Cli,
        code.as_bytes(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(device_state(&app.pool, old.row_id).await, "revoked");
}

#[sqlx::test]
async fn newest_step_up_is_used(pool: PgPool) {
    // Confirmed first with a new passkey (refused), then with an older one
    // in the same session: the next try uses the older one's confirmation.
    let app = TestApp::with_passkeys(pool).await;
    let (_, sid, token, young) = old_phone_and_new_passkey(&app).await;
    let user: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let (_, older) = seed_passkey(&app, user, PasskeyState::Active).await;
    added_ago(
        &app.pool,
        AuthenticatorRef::passkey(older.id),
        Duration::days(5),
    )
    .await;
    grant(&app, sid, StepUpAction::RecoveryCodesCreate, "", young).await;
    let r = send(&app, Method::POST, "/auth/recovery-codes", &token, None).await;
    assert_too_new(&app.pool, r, young, 24).await;
    // Make the young one's confirmation older than the next one.
    sqlx::query("UPDATE step_up_grants SET created_at = created_at - INTERVAL '1 second', expires_at = expires_at - INTERVAL '1 second' WHERE session_id = $1")
        .bind(sid)
        .execute(&app.pool)
        .await
        .unwrap();
    grant(
        &app,
        sid,
        StepUpAction::RecoveryCodesCreate,
        "",
        AuthenticatorRef::passkey(older.id),
    )
    .await;
    let (s, b) = send(&app, Method::POST, "/auth/recovery-codes", &token, None).await;
    assert_eq!(s, 200, "{b}");
}

#[sqlx::test]
async fn own_setup_without_issuer_never_replaces(pool: PgPool) {
    // An own set-up whose issuing phone or passkey is not recorded (no path
    // writes such a row; staged here by hand) never replaces a live key.
    let app = TestApp::with_passkeys(pool).await;
    let (_, old, issuer) = old_and_young_phone(&app).await;
    added_ago(&app.pool, kd(&issuer), Duration::days(10)).await;
    let (id, setup) = issue_setup(
        &app,
        issuer.session_id.unwrap(),
        &issuer.access_token,
        kd(&issuer),
    )
    .await;
    sqlx::query(
        "UPDATE enrollments SET issuer_device_id = NULL, issuer_passkey_id = NULL
          WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&app.pool)
    .await
    .unwrap();
    let (s, b) = pop(&app, &setup, &same_install_as(&old)).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("ENROLLMENT_STATE")),
        "{b}"
    );
    assert_untouched(&app.pool, &old).await;
}

#[sqlx::test]
async fn boundary_is_exact(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, young) = old_and_young_phone(&app).await;
    let at = created_at(&app.pool, kd(&young)).await + Duration::hours(24);
    let mut conn = app.pool.acquire().await.unwrap();
    // Exactly the waiting time after its creation: settled.
    let r = vc_server::auth::strong::ensure_settled_at(
        &mut conn,
        user,
        kd(&young),
        Some(kd(&old)),
        24,
        at,
    )
    .await;
    assert!(r.is_ok(), "{r:?}");
    let r = vc_server::auth::strong::ensure_settled_at(
        &mut conn,
        user,
        kd(&young),
        Some(kd(&old)),
        24,
        at - Duration::microseconds(1),
    )
    .await;
    assert!(
        matches!(r, Err(vc_server::auth::AuthError::AuthenticatorTooNew { allowed_at }) if allowed_at == at),
        "{r:?}"
    );
}

// The admin routes on the admin's own account follow the same rule.

async fn admin_remove(
    app: &TestApp,
    by: &KeyDeviceFixture,
    account: Uuid,
    target: Uuid,
) -> (u16, Value) {
    grant(
        app,
        by.session_id.unwrap(),
        StepUpAction::AdminAuthenticatorRemove,
        &target.to_string(),
        kd(by),
    )
    .await;
    send(
        app,
        Method::POST,
        &format!("/api/admin/users/{account}/authenticators/{target}/remove"),
        &by.access_token,
        None,
    )
    .await
}

#[sqlx::test]
async fn admin_fresh_phone_cannot_remove_own_older_phone(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (admin, old, young) = old_and_young_phone(&app).await;
    crate::helpers::make_admin(&app.pool, admin).await;
    // Admin mode opens from the young phone.
    grant(
        &app,
        young.session_id.unwrap(),
        StepUpAction::AdminElevate,
        "",
        kd(&young),
    )
    .await;
    let (s, b) = send(
        &app,
        Method::POST,
        "/api/admin/elevate",
        &young.access_token,
        Some(json!({"reason": "test"})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    // "Remove device now" on the own older phone: refused.
    let r = admin_remove(&app, &young, admin, old.row_id).await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    assert_untouched(&app.pool, &old).await;
    // Factor states on the own account: refused.
    grant(
        &app,
        young.session_id.unwrap(),
        StepUpAction::AdminFactorSetState,
        &admin.to_string(),
        kd(&young),
    )
    .await;
    let r = send(
        &app,
        Method::PUT,
        &format!("/api/admin/users/{admin}/factors"),
        &young.access_token,
        Some(json!({"password": "compromised"})),
    )
    .await;
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    assert_eq!(password_state(&app.pool, admin).await, "active");

    // Another account: not affected.
    let (other, _) = create_test_user(&app.pool).await;
    let theirs = KeyDeviceFixture::insert(&app.pool, &app.config, other, "active").await;
    added_ago(&app.pool, kd(&theirs), Duration::days(30)).await;
    let (s, b) = admin_remove(&app, &young, other, theirs.row_id).await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(device_state(&app.pool, theirs.row_id).await, "revoked");
}

#[sqlx::test]
async fn admin_settled_phone_removes_own_older_phone(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (admin, old, settled) = old_and_young_phone(&app).await;
    added_ago(&app.pool, kd(&settled), Duration::days(2)).await;
    crate::helpers::make_admin(&app.pool, admin).await;
    let (s, b) = admin_remove(&app, &settled, admin, old.row_id).await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(device_state(&app.pool, old.row_id).await, "revoked");
}

#[sqlx::test]
async fn totp_verify_checks_again_when_the_step_up_is_used(pool: PgPool) {
    // An older phone becomes active between the early check and the use of
    // the step-up (for example when an account request completes): the check
    // at use refuses, TOTP stays off.
    let app = TestApp::with_passkeys(pool).await;
    let (user, _) = create_test_user(&app.pool).await;
    let older = KeyDeviceFixture::insert(&app.pool, &app.config, user, "pending").await;
    added_ago(&app.pool, kd(&older), Duration::days(2)).await;
    let young = KeyDeviceFixture::insert(&app.pool, &app.config, user, "active").await;
    let sid = young.session_id.unwrap();
    grant(&app, sid, StepUpAction::TotpSet, "", kd(&young)).await;
    let (s, b) = send(
        &app,
        Method::POST,
        "/auth/mfa/setup",
        &young.access_token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    let code = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(b["secret"].as_str().unwrap().to_string())
            .to_bytes()
            .unwrap(),
        Some("Kaiku".to_string()),
        username,
    )
    .unwrap()
    .generate_current()
    .unwrap();

    // Hold the step-up row: the request passes the early check and the code,
    // then waits where it uses the step-up.
    let mut hold = app.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM step_up_grants WHERE session_id = $1 FOR UPDATE")
        .bind(sid)
        .fetch_all(&mut *hold)
        .await
        .unwrap();
    let activate = async {
        crate::helpers::wait_for_lock_waiters(&app.pool, 1).await;
        sqlx::query(
            "UPDATE approval_devices SET state = 'active', activated_at = NOW() WHERE id = $1",
        )
        .bind(older.row_id)
        .execute(&mut *hold)
        .await
        .unwrap();
        hold.commit().await.unwrap();
    };
    let (r, ()) = tokio::join!(
        send(
            &app,
            Method::POST,
            "/auth/mfa/verify",
            &young.access_token,
            Some(json!({ "code": code })),
        ),
        activate
    );
    assert_too_new(&app.pool, r, kd(&young), 24).await;
    let has: bool = sqlx::query_scalar("SELECT mfa_secret IS NOT NULL FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(!has, "TOTP stays off");
}

#[sqlx::test]
async fn setup_with_issuer_gone_is_no_longer_valid(pool: PgPool) {
    // A PC set-up whose issuing passkey was removed while the PC session
    // (signed in with another passkey) stays live.
    let app = TestApp::with_passkeys(pool).await;
    let (user, old, _) = old_and_young_phone(&app).await;
    let (_, signed_in) = seed_passkey(&app, user, PasskeyState::Active).await;
    let a = AuthenticatorRef::passkey(signed_in.id);
    added_ago(&app.pool, a, Duration::days(30)).await;
    let (_, issuing) = seed_passkey(&app, user, PasskeyState::Active).await;
    let b = AuthenticatorRef::passkey(issuing.id);
    added_ago(&app.pool, b, Duration::hours(25)).await;
    let (sid, token) = passkey_session(&app.pool, &app.config, user, signed_in.id).await;

    // At the challenge step.
    let (_, setup) = issue_setup(&app, sid, &token, b).await;
    // A second set-up, claimed while the issuer was still there.
    let (id2, setup2) = issue_setup(&app, sid, &token, b).await;
    let phone = same_install_as(&old);
    let code = claim(&app, &setup2, &phone).await;
    grant(
        &app,
        sid,
        StepUpAction::PasskeyRemove,
        &issuing.id.to_string(),
        a,
    )
    .await;
    let (s, r) = send(
        &app,
        Method::DELETE,
        &format!("/auth/passkeys/{}", issuing.id),
        &token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{r}");
    let (s, r) = pop(&app, &setup, &phone).await;
    assert_eq!(
        (s, r["error"].as_str()),
        (409, Some("ENROLLMENT_STATE")),
        "{r}"
    );
    assert_untouched(&app.pool, &old).await;
    // At completion.
    let (s, r) = send(
        &app,
        Method::POST,
        &format!("/auth/enrollments/{id2}/confirm"),
        &token,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(
        (s, r["error"].as_str()),
        (409, Some("ENROLLMENT_STATE")),
        "{r}"
    );
    assert_untouched(&app.pool, &old).await;
}

#[sqlx::test]
async fn admin_fresh_phone_removes_own_younger_and_itself(pool: PgPool) {
    let app = TestApp::with_passkeys(pool).await;
    let (admin, old, young) = old_and_young_phone(&app).await;
    added_ago(&app.pool, kd(&young), Duration::hours(1)).await;
    crate::helpers::make_admin(&app.pool, admin).await;
    let younger = KeyDeviceFixture::insert(&app.pool, &app.config, admin, "active").await;
    let (s, b) = admin_remove(&app, &young, admin, younger.row_id).await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(device_state(&app.pool, younger.row_id).await, "revoked");
    let (s, b) = admin_remove(&app, &young, admin, young.row_id).await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(device_state(&app.pool, young.row_id).await, "revoked");
    assert_eq!(device_state(&app.pool, old.row_id).await, "active");
}
