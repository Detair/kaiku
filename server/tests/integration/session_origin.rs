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

// ============================================================================
// T3: removing a key device ends the sessions it confirmed
// ============================================================================

/// The proof: a PC signed in by QR loses its socket, its refresh and its
/// bearer when the confirming phone is removed.
#[sqlx::test]
async fn removing_phone_ends_qr_sessions_it_approved(pool: PgPool) {
    use tokio_tungstenite::tungstenite::Message;
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    // A second phone, so the first is not the last strong authenticator.
    let _second = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = qr_pc(&app, &phone).await;
    let pc_access = pc["access_token"].as_str().unwrap().to_string();
    let pc_refresh = pc["refresh_token"].as_str().unwrap().to_string();
    assert_eq!(me(&app, &pc_access).await.0, 200);
    let mut ws = ws_connect(&server, &pc_access).await;
    next_ws_event(&mut ws, "ready", Duration::from_secs(5)).await;

    // "Sign out and remove this phone" from the phone's own full session.
    let (s, b) = remove_device(&app, &phone, phone.row_id).await;
    assert_eq!(s, 200, "{b}");

    let err = next_ws_event(&mut ws, "error", Duration::from_secs(5)).await;
    assert_eq!(err["code"], "SESSION_REVOKED");
    let close = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    })
    .await
    .expect("closed in time");
    let code: u16 = close.expect("close code").code.into();
    assert_eq!(code, 1008);

    let (s, b) = refresh(&app, &pc_refresh).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_TOKEN")),
        "{b}"
    );
    let (s, b) = me(&app, &pc_access).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_TOKEN")),
        "{b}"
    );
    assert_eq!(
        ended(&pool, sid_of(&pc)).await,
        (false, Some("authenticator_revoked".into()))
    );
    // The notice counts the phone's own session and the PC's.
    let removed = event_details(&pool, user, "authenticator_removed").await;
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["ended_sessions"], 2, "{}", removed[0]);
    assert_eq!(removed[0]["type"], "key_device");
}

#[sqlx::test]
async fn removing_phone_ends_password_phone_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = password_phone_pc(&app, &phone, &name).await;
    let (s, b) = remove_device(&app, &other, phone.row_id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(
        ended(&pool, sid_of(&pc)).await,
        (false, Some("authenticator_revoked".into()))
    );
    assert!(live(&pool, other.session_id.unwrap()).await);
}

#[sqlx::test]
async fn admin_removal_ends_qr_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (admin_sid, admin_token) = create_session_with_token(&pool, &app.config, admin).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = qr_pc(&app, &phone).await;
    grant_step_up(
        &pool,
        admin_sid,
        "admin.authenticator.remove",
        &phone.row_id.to_string(),
    )
    .await;
    let (s, b) = call(
        &app,
        Method::POST,
        &format!(
            "/api/admin/users/{user}/authenticators/{}/remove",
            phone.row_id
        ),
        &admin_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(
        ended(&pool, sid_of(&pc)).await,
        (false, Some("authenticator_revoked".into()))
    );
}

#[derive(Default)]
struct CliOut(Vec<String>);

impl vc_server::cli::CliIo for CliOut {
    fn print(&mut self, s: &str) {
        self.0.push(s.to_string());
    }

    async fn read_line(&mut self, _prompt: &str) -> std::io::Result<String> {
        Err(std::io::ErrorKind::UnexpectedEof.into())
    }
}

#[sqlx::test]
async fn cli_removal_ends_qr_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = qr_pc(&app, &phone).await;
    let args: Vec<String> = [
        "devices",
        "remove",
        name.as_str(),
        &phone.row_id.to_string(),
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    let cmd = vc_server::cli::Command::parse(&args).unwrap();
    let ctx = vc_server::cli::CliCtx {
        db: app.pool.clone(),
        redis: app.state.redis.clone(),
        config: (*app.config).clone(),
    };
    vc_server::cli::run(&ctx, cmd, &mut CliOut::default())
        .await
        .unwrap();
    assert_eq!(
        ended(&pool, sid_of(&pc)).await,
        (false, Some("authenticator_revoked".into()))
    );
}

