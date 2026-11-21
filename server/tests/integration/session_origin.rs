//! Sessions end with the key device that confirmed them and with the factor
//! generation they used (C27 "Sessions and assurance", B.7 rule 2).
//!
//! Run with: `cargo test --test integration session_origin`

use std::time::Duration;

use axum::body::Body;
use axum::http::Method;
use futures::StreamExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::signing::Decision;
use vc_server::auth::strong::AuthenticatorRef;
use vc_server::security_events::{record, Actor, EventType, NewSecurityEvent};

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, f4_sign_in, password_user, peer_request};
use super::helpers::migrations::{apply_from, apply_until};
use super::helpers::passkeys::passkey_session;
use super::helpers::{
    body_to_json_lenient, create_session_with_token, create_test_user, grant_step_up, make_admin,
    next_ws_event, spawn_test_server, ws_connect, TestApp,
};
use super::login_attempts::{answer_login, approved_attempt, attempt_id, claim, pc_call, qr_token};
use super::login_password::login_native;

const ORIGIN_MIGRATION: i64 = 20_261_127_000_000;
const PASSWORD: &str = "zq8#vT4p-origin";
const NEW_PASSWORD: &str = "Neuer-Pfad-7392-Zug";
const DESKTOP: &[(&str, &str)] = &[("X-Kaiku-Client", "desktop")];

// ============================================================================
// Helpers
// ============================================================================

