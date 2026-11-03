//! "That wasn't me" (B.2 repudiation table, C28).
//!
//! Run with: `cargo test --test integration repudiation`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::auth::strong::AuthenticatorRef;
use vc_server::security_events::{record, Actor, EventType, NewSecurityEvent};

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::enable_totp;
use super::helpers::{
    body_to_json_lenient, create_probation_session_with_token, create_test_user, grant_step_up,
    insert_session_row, TestApp,
};

async fn call(app: &TestApp, uri: &str, token: &str, body: Value) -> (u16, Value) {
    let req = TestApp::request(Method::POST, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn get(app: &TestApp, uri: &str, token: &str) -> Value {
    let req = TestApp::request(Method::GET, uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    body_to_json_lenient(app.oneshot(req).await).await
}

/// Record an event now (a few ms after anything created before).
async fn event(
    pool: &PgPool,
    user: Uuid,
    t: EventType,
    session: Option<Uuid>,
    a: Option<AuthenticatorRef>,
    details: Value,
) -> Uuid {
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let mut conn = pool.acquire().await.unwrap();
    let mut ev = NewSecurityEvent::new(user, t, Actor::User(user), details);
    ev.session_id = session;
    ev.authenticator = a;
    record(&mut conn, ev).await.unwrap()
}

async fn repudiate(app: &TestApp, dev: &KeyDeviceFixture, id: Uuid) -> (u16, Value) {
    grant_step_up(
        &app.pool,
        dev.session_id.unwrap(),
        "event.repudiate",
        &id.to_string(),
    )
    .await;
    call(
        app,
        &format!("/api/me/security-events/{id}/repudiate"),
        &dev.access_token,
        json!({}),
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

async fn device_state(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT state FROM approval_devices WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
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

#[sqlx::test]
async fn repudiating_a_password_sign_in_ends_it_and_compromises(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, user, false).await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "totp"], "assurance": "probation"}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["ended_session_ids"], json!([pc]));
    assert_eq!(b["compromised"], json!(["password", "totp"]));
    assert_eq!(b["revoked_authenticator_ids"], json!([]));
    assert!(b["event"]["repudiated_at"].as_str().is_some());
    assert!(!session_live(&pool, pc).await);
    let (pw, totp): (String, String) =
        sqlx::query_as("SELECT password_state, totp_state FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((pw.as_str(), totp.as_str()), ("compromised", "compromised"));
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM security_events WHERE subject_id = $1 AND event_type = 'repudiated'",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(kinds.len(), 1);
}

#[sqlx::test]
async fn repudiating_a_passkey_sign_in_revokes_that_passkey(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let passkey = insert_passkey(&pool, user).await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        None,
        Some(AuthenticatorRef::passkey(passkey)),
        json!({"auth_methods": ["passkey"], "assurance": "full"}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["revoked_authenticator_ids"], json!([passkey]));
    let state: String = sqlx::query_scalar("SELECT state FROM passkeys WHERE id = $1")
        .bind(passkey)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "revoked");
}

#[sqlx::test]
async fn relayed_qr_approver_is_not_revoked(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let approver = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, user, false).await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        Some(AuthenticatorRef::key_device(approver.row_id)),
        json!({"auth_methods": ["qr"], "assurance": "probation"}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert!(!session_live(&pool, pc).await);
    assert_eq!(device_state(&pool, approver.row_id).await, "active");
}

#[sqlx::test]
async fn dictated_enrolment_revokes_only_the_new_device(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let new = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let id = event(
        &pool,
        user,
        EventType::AuthenticatorAdded,
        None,
        Some(AuthenticatorRef::key_device(new.row_id)),
        json!({"type": "key_device"}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["revoked_authenticator_ids"], json!([new.row_id]));
    assert_eq!(device_state(&pool, new.row_id).await, "revoked");
    assert_eq!(device_state(&pool, dev.row_id).await, "active");
}

#[sqlx::test]
async fn part1_needs_an_older_strong_authenticator(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, user, false).await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "totp"]}),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let younger = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, b) = repudiate(&app, &younger, id).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("REPUDIATE_NOT_ALLOWED")),
        "{b}"
    );
    assert!(session_live(&pool, pc).await);
    // The event's own device cannot vouch for it either.
    let own = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        own.session_id,
        Some(AuthenticatorRef::key_device(own.row_id)),
        json!({"auth_methods": ["device_key"]}),
    )
    .await;
    let (s, _) = repudiate(&app, &own, id).await;
    assert_eq!(s, 403);
}