/// "That wasn't me" on the phone's `authenticator_added` revokes it, and
/// with it the PC it confirmed.
#[sqlx::test]
async fn repudiated_phone_ends_qr_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let old = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let added = {
        let mut conn = pool.acquire().await.unwrap();
        let mut ev = NewSecurityEvent::new(
            user,
            EventType::AuthenticatorAdded,
            Actor::User(user),
            json!({"type": "key_device"}),
        );
        ev.authenticator = Some(AuthenticatorRef::key_device(phone.row_id));
        record(&mut conn, ev).await.unwrap()
    };
    let pc = qr_pc(&app, &phone).await;
    grant_step_up(
        &pool,
        old.session_id.unwrap(),
        "event.repudiate",
        &added.to_string(),
    )
    .await;
    let (s, b) = call(
        &app,
        Method::POST,
        &format!("/api/me/security-events/{added}/repudiate"),
        &old.access_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["revoked_authenticator_ids"], json!([phone.row_id]));
    assert_eq!(
        ended(&pool, sid_of(&pc)).await,
        (false, Some("authenticator_revoked".into()))
    );
    assert!(live(&pool, old.session_id.unwrap()).await);
}

#[sqlx::test]
async fn invalidated_phone_ends_qr_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = qr_pc(&app, &phone).await;
    let (s, b) = call(
        &app,
        Method::POST,
        &format!("/api/approval-devices/{}/invalidated", phone.row_id),
        &phone.access_token,
        Some(json!({})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(
        ended(&pool, sid_of(&pc)).await,
        (false, Some("authenticator_revoked".into()))
    );
    let notice = event_details(&pool, user, "key_invalidated").await;
    assert_eq!(notice.len(), 1);
    assert_eq!(notice[0]["ended_sessions"], 2, "{}", notice[0]);
}

#[sqlx::test]
async fn other_phones_sessions_survive(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let a = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let b = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc_a = qr_pc(&app, &a).await;
    let pc_b = qr_pc(&app, &b).await;
    // Another account's PC confirmed by its own phone.
    let (stranger, _) = create_test_user(&pool).await;
    let s_phone = KeyDeviceFixture::insert(&pool, &app.config, stranger, "active").await;
    let pc_s = qr_pc(&app, &s_phone).await;

    let (s, body) = remove_device(&app, &b, a.row_id).await;
    assert_eq!(s, 200, "{body}");
    assert!(!live(&pool, sid_of(&pc_a)).await);
    assert!(
        live(&pool, sid_of(&pc_b)).await,
        "the other phone's PC stays"
    );
    assert!(live(&pool, b.session_id.unwrap()).await);
    assert!(
        live(&pool, sid_of(&pc_s)).await,
        "another account is untouched"
    );
}

/// Sessions from before the upgrade whose approver is unknown end when any
/// key device of the account is revoked; sessions that no phone confirmed
/// stay.
#[sqlx::test]
async fn unknown_approver_ends_on_any_device_removal(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let a = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let b = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc_b = qr_pc(&app, &b).await;
    let unknown = raw_session(&pool, user, &["qr"], false).await;
    sqlx::query("UPDATE sessions SET approved_by_type = 'key_device' WHERE id = $1")
        .bind(unknown)
        .execute(&pool)
        .await
        .unwrap();
    let legacy = raw_session(&pool, user, &["legacy"], false).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let pw_totp = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;

    let (s, body) = remove_device(&app, &b, a.row_id).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(
        ended(&pool, unknown).await,
        (false, Some("authenticator_revoked".into()))
    );
    assert!(live(&pool, sid_of(&pc_b)).await, "known approver b stays");
    assert!(live(&pool, legacy).await, "no phone confirmed it");
    assert!(live(&pool, sid_of(&pw_totp)).await, "no phone confirmed it");
}

/// Collect and revoke race: whatever commits first, no live session is left
/// that a revoked key device confirmed.
#[sqlx::test]
async fn collect_racing_revocation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let mut collected = 0;
    let mut refused = 0;
    for round in 0..50u64 {
        let (user, _) = create_test_user(&pool).await;
        let keeper = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
        let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
        let (id, secret) = approved_attempt(&app, &phone).await;
        grant_step_up(
            &pool,
            keeper.session_id.unwrap(),
            "key_device.remove",
            &phone.row_id.to_string(),
        )
        .await;
        let uri = format!("/auth/login-attempts/{id}/collect");
        let remove_uri = format!("/api/approval-devices/{}", phone.row_id);
        // A varying head start for the revocation, so both commit orders
        // occur over the rounds.
        let delay = Duration::from_micros((round % 10) * 400);
        let collect = async {
            tokio::time::sleep(delay).await;
            pc_call(&app, &uri, &secret, json!({})).await
        };
        let revoke = call(
            &app,
            Method::DELETE,
            &remove_uri,
            &keeper.access_token,
            None,
        );
        let ((cs, cb, _), (rs, rb)) = tokio::join!(collect, revoke);
        assert_eq!(rs, 200, "{rb}");
        match cs {
            200 => collected += 1,
            410 => refused += 1,
            other => panic!("collect answered {other}: {cb}"),
        }
    }
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts a
           JOIN sessions s ON s.id = a.session_id
           JOIN approval_devices d ON d.id = a.claimed_by_device
          WHERE s.ended_at IS NULL AND d.state = 'revoked'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    eprintln!("collect_racing_revocation: collected {collected}, refused {refused}");
    assert_eq!(
        left, 0,
        "live sessions confirmed by a revoked device ({collected} collected, {refused} refused)"
    );
}