async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let mut req = TestApp::request(method, uri).header("Authorization", format!("Bearer {token}"));
    let body = match body {
        Some(b) => {
            req = req.header("Content-Type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = app.oneshot(req.body(body).unwrap()).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// `(live, end_reason)` of a session row.
async fn ended(pool: &PgPool, sid: Uuid) -> (bool, Option<String>) {
    sqlx::query_as("SELECT ended_at IS NULL, end_reason FROM sessions WHERE id = $1")
        .bind(sid)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn live(pool: &PgPool, sid: Uuid) -> bool {
    ended(pool, sid).await.0
}

/// The session's recorded origin (read as text, so the helper compiles and
/// fails cleanly on a schema without the columns).
async fn origin(pool: &PgPool, sid: Uuid) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
                    'approved_by_type', approved_by_type,
                    'approved_by_id', approved_by_id,
                    'password_generation', password_generation,
                    'totp_generation', totp_generation)
           FROM sessions WHERE id = $1",
    )
    .bind(sid)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn sid_of(auth: &Value) -> Uuid {
    auth["session_id"]
        .as_str()
        .expect("session_id")
        .parse()
        .unwrap()
}

/// A QR sign-in of a desktop "PC" confirmed by `dev` → the `AuthResponse`.
async fn qr_pc(app: &TestApp, dev: &KeyDeviceFixture) -> Value {
    let (id, secret) = approved_attempt(app, dev).await;
    let (s, auth, _) = pc_call(
        app,
        &format!("/auth/login-attempts/{id}/collect"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    auth
}

/// A password sign-in of a desktop "PC" confirmed by `dev`.
async fn password_phone_pc(app: &TestApp, dev: &KeyDeviceFixture, name: &str) -> Value {
    let (s, step, secret) = login_native(app, name, PASSWORD).await;
    assert_eq!(s, 202, "{step}");
    let attempt = &step["attempt"];
    let (token, bytes) = qr_token(attempt);
    let (s, c) = claim(app, dev, &token).await;
    assert_eq!(s, 201, "{c}");
    let (s, a) = answer_login(app, dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(s, 200, "{a}");
    let id = attempt_id(attempt);
    let (s, auth, _) = pc_call(
        app,
        &format!("/auth/login-attempts/{id}/collect"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    auth
}

/// A password + TOTP sign-in of a desktop "PC" through the routes.
async fn password_totp_pc(
    app: &TestApp,
    user: Uuid,
    name: &str,
    password: &str,
    totp: &totp_rs::TOTP,
) -> Value {
    let resp = f4_sign_in(app, user, name, password, totp, DESKTOP).await;
    let status = resp.status().as_u16();
    let auth = body_to_json_lenient(resp).await;
    assert_eq!(status, 200, "{auth}");
    auth
}

/// Remove key device `target` from `actor`'s session (full + grant).
async fn remove_device(app: &TestApp, actor: &KeyDeviceFixture, target: Uuid) -> (u16, Value) {
    grant_step_up(
        &app.pool,
        actor.session_id.unwrap(),
        "key_device.remove",
        &target.to_string(),
    )
    .await;
    call(
        app,
        Method::DELETE,
        &format!("/api/approval-devices/{target}"),
        &actor.access_token,
        None,
    )
    .await
}

async fn refresh(app: &TestApp, refresh_token: &str) -> (u16, Value) {
    let req = peer_request(Method::POST, "/auth/refresh")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .body(Body::from(
            json!({ "refresh_token": refresh_token }).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn me(app: &TestApp, token: &str) -> (u16, Value) {
    call(app, Method::GET, "/auth/me", token, None).await
}

async fn event_details(pool: &PgPool, user: Uuid, event_type: &str) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = $2
          ORDER BY created_at, id",
    )
    .bind(user)
    .bind(event_type)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn insert_passkey(pool: &PgPool, user: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, credential, label, state,
                               backup_eligible, backup_state)
         VALUES ($1, $2, $3, '{}'::jsonb, 'Laptop', 'active', false, false)",
    )
    .bind(id)
    .bind(user)
    .bind(id.as_bytes().to_vec())
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn forget_totp_step(app: &TestApp, user: Uuid) {
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
}

fn totp_of(secret: &str, name: &str) -> totp_rs::TOTP {
    totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(secret.to_string())
            .to_bytes()
            .unwrap(),
        Some("Kaiku".into()),
        name.into(),
    )
    .unwrap()
}

/// A password + TOTP account with a passkey and a full passkey session
/// (the session that changes factors) → `(user, name, totp, sid, token)`.
async fn factor_account(app: &TestApp) -> (Uuid, String, totp_rs::TOTP, Uuid, String) {
    let (user, name) = password_user(&app.pool, PASSWORD).await;
    let totp = enable_totp(&app.pool, &app.config, user, &name).await;
    let passkey = insert_passkey(&app.pool, user).await;
    let (sid, token) = passkey_session(&app.pool, &app.config, user, passkey).await;
    (user, name, totp, sid, token)
}

async fn change_password(app: &TestApp, sid: Uuid, token: &str, new_password: &str) {
    grant_step_up(&app.pool, sid, "password.set", "").await;
    let (s, b) = call(
        app,
        Method::POST,
        "/auth/me/password",
        token,
        Some(json!({ "new_password": new_password })),
    )
    .await;
    assert_eq!(s, 200, "{b}");
}

// ============================================================================
// T1: back-fill
// ============================================================================

async fn raw_user(pool: &PgPool, name: &str, password: bool, totp: bool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (username, display_name, password_hash, mfa_secret,
                            password_generation, totp_generation)
         VALUES ($1, $1, $2, $3, 4, 2) RETURNING id",
    )
    .bind(name)
    .bind(password.then_some("x"))
    .bind(totp.then_some("enc"))
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn raw_session(pool: &PgPool, user: Uuid, methods: &[&str], ended: bool) -> Uuid {
    let id = Uuid::now_v7();
    let full = methods == ["device_key"];
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, assurance, auth_methods,
                               client_class, authenticator_type, authenticator_id,
                               absolute_expires_at, ended_at, end_reason)
         VALUES ($1, $2, $3, NOW() + INTERVAL '7 days', $4, $5, 'desktop', $6, $7,
                 NOW() + INTERVAL '90 days',
                 CASE WHEN $8 THEN NOW() END, CASE WHEN $8 THEN 'logout' END)",
    )
    .bind(id)
    .bind(user)
    .bind(format!("hash-{id}"))
    .bind(if full { "full" } else { "probation" })
    .bind(methods.iter().map(|m| (*m).to_string()).collect::<Vec<_>>())
    .bind(full.then_some("key_device"))
    .bind(full.then(Uuid::new_v4))
    .bind(ended)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn raw_device(pool: &PgPool, user: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO approval_devices (id, user_id, device_id, public_key, public_key_hash,
             uv_class, label, state, enrolled_via, activated_at)
         VALUES ($1, $2, gen_random_uuid(), '\\x00', decode(repeat('00', 32), 'hex'),
                 'per_use', 'P', 'active', 'cli', NOW())",
    )
    .bind(id)
    .bind(user)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn raw_consumed_attempt(pool: &PgPool, user: Uuid, device: Uuid, session: Uuid) {
    sqlx::query(
        "INSERT INTO login_attempts (id, kind, collection_hash, collection_kind,
             collection_digest, user_id, client_class, client_label, state, claimed_by_device,
             session_id, expires_at, consumed_at, ended_at)
         VALUES ($1, 'qr', '\\x01', 's256', '\\x01', $2, 'desktop', 'PC', 'consumed', $3, $4,
                 NOW() + INTERVAL '5 minutes', NOW(), NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(device)
    .bind(session)
    .execute(pool)
    .await
    .unwrap();
}

async fn raw_sign_in_event(pool: &PgPool, user: Uuid, session: Uuid, details: Value) {
    sqlx::query(
        "INSERT INTO security_events (id, subject_id, event_type, severity, actor_kind,
                                      actor_user_id, session_id, details)
         VALUES ($1, $2, 'sign_in', 'normal', 'self', $2, $3, $4)",
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(session)
    .bind(details)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = false)]
async fn session_origin_backfill(pool: PgPool) {
    apply_until(&pool, ORIGIN_MIGRATION).await;
    let u = raw_user(&pool, "origin_all", true, true).await;
    let bare = raw_user(&pool, "origin_bare", false, false).await;
    let dev = raw_device(&pool, u).await;

    // A QR session whose attempt still exists: the exact approver.
    let qr_known = raw_session(&pool, u, &["qr"], false).await;
    raw_consumed_attempt(&pool, u, dev, qr_known).await;
    // A QR session whose attempt is gone: "some key device".
    let qr_unknown = raw_session(&pool, u, &["qr"], false).await;
    // Password + phone, attempt gone, event with the generation.
    let pw_phone = raw_session(&pool, u, &["password", "phone_confirm"], false).await;
    raw_sign_in_event(
        &pool,
        u,
        pw_phone,
        json!({"auth_methods": ["password", "phone_confirm"],
               "factors": [{"factor": "password", "generation": 3}]}),
    )
    .await;
    // Password + TOTP: both generations from the event, no approver.
    let pw_totp = raw_session(&pool, u, &["password", "totp"], false).await;
    raw_sign_in_event(
        &pool,
        u,
        pw_totp,
        json!({"auth_methods": ["password", "totp"],
               "factors": [{"factor": "password", "generation": 3},
                           {"factor": "totp", "generation": 1}]}),
    )
    .await;
    // OIDC + TOTP without an event (safety net): the current TOTP generation.
    let oidc_totp = raw_session(&pool, u, &["oidc", "totp"], false).await;
    // Pre-v1 sessions: the account's current generations of what it has.
    let legacy = raw_session(&pool, u, &["legacy"], false).await;
    let legacy_bare = raw_session(&pool, bare, &["legacy"], false).await;
    // Untouched: an ended QR session (attempt present) and a device session.
    let qr_ended = raw_session(&pool, u, &["qr"], true).await;
    raw_consumed_attempt(&pool, u, dev, qr_ended).await;
    let device = raw_session(&pool, u, &["device_key"], false).await;
    // A non-array `factors` must not break the migration.
    let odd = raw_session(&pool, u, &["password", "totp"], false).await;
    raw_sign_in_event(&pool, u, odd, json!({"factors": {"password": 9}})).await;

    apply_from(&pool, ORIGIN_MIGRATION).await;

    let o = |t: Option<&str>, id: Option<Uuid>, pw: Option<i64>, totp: Option<i64>| {
        json!({"approved_by_type": t, "approved_by_id": id,
               "password_generation": pw, "totp_generation": totp})
    };
    assert_eq!(
        origin(&pool, qr_known).await,
        o(Some("key_device"), Some(dev), None, None)
    );
    assert_eq!(
        origin(&pool, qr_unknown).await,
        o(Some("key_device"), None, None, None)
    );
    assert_eq!(
        origin(&pool, pw_phone).await,
        o(Some("key_device"), None, Some(3), None)
    );
    assert_eq!(
        origin(&pool, pw_totp).await,
        o(None, None, Some(3), Some(1))
    );
    assert_eq!(origin(&pool, oidc_totp).await, o(None, None, None, Some(2)));
    assert_eq!(origin(&pool, legacy).await, o(None, None, Some(4), Some(2)));
    assert_eq!(origin(&pool, legacy_bare).await, o(None, None, None, None));
    assert_eq!(origin(&pool, qr_ended).await, o(None, None, None, None));
    assert_eq!(origin(&pool, device).await, o(None, None, None, None));
    assert_eq!(origin(&pool, odd).await, o(None, None, Some(4), Some(2)));
    // Nobody is signed out by the upgrade.
    for sid in [
        qr_known, qr_unknown, pw_phone, pw_totp, oidc_totp, legacy, device,
    ] {
        assert!(live(&pool, sid).await, "{sid} stays live");
    }
}

// ============================================================================
// T2: recorded at creation
// ============================================================================

#[sqlx::test]
async fn qr_session_records_approver(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let auth = qr_pc(&app, &dev).await;
    assert_eq!(
        origin(&pool, sid_of(&auth)).await,
        json!({"approved_by_type": "key_device", "approved_by_id": dev.row_id,
               "password_generation": null, "totp_generation": null})
    );
}

#[sqlx::test]
async fn password_phone_records_password_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    sqlx::query("UPDATE users SET password_generation = 3 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let auth = password_phone_pc(&app, &dev, &name).await;
    assert_eq!(
        origin(&pool, sid_of(&auth)).await,
        json!({"approved_by_type": "key_device", "approved_by_id": dev.row_id,
               "password_generation": 3, "totp_generation": null})
    );
}

/// A phone that only claimed the attempt did not confirm the sign-in.
#[sqlx::test]
async fn totp_after_phone_claim_records_no_approver(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    sqlx::query("UPDATE users SET totp_generation = 2 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    let (token, _) = qr_token(&step["attempt"]);
    let (s, c) = claim(&app, &dev, &token).await;
    assert_eq!(s, 201, "{c}");
    forget_totp_step(&app, user).await;
    let (s, auth, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    assert_eq!(
        origin(&pool, sid_of(&auth)).await,
        json!({"approved_by_type": null, "approved_by_id": null,
               "password_generation": 1, "totp_generation": 2})
    );
}
