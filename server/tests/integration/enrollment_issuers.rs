//! The bearer issuer routes: "Add a phone" (`own`, F5a) from a key device or
//! a PC with a passkey, and `admin` enrolments for GET/DELETE.
//!
//! Run with: `cargo test --test integration enrollment_issuers`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::enrollments::{claim_with, good, Phone};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::{
    body_to_json_lenient, create_session_with_token, create_test_user, grant_step_up, make_admin,
    TestApp,
};

pub(super) async fn call(
    app: &TestApp,
    method: Method,
    uri: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    let req = TestApp::request(method, uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn device_state(pool: &PgPool, phone: &Phone) -> String {
    sqlx::query_scalar("SELECT state FROM approval_devices WHERE device_id = $1")
        .bind(phone.device_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Answer the issuer's `enroll` approval with `code` on `dev`.
pub(super) async fn type_code_on(
    app: &TestApp,
    dev: &KeyDeviceFixture,
    enrollment_id: &str,
    code: &str,
) -> (u16, Value) {
    let (s, c) = call(
        app,
        Method::POST,
        &format!("/auth/enrollments/{enrollment_id}/challenge"),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{c}");
    super::approvals_core::answer(app, dev, &c, code.as_bytes()).await
}

#[sqlx::test]
async fn own_from_a_key_device_needs_device_stepup_and_may_approve(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let sid = dev.session_id.unwrap();
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/enrollments",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("STEP_UP_REQUIRED")),
        "{b}"
    );
    sqlx::query("UPDATE users SET may_approve = false WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    grant_step_up(&pool, sid, "enrollment.issue", "").await;
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/enrollments",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("APPROVAL_NOT_ALLOWED")),
        "{b}"
    );
    sqlx::query("UPDATE users SET may_approve = true WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = call(
        &app,
        Method::POST,
        "/auth/enrollments",
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{b}");
    assert!(b["setup_token"].as_str().is_some());
}

#[sqlx::test]
async fn own_is_active_at_once(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    grant_step_up(&pool, dev.session_id.unwrap(), "enrollment.issue", "").await;
    let (_, issued) = call(
        &app,
        Method::POST,
        "/auth/enrollments",
        &dev.access_token,
        json!({}),
    )
    .await;
    let id = issued["enrollment_id"].as_str().unwrap().to_string();
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
    let (s, got) = call(
        &app,
        Method::GET,
        &format!("/auth/enrollments/{id}"),
        &dev.access_token,
        json!({}),
    )
    .await;
    assert_eq!((s, got["state"].as_str()), (200, Some("claimed")), "{got}");
    let (s, b) = type_code_on(&app, &dev, &id, claimed["code"].as_str().unwrap()).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(device_state(&pool, &phone).await, "active");
}

/// A full PC session signed in with an active passkey.
async fn passkey_pc(pool: &PgPool, app: &TestApp, user: Uuid) -> (Uuid, Uuid, String) {
    let passkey = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, credential, label, state,
                               backup_eligible, backup_state)
         VALUES ($1, $2, $3, '{}'::jsonb, 'Laptop', 'active', false, false)",
    )
    .bind(passkey)
    .bind(user)
    .bind(passkey.as_bytes().to_vec())
    .execute(pool)
    .await
    .unwrap();
    let (sid, token) = create_session_with_token(pool, &app.config, user).await;
    sqlx::query(
        "UPDATE sessions SET auth_methods = '{passkey}', client_class = 'desktop',
                authenticator_type = 'passkey', authenticator_id = $2
          WHERE id = $1",
    )
    .bind(sid)
    .bind(passkey)
    .execute(pool)
    .await
    .unwrap();
    (sid, passkey, token)
}

#[sqlx::test]
async fn own_from_a_pc_rechecks_session_and_passkey_at_confirm(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (sid, passkey, token) = passkey_pc(&pool, &app, user).await;

    // Confirmed by the issuing PC with the code.
    grant_step_up(&pool, sid, "enrollment.issue", "").await;
    let (s, issued) = call(&app, Method::POST, "/auth/enrollments", &token, json!({})).await;
    assert_eq!(s, 201, "{issued}");
    let (kind, rec): (String, Option<Uuid>) = sqlx::query_as(
        "SELECT issuer_kind, issuer_passkey_id FROM enrollments WHERE id = $1::uuid",
    )
    .bind(issued["enrollment_id"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((kind.as_str(), rec), ("session", Some(passkey)));
    let phone = Phone::new();
    let (_, claimed) = claim_with(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    let id = issued["enrollment_id"].as_str().unwrap();
    let (s, b) = call(
        &app,
        Method::POST,
        &format!("/auth/enrollments/{id}/confirm"),
        &token,
        json!({"code": claimed["code"]}),
    )
    .await;
    assert_eq!((s, b["state"].as_str()), (200, Some("done")), "{b}");
    assert_eq!(device_state(&pool, &phone).await, "active");

    // The recorded passkey revoked before the confirm: the enrolment ends.
    grant_step_up(&pool, sid, "enrollment.issue", "").await;
    let (_, issued) = call(&app, Method::POST, "/auth/enrollments", &token, json!({})).await;
    let phone = Phone::new();
    let (_, claimed) = claim_with(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        "Pixel 9",
        good(),
        "per_use",
    )
    .await;
    sqlx::query("UPDATE passkeys SET state = 'revoked', revoked_at = NOW() WHERE id = $1")
        .bind(passkey)
        .execute(&pool)
        .await
        .unwrap();
    let id = issued["enrollment_id"].as_str().unwrap();
    let (s, b) = call(
        &app,
        Method::POST,
        &format!("/auth/enrollments/{id}/confirm"),
        &token,
        json!({"code": claimed["code"]}),
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (409, Some("ENROLLMENT_STATE")),
        "{b}"
    );
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");

    // Ending the issuing session expires its open enrolments.
    let (sid2, _, token2) = passkey_pc(&pool, &app, user).await;
    grant_step_up(&pool, sid2, "enrollment.issue", "").await;
    let (s, issued) = call(&app, Method::POST, "/auth/enrollments", &token2, json!({})).await;
    assert_eq!(s, 201, "{issued}");
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = pool.begin().await.unwrap();
    vc_server::approvals::transition::lock_accounts(&mut tx, &[user])
        .await
        .unwrap();
    vc_server::auth::revocation::end_sessions_tx(
        &mut tx,
        &ctx,
        user,
        &[sid2],
        vc_server::auth::revocation::EndReason::Revoked,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1::uuid")
        .bind(issued["enrollment_id"].as_str().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");
}

#[sqlx::test]
async fn any_admin_can_cancel_an_admin_enrolment(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (a, _) = create_test_user(&pool).await;
    make_admin(&pool, a).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, a, "active").await;
    let (target, _) = create_test_user(&pool).await;
    grant_step_up(
        &pool,
        adev.session_id.unwrap(),
        "admin.enrollment.issue",
        &target.to_string(),
    )
    .await;
    let (s, issued) = call(
        &app,
        Method::POST,
        &format!("/api/admin/users/{target}/enrollments"),
        &adev.access_token,
        json!({"mode": "setup", "claimant_check": {"method": "in_person", "note": "at home"}}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let id = issued["enrollment_id"].as_str().unwrap();
    // A member who is no admin sees nothing.
    let (member, _) = create_test_user(&pool).await;
    let (_, member_token) = create_session_with_token(&pool, &app.config, member).await;
    let (s, _) = call(
        &app,
        Method::DELETE,
        &format!("/auth/enrollments/{id}"),
        &member_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 404);
    // Another admin cancels it.
    let (b, _) = create_test_user(&pool).await;
    make_admin(&pool, b).await;
    let (_, b_token) = create_session_with_token(&pool, &app.config, b).await;
    let (s, body) = call(
        &app,
        Method::DELETE,
        &format!("/auth/enrollments/{id}"),
        &b_token,
        json!({}),
    )
    .await;
    assert_eq!(s, 204, "{body}");
    let state: String = sqlx::query_scalar("SELECT state FROM enrollments WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "cancelled");
}

/// `POST` without a bearer token (the browser on a setup link).
async fn browser(
    app: &TestApp,
    uri: &str,
    cookie: Option<&str>,
    body: Value,
) -> (u16, Value, Option<String>) {
    let mut req = super::helpers::login::peer_request(Method::POST, uri)
        .header("Content-Type", "application/json");
    if let Some(c) = cookie {
        req = req.header("Cookie", c);
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    let status = resp.status().as_u16();
    let set_cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.contains("kaiku_enroll"))
        .map(|c| c.split(';').next().unwrap_or_default().to_string());
    (status, body_to_json_lenient(resp).await, set_cookie)
}

#[sqlx::test]
async fn browser_setup_link_registers_a_passkey(pool: PgPool) {
    use super::helpers::login::TEST_ORIGIN;
    use super::helpers::passkeys::SoftAuthenticator;
    let app = TestApp::with_passkeys(pool.clone()).await;
    let (admin, _) = create_test_user(&pool).await;
    make_admin(&pool, admin).await;
    let adev = KeyDeviceFixture::insert(&pool, &app.config, admin, "active").await;
    let (oma, oma_name) = create_test_user(&pool).await;
    grant_step_up(
        &pool,
        adev.session_id.unwrap(),
        "admin.enrollment.issue",
        &oma.to_string(),
    )
    .await;
    let (s, issued) = call(
        &app,
        Method::POST,
        &format!("/api/admin/users/{oma}/enrollments"),
        &adev.access_token,
        json!({"mode": "setup", "claimant_check": {"method": "in_person", "note": "at home"}}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let token = issued["setup_token"].as_str().unwrap();

    let (s, start, _) = browser(
        &app,
        "/auth/enrollments/passkey/start",
        None,
        json!({"setup_token": token}),
    )
    .await;
    assert_eq!(s, 200, "{start}");
    assert_eq!(start["subject"]["username"], oma_name.as_str());
    assert!(start["server_name"].as_str().is_some());
    let mut soft = SoftAuthenticator::new();
    let credential = soft.register(TEST_ORIGIN, &start["options"]);
    let (s, claimed, cookie) = browser(
        &app,
        "/auth/enrollments/passkey/finish",
        None,
        json!({"setup_token": token, "ceremony_id": start["ceremony_id"],
               "credential": credential, "label": null}),
    )
    .await;
    assert_eq!(s, 201, "{claimed}");
    assert_eq!(claimed["state"], "claimed");
    assert_eq!(claimed["code"].as_str().unwrap().len(), 6);
    let cookie = cookie.expect("the claim-secret cookie");
    // No passkey row before `done`.
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1")
        .bind(oma)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    // The browser long-polls with its cookie.
    let id = claimed["enrollment_id"].as_str().unwrap();
    let (s, st, _) = browser(
        &app,
        "/auth/enrollments/status",
        Some(&cookie),
        json!({"enrollment_id": id, "wait": 0}),
    )
    .await;
    assert_eq!((s, st["state"].as_str()), (200, Some("claimed")), "{st}");
    // The admin types the code on their phone.
    let (s, b) = type_code_on(&app, &adev, id, claimed["code"].as_str().unwrap()).await;
    assert_eq!(s, 200, "{b}");
    let (state, enrollment): (String, Option<Uuid>) =
        sqlx::query_as("SELECT state, enrollment_id FROM passkeys WHERE user_id = $1")
            .bind(oma)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "active", "a new account is active at once");
    assert_eq!(enrollment.map(|e| e.to_string()).as_deref(), Some(id));
    let (_, st, _) = browser(
        &app,
        "/auth/enrollments/status",
        Some(&cookie),
        json!({"enrollment_id": id, "wait": 0}),
    )
    .await;
    assert_eq!(
        (st["state"].as_str(), st["authenticator_state"].as_str()),
        (Some("done"), Some("active")),
        "{st}"
    );
}

#[sqlx::test]
async fn probation_sessions_cannot_add_a_phone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, _) = create_test_user(&pool).await;
    let (_, token) =
        super::helpers::create_probation_session_with_token(&pool, &app.config, user).await;
    let (s, b) = call(&app, Method::POST, "/auth/enrollments", &token, json!({})).await;
    assert_eq!(
        (s, b["error"].as_str()),
        (403, Some("FULL_SESSION_REQUIRED")),
        "{b}"
    );
}