// ============================================================================
// T4: a factor change ends the sessions that used the factor
// ============================================================================

/// The proof: a password change from a passkey session ends the
/// password + TOTP session and keeps the passkey session (and sessions that
/// used no password).
#[sqlx::test]
async fn password_change_ends_password_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, pk_sid, pk_token) = factor_account(&app).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pw_totp = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    let pw_phone = password_phone_pc(&app, &phone, &name).await;
    let qr = qr_pc(&app, &phone).await;

    change_password(&app, pk_sid, &pk_token, NEW_PASSWORD).await;

    assert_eq!(
        ended(&pool, sid_of(&pw_totp)).await,
        (false, Some("factor_ended".into()))
    );
    assert_eq!(
        ended(&pool, sid_of(&pw_phone)).await,
        (false, Some("factor_ended".into()))
    );
    assert!(
        live(&pool, pk_sid).await,
        "the acting passkey session stays"
    );
    assert!(
        live(&pool, sid_of(&qr)).await,
        "a QR sign-in used no password"
    );
    assert!(live(&pool, phone.session_id.unwrap()).await);
    let (s, b) = refresh(&app, pw_totp["refresh_token"].as_str().unwrap()).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_TOKEN")),
        "{b}"
    );
    let changed = event_details(&pool, user, "password_changed").await;
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["ended_sessions"], 2, "{}", changed[0]);
    assert_eq!(changed[0]["generation"], 2, "the old field stays");
}

/// A session of an older generation is not counted twice; a change ends
/// only the sessions of the generation it ends.
#[sqlx::test]
async fn older_generation_session_unaffected(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, pk_sid, pk_token) = factor_account(&app).await;
    let first = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    change_password(&app, pk_sid, &pk_token, NEW_PASSWORD).await;
    assert!(!live(&pool, sid_of(&first)).await);
    let second = password_totp_pc(&app, user, &name, NEW_PASSWORD, &totp).await;
    assert_eq!(
        origin(&pool, sid_of(&second)).await["password_generation"],
        2
    );
    assert!(live(&pool, sid_of(&second)).await, "made after the change");
    change_password(&app, pk_sid, &pk_token, "Zweiter-Weg-5521-Bus").await;
    assert_eq!(
        ended(&pool, sid_of(&second)).await,
        (false, Some("factor_ended".into()))
    );
    let counts: Vec<Value> = event_details(&pool, user, "password_changed")
        .await
        .into_iter()
        .map(|d| d["ended_sessions"].clone())
        .collect();
    assert_eq!(counts, vec![json!(1), json!(1)]);
}

