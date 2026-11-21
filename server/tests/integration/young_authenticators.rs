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