#[sqlx::test]
async fn part2_needs_no_strong_authenticator_an_older_session_and_a_fresh_factor(pool: PgPool) {
    use fred::interfaces::KeysInterface;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (_, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, user, false).await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "phone_confirm"]}),
    )
    .await;
    let uri = format!("/api/me/security-events/{id}/repudiate");
    // A wrong code.
    let (s, b) = call(
        &app,
        &uri,
        &token,
        json!({"part2": {"factor": {"type": "totp", "code": "000000"}}}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (401, Some("INVALID_MFA")), "{b}");
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
    let (s, b) = call(
        &app,
        &uri,
        &token,
        json!({"part2": {"factor": {"type": "totp", "code": totp.generate_current().unwrap()}}}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert!(!session_live(&pool, pc).await);
    // A session younger than the event cannot use part 2.
    let id = event(
        &pool,
        user,
        EventType::PasswordChanged,
        None,
        None,
        json!({}),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let (_, young) = create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = call(
        &app,
        &format!("/api/me/security-events/{id}/repudiate"),
        &young,
        json!({"part2": {"factor": {"type": "recovery_code", "code": "AAAAA-AAAAA-AAA"}}}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("REPUDIATE_NOT_ALLOWED")),
        "{b}"
    );
}

#[sqlx::test]
async fn part2_never_revokes(pool: PgPool) {
    use fred::interfaces::KeysInterface;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (_, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    // A pending device (no active strong authenticator: part 2 applies).
    let pending = KeyDeviceFixture::insert(&pool, &app.config, user, "pending").await;
    let id = event(
        &pool,
        user,
        EventType::AuthenticatorAdded,
        None,
        Some(AuthenticatorRef::key_device(pending.row_id)),
        json!({"type": "key_device"}),
    )
    .await;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
    let (s, b) = call(
        &app,
        &format!("/api/me/security-events/{id}/repudiate"),
        &token,
        json!({"part2": {"factor": {"type": "totp", "code": totp.generate_current().unwrap()}}}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["revoked_authenticator_ids"], json!([]));
    assert_eq!(device_state(&pool, pending.row_id).await, "pending");
}

#[sqlx::test]
async fn second_repudiation_conflicts(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let id = event(
        &pool,
        user,
        EventType::PasswordChanged,
        None,
        None,
        json!({}),
    )
    .await;
    assert_eq!(repudiate(&app, &dev, id).await.0, 200);
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("EVENT_ALREADY_REPUDIATED")),
        "{b}"
    );
}

#[sqlx::test]
async fn actions_only_where_usable(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    // The account's only phone signs in: nothing older vouches, and part 2
    // is closed (a strong authenticator exists).
    let only = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        only.session_id,
        Some(AuthenticatorRef::key_device(only.row_id)),
        json!({"auth_methods": ["device_key"]}),
    )
    .await;
    let page = get(&app, "/api/me/security-events", &only.access_token).await;
    let ev = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id.to_string())
        .unwrap()
        .clone();
    assert!(
        !ev["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["action"] == "repudiate"),
        "{ev}"
    );
    // A password change after the phone: repudiable by step-up.
    let id = event(
        &pool,
        user,
        EventType::PasswordChanged,
        None,
        None,
        json!({}),
    )
    .await;
    let page = get(&app, "/api/me/security-events", &only.access_token).await;
    let ev = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id.to_string())
        .unwrap()
        .clone();
    let a = ev["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["action"] == "repudiate")
        .cloned()
        .unwrap_or_else(|| panic!("{ev}"));
    assert_eq!(a["via"], "step_up");
}

#[sqlx::test]
async fn warning_before_voiding_the_own_bootstrap(pool: PgPool) {
    use super::account_requests::{admin_with_device, bootstrap_user, start_bootstrap};
    let app = TestApp::with_pool(pool.clone()).await;
    admin_with_device(&app).await;
    let (user, name, totp) = bootstrap_user(&app).await;
    let (_, token) = create_probation_session_with_token(&pool, &app.config, user).await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, user, false).await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "totp"]}),
    )
    .await;
    let (s, started) = start_bootstrap(&app, user, &name, &totp).await;
    assert_eq!(s, 201, "{started}");
    let page = get(&app, "/api/me/security-events", &token).await;
    let ev = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id.to_string())
        .unwrap()
        .clone();
    let a = ev["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["action"] == "repudiate")
        .cloned()
        .unwrap_or_else(|| panic!("{ev}"));
    assert_eq!(
        (a["via"].as_str(), a["warning"].as_str()),
        (Some("part2"), Some("voids_own_bootstrap"))
    );
    // Repudiating anyway voids the request (B.6.13).
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
    let (s, b) = call(
        &app,
        &format!("/api/me/security-events/{id}/repudiate"),
        &token,
        json!({"part2": {"factor": {"type": "totp", "code": totp.generate_current().unwrap()}}}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["voided_request_ids"], json!([started["request"]["id"]]));
}