#[sqlx::test]
async fn totp_renew_ends_totp_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, pk_sid, pk_token) = factor_account(&app).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pw_totp = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    let pw_phone = password_phone_pc(&app, &phone, &name).await;

    grant_step_up(&pool, pk_sid, "totp.set", "").await;
    let (s, setup) = call(&app, Method::POST, "/auth/mfa/setup", &pk_token, None).await;
    assert_eq!(s, 200, "{setup}");
    let renewed = totp_of(setup["secret"].as_str().unwrap(), &name);
    forget_totp_step(&app, user).await;
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/mfa/verify",
        &pk_token,
        Some(json!({"code": renewed.generate_current().unwrap()})),
    )
    .await;
    assert_eq!(s, 200, "{b}");

    assert_eq!(
        ended(&pool, sid_of(&pw_totp)).await,
        (false, Some("factor_ended".into()))
    );
    assert!(
        live(&pool, sid_of(&pw_phone)).await,
        "it used the password, not the TOTP secret"
    );
    assert!(live(&pool, pk_sid).await);
    let added = event_details(&pool, user, "authenticator_added").await;
    let added: Vec<&Value> = added.iter().filter(|d| d["type"] == "totp").collect();
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["ended_sessions"], 1, "{}", added[0]);
    assert_eq!(added[0]["generation"], 2, "the old field stays");
}

/// OIDC + TOTP sessions end with their TOTP generation (the generation is
/// recorded by the same sign-in path as password + TOTP).
#[sqlx::test]
async fn totp_removal_ends_oidc_totp_sessions(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _, _, pk_sid, pk_token) = factor_account(&app).await;
    let oidc_totp = raw_session(&pool, user, &["oidc", "totp"], false).await;
    let oidc_old = raw_session(&pool, user, &["oidc", "totp"], false).await;
    sqlx::query("UPDATE sessions SET totp_generation = 1 WHERE id = $1")
        .bind(oidc_totp)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE sessions SET totp_generation = 0 WHERE id = $1")
        .bind(oidc_old)
        .execute(&pool)
        .await
        .unwrap();
    grant_step_up(&pool, pk_sid, "totp.remove", "").await;
    let (s, b) = call(&app, Method::POST, "/auth/mfa/disable", &pk_token, None).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(
        ended(&pool, oidc_totp).await,
        (false, Some("factor_ended".into()))
    );
    assert!(live(&pool, oidc_old).await, "another generation");
    let removed = event_details(&pool, user, "authenticator_removed").await;
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["ended_sessions"], 1, "{}", removed[0]);
    assert_eq!(removed[0]["type"], "totp");
}

#[sqlx::test]
async fn password_remove_ends(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, pk_sid, pk_token) = factor_account(&app).await;
    let pw_totp = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    grant_step_up(&pool, pk_sid, "password.remove", "").await;
    let (s, b) = call(&app, Method::DELETE, "/auth/me/password", &pk_token, None).await;
    assert_eq!(s, 204, "{b}");
    assert_eq!(
        ended(&pool, sid_of(&pw_totp)).await,
        (false, Some("factor_ended".into()))
    );
    assert!(live(&pool, pk_sid).await);
    let removed = event_details(&pool, user, "password_removed").await;
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["ended_sessions"], 1, "{}", removed[0]);
}

