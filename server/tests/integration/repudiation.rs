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
        json!({"auth_methods": ["password", "totp"], "assurance": "probation",
               "factors": [{"factor": "password", "generation": 1},
                           {"factor": "totp", "generation": 1}]}),
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
    let (pw_gen, totp_gen): (i64, i64) =
        sqlx::query_as("SELECT password_generation, totp_generation FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "totp"],
               "factors": [{"factor": "password", "generation": pw_gen},
                           {"factor": "totp", "generation": totp_gen}]}),
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
async fn a_sign_in_records_the_generations_it_used(pool: PgPool) {
    use super::helpers::login::password_user;
    use super::login_attempts::{attempt_id, pc_call};
    use super::login_password::login_native;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, "zq8#vT4p-repud").await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    // Renew TOTP once so the generations differ.
    sqlx::query("UPDATE users SET totp_generation = 3 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, step, secret) = login_native(&app, &name, "zq8#vT4p-repud").await;
    assert_eq!(s, 202, "{step}");
    let id = attempt_id(&step["attempt"]);
    let (s, b, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let details: Value = sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = 'sign_in'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        details["factors"],
        json!([{"factor": "password", "generation": 1}, {"factor": "totp", "generation": 3}]),
        "{details}"
    );
}

#[sqlx::test]
async fn repudiation_compromises_exactly_the_recorded_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let pc = Uuid::new_v4();
    insert_session_row(&pool, pc, user, false).await;
    // Only the password was used (with a phone confirmation).
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        Some(pc),
        None,
        json!({"auth_methods": ["password", "phone_confirm"],
               "factors": [{"factor": "password", "generation": 1}]}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["compromised"], json!(["password"]));
    let (pw_gen, totp_gen, pw, totp) = generations(&pool, user).await;
    assert_eq!((pw.as_str(), totp.as_str()), ("compromised", "active"));
    assert_eq!((pw_gen, totp_gen), (2, 1));
}

#[sqlx::test]
async fn a_factor_replaced_since_is_left_alone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        None,
        None,
        json!({"auth_methods": ["password", "totp"],
               "factors": [{"factor": "password", "generation": 1},
                           {"factor": "totp", "generation": 1}]}),
    )
    .await;
    // Both were replaced after the sign-in.
    sqlx::query("UPDATE users SET password_generation = 2, totp_generation = 2 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["compromised"], json!([]));
    assert!(
        b["event"]["repudiated_at"].as_str().is_some(),
        "still recorded"
    );
    let (pw_gen, totp_gen, pw, totp) = generations(&pool, user).await;
    assert_eq!((pw_gen, totp_gen), (2, 2));
    assert_eq!((pw.as_str(), totp.as_str()), ("active", "active"));
    // A sign-in event without recorded generations compromises nothing.
    let id = event(
        &pool,
        user,
        EventType::SignIn,
        None,
        None,
        json!({"auth_methods": ["password", "totp"]}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!((s, &b["compromised"]), (200, &json!([])), "{b}");
}

#[sqlx::test]
async fn an_unfinished_sign_in_compromises_its_password(pool: PgPool) {
    use super::helpers::login::password_user;
    use super::login_attempts::{attempt_id, pc_call};
    use super::login_password::login_native;
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, "zq8#vT4p-unfin").await;
    enable_totp(&pool, &app.config, user, &name).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    // A correct password, then the PC gives up: the real notice.
    let (s, step, secret) = login_native(&app, &name, "zq8#vT4p-unfin").await;
    assert_eq!(s, 202, "{step}");
    let id = attempt_id(&step["attempt"]);
    let (s, _, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/cancel"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 204);
    let (event, details): (Uuid, Value) = sqlx::query_as(
        "SELECT id, details FROM security_events
          WHERE subject_id = $1 AND event_type = 'sign_in_not_finished'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        details["factors"],
        json!([{"factor": "password", "generation": 1}]),
        "{details}"
    );
    let (s, b) = repudiate(&app, &dev, event).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["compromised"], json!(["password"]));
    assert_eq!(b["revoked_authenticator_ids"], json!([]));
}

#[sqlx::test]
async fn a_step_up_action_ends_its_session_and_revokes_nothing(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    // An older device reports a step-up made by a newer one in its session.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let other = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let id = event(
        &pool,
        user,
        EventType::StepUpAction,
        other.session_id,
        Some(AuthenticatorRef::key_device(other.row_id)),
        json!({"action": "sessions.end", "target_label": "all other sessions"}),
    )
    .await;
    let (s, b) = repudiate(&app, &dev, id).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["ended_session_ids"], json!([other.session_id.unwrap()]));
    assert_eq!(
        b["revoked_authenticator_ids"],
        json!([]),
        "the authoriser is not revoked"
    );
    assert_eq!(device_state(&pool, other.row_id).await, "active");
    // Repudiation negatives (Fable M4).
    let (s, b) = call(
        &app,
        &format!("/api/me/security-events/{}/repudiate", Uuid::new_v4()),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!((s, b["error"].as_str()), (404, Some("NOT_FOUND")), "{b}");
    let ev = event(
        &pool,
        user,
        EventType::BootstrapReminder,
        None,
        None,
        json!({}),
    )
    .await;
    grant_step_up(
        &pool,
        dev.session_id.unwrap(),
        "event.repudiate",
        &ev.to_string(),
    )
    .await;
    let (s, b) = call(
        &app,
        &format!("/api/me/security-events/{ev}/repudiate"),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("REPUDIATE_NOT_ALLOWED")),
        "{b}"
    );
}

#[sqlx::test]
async fn a_password_change_compromises_only_the_generation_it_made(pool: PgPool) {
    use super::helpers::{create_session_with_token, make_session_full};
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = create_test_user(&pool).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    // A real change records the generation it made.
    let (sid, token) = create_session_with_token(&pool, &app.config, user).await;
    make_session_full(&pool, sid).await;
    grant_step_up(&pool, sid, "password.set", "").await;
    let (s, b) = call(
        &app,
        "/auth/me/password",
        &token,
        json!({"new_password": "a perfectly fine passphrase 7"}),
    )
    .await;
    assert_eq!(s, 200, "{b}");
    let (changed, details): (Uuid, Value) = sqlx::query_as(
        "SELECT id, details FROM security_events
          WHERE subject_id = $1 AND event_type IN ('password_set', 'password_changed')",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    let made = details["generation"].as_i64().expect("generation recorded");
    // Replaced again since: repudiating the old change leaves the new one.
    sqlx::query("UPDATE users SET password_generation = $2 WHERE id = $1")
        .bind(user)
        .bind(made + 1)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = repudiate(&app, &dev, changed).await;
    assert_eq!((s, &b["compromised"]), (200, &json!([])), "{b}");
    let (_, _, pw, _) = generations(&pool, user).await;
    assert_eq!(pw, "active");
}