#[sqlx::test]
async fn compromise_by_admin_ends(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let (admin_sid, admin_token) = create_session_with_token(&pool, &app.config, admin).await;
    let (user, name, totp, pk_sid, _) = factor_account(&app).await;
    let pw_totp = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    grant_step_up(
        &pool,
        admin_sid,
        "admin.factor.set_state",
        &user.to_string(),
    )
    .await;
    let (s, b) = call(
        &app,
        Method::PUT,
        &format!("/api/admin/users/{user}/factors"),
        &admin_token,
        Some(json!({"totp": "compromised"})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(
        ended(&pool, sid_of(&pw_totp)).await,
        (false, Some("factor_ended".into()))
    );
    assert!(live(&pool, pk_sid).await);
    let notice = event_details(&pool, user, "totp_compromised").await;
    assert_eq!(notice, vec![json!({"cause": "admin", "ended_sessions": 1})]);
}

/// "I didn't start this" on a password attempt compromises the password:
/// the sessions that used it end.
#[sqlx::test]
async fn compromise_by_phone_report_ends(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let phone = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pw_totp = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    let (_, step, _) = login_native(&app, &name, PASSWORD).await;
    let (token, bytes) = qr_token(&step["attempt"]);
    let (_, c) = claim(&app, &phone, &token).await;
    let (s, r) = answer_login(&app, &phone, &c, &bytes, Decision::Report).await;
    assert_eq!((s, r["state"].as_str()), (200, Some("reported")), "{r}");
    assert_eq!(
        ended(&pool, sid_of(&pw_totp)).await,
        (false, Some("factor_ended".into()))
    );
    assert!(live(&pool, phone.session_id.unwrap()).await);
    assert_eq!(
        event_details(&pool, user, "password_compromised").await,
        vec![json!({"cause": "report", "ended_sessions": 1})]
    );
}

/// Owner rule part 2 from a chat-only session that used the password: the
/// repudiated sign-in and every other session of that password generation
/// end; the acting session, which just proved a fresh factor, stays.
#[sqlx::test]
async fn part2_repudiation_keeps_acting_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    // No strong authenticator: part 2 applies.
    let acting = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    let bystander = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let intruder = password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    let intruder_sid = sid_of(&intruder);
    let event: Uuid = sqlx::query_scalar(
        "SELECT id FROM security_events WHERE subject_id = $1 AND event_type = 'sign_in'
            AND session_id = $2",
    )
    .bind(user)
    .bind(intruder_sid)
    .fetch_one(&pool)
    .await
    .unwrap();
    forget_totp_step(&app, user).await;
    let (s, b) = call(
        &app,
        Method::POST,
        &format!("/api/me/security-events/{event}/repudiate"),
        acting["access_token"].as_str().unwrap(),
        Some(json!({"part2": {"factor": {"type": "totp",
                                          "code": totp.generate_current().unwrap()}}})),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(
        ended(&pool, intruder_sid).await,
        (false, Some("repudiated".into()))
    );
    assert_eq!(
        ended(&pool, sid_of(&bystander)).await,
        (false, Some("factor_ended".into()))
    );
    assert!(
        live(&pool, sid_of(&acting)).await,
        "the acting session stays"
    );
}

// ============================================================================
// T5: released clients still read the old fields
// ============================================================================

#[sqlx::test]
async fn security_event_list_keeps_its_fields(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, pk_sid, pk_token) = factor_account(&app).await;
    password_totp_pc(&app, user, &name, PASSWORD, &totp).await;
    change_password(&app, pk_sid, &pk_token, NEW_PASSWORD).await;
    let (s, list) = call(
        &app,
        Method::GET,
        "/api/me/security-events",
        &pk_token,
        None,
    )
    .await;
    assert_eq!(s, 200, "{list}");
    let ev = list["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|e| e["event_type"] == "password_changed")
        .expect("password_changed listed")
        .clone();
    for field in [
        "id",
        "event_type",
        "severity",
        "created_at",
        "subject_id",
        "actor",
        "session_id",
        "authenticator",
        "details",
        "actions",
    ] {
        assert!(ev.get(field).is_some(), "{field} in {ev}");
    }
    assert_eq!(ev["details"]["generation"], 2);
    assert_eq!(ev["details"]["ended_sessions"], 1);
}
