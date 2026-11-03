//! Password (and OIDC) sign-in with phone confirmation or TOTP (F3, F4;
//! C27 "Login attempts").
//!
//! Run with: `cargo test --test integration login_password`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::approvals::signing::Decision;

use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, password_user, peer_request};
use super::helpers::{body_to_json_lenient, create_test_user, TestApp};
use super::login_attempts::{
    answer_login, attempt_id, claim, pc_call, qr_token, verifier_pair, Secret,
};

const PASSWORD: &str = "zq8#vT4p-login";

/// `POST /auth/login` from the desktop app with a fresh verifier.
async fn login_native(app: &TestApp, username: &str, password: &str) -> (u16, Value, Secret) {
    let (verifier, challenge) = verifier_pair();
    let req = peer_request(Method::POST, "/auth/login")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "desktop")
        .body(Body::from(
            json!({"username": username, "password": password, "client_challenge": challenge})
                .to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (
        status,
        body_to_json_lenient(resp).await,
        Secret::Verifier(verifier),
    )
}

async fn events(pool: &PgPool, user: Uuid, ty: &str) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT details FROM security_events WHERE subject_id = $1 AND event_type = $2
          ORDER BY created_at",
    )
    .bind(user)
    .bind(ty)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn neutral_answer_is_identical(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (with_totp, totp_name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, with_totp, &totp_name).await;
    let (compromised, compromised_name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, compromised, &compromised_name).await;
    sqlx::query("UPDATE users SET password_state = 'compromised' WHERE id = $1")
        .bind(compromised)
        .execute(&pool)
        .await
        .unwrap();
    let (throttled, throttled_name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, throttled, &throttled_name).await;
    sqlx::query("UPDATE users SET password_state = 'throttled' WHERE id = $1")
        .bind(throttled)
        .execute(&pool)
        .await
        .unwrap();
    let (_, password_only) = password_user(&pool, PASSWORD).await;

    let cases = [
        (totp_name.as_str(), "wrong-password-1"),
        ("no_such_user_xyz", PASSWORD),
        (compromised_name.as_str(), PASSWORD),
        (throttled_name.as_str(), PASSWORD),
        (password_only.as_str(), PASSWORD),
        ("kaiku", PASSWORD),
    ];
    let mut answers = Vec::new();
    for (u, p) in cases {
        let (s, body, _) = login_native(&app, u, p).await;
        answers.push((s, body));
    }
    for (s, body) in &answers {
        assert_eq!(*s, 401, "{body}");
        assert_eq!(body, &answers[0].1, "identical bodies");
        assert_eq!(body["error"], "INVALID_CREDENTIALS");
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM login_attempts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "no attempt for a neutral answer");
}

#[sqlx::test]
async fn mfa_code_is_client_outdated(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (_, name) = password_user(&pool, PASSWORD).await;
    let req = peer_request(Method::POST, "/auth/login")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"username": name, "password": PASSWORD, "mfa_code": "123456"}).to_string(),
        ))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 400);
    assert_eq!(body_to_json_lenient(resp).await["error"], "CLIENT_OUTDATED");
}

#[sqlx::test]
async fn correct_password_with_key_device_needs_confirmation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (s, step, _) = login_native(&app, &name, PASSWORD).await;
    assert_eq!(s, 202, "{step}");
    assert_eq!(step["status"], "confirmation_required");
    assert_eq!(step["attempt"]["kind"], "password");
    assert_eq!(step["attempt"]["totp_available"], true);
    assert!(step["attempt"]["qr"]["payload"].is_string());
    // Never a session.
    assert!(step.get("access_token").is_none());
}

#[sqlx::test]
async fn without_key_device_totp_is_required(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    // A device without `may_approve` does not count.
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    sqlx::query("UPDATE users SET may_approve = false WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, step, _) = login_native(&app, &name, PASSWORD).await;
    assert_eq!(s, 202, "{step}");
    assert_eq!(step["status"], "totp_required");
    assert!(step["attempt"]["qr"].is_null());
}

#[sqlx::test]
async fn scan_then_collect_is_probation_password_phone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let attempt = &step["attempt"];
    let (token, bytes) = qr_token(attempt);
    let (s, c) = claim(&app, &dev, &token).await;
    assert_eq!(s, 201, "{c}");
    assert_eq!(c["variant"], "password");
    let (s, a) = answer_login(&app, &dev, &c, &bytes, Decision::Approve).await;
    assert_eq!(s, 200, "{a}");
    let id = attempt_id(attempt);
    let (s, auth, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/collect"),
        &secret,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    assert_eq!(auth["assurance"], "probation");
    let methods: Vec<String> =
        sqlx::query_scalar("SELECT auth_methods FROM sessions WHERE id = $1::uuid")
            .bind(auth["session_id"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(methods, vec!["password", "phone_confirm"]);
}

#[sqlx::test]
async fn other_account_cannot_claim_a_password_attempt(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (other, _) = create_test_user(&pool).await;
    let stranger = KeyDeviceFixture::insert(&pool, &app.config, other, "active").await;
    let (_, step, _) = login_native(&app, &name, PASSWORD).await;
    let (token, _) = qr_token(&step["attempt"]);
    let (s, body) = claim(&app, &stranger, &token).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (409, Some("LOGIN_ATTEMPT_OTHER_ACCOUNT"))
    );
}

#[sqlx::test]
async fn report_compromises_the_password(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    enable_totp(&pool, &app.config, user, &name).await;
    let g: i64 = sqlx::query_scalar("SELECT password_generation FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (_, step, _) = login_native(&app, &name, PASSWORD).await;
    let (token, bytes) = qr_token(&step["attempt"]);
    let (_, c) = claim(&app, &dev, &token).await;
    let (s, r) = answer_login(&app, &dev, &c, &bytes, Decision::Report).await;
    assert_eq!((s, r["state"].as_str()), (200, Some("reported")), "{r}");
    let (state, g2): (String, i64) =
        sqlx::query_as("SELECT password_state, password_generation FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((state.as_str(), g2), ("compromised", g + 1));
    assert_eq!(
        events(&pool, user, "password_compromised").await,
        vec![json!({"cause": "report"})]
    );
    let (s, body, _) = login_native(&app, &name, PASSWORD).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (401, Some("INVALID_CREDENTIALS"))
    );
}

/// "Phone not at hand?" while a phone holds the claim: the TOTP step voids
/// that claim and signs in.
#[sqlx::test]
async fn phone_not_at_hand_while_claimed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let dev = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    let (token, bytes) = qr_token(&step["attempt"]);
    let (_, c) = claim(&app, &dev, &token).await;
    let code = totp.generate_current().unwrap();
    let (s, auth, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": code}),
    )
    .await;
    assert_eq!(s, 200, "{auth}");
    assert_eq!(auth["assurance"], "probation");
    let methods: Vec<String> =
        sqlx::query_scalar("SELECT auth_methods FROM sessions WHERE id = $1::uuid")
            .bind(auth["session_id"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(methods, vec!["password", "totp"]);
    // The phone's late approval finds nothing to approve.
    let (s, body) = answer_login(&app, &dev, &c, &bytes, Decision::Approve).await;
    assert_ne!(s, 200, "{body}");
}

#[sqlx::test]
async fn totp_needs_active_totp_and_unchanged_generation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;

    // TOTP throttled after the attempt began → 403 TOTP_UNAVAILABLE.
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    sqlx::query("UPDATE users SET totp_state = 'throttled' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, body, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!((s, body["error"].as_str()), (403, Some("TOTP_UNAVAILABLE")));
    sqlx::query("UPDATE users SET totp_state = 'active' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();

    // The password changed since → the attempt has ended.
    sqlx::query("UPDATE users SET password_generation = password_generation + 1 WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, body, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(
        (s, body["error"].as_str()),
        (410, Some("LOGIN_ATTEMPT_ENDED")),
        "{body}"
    );
}

#[sqlx::test]
async fn totp_replay_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    let totp = enable_totp(&pool, &app.config, user, &name).await;
    let code = totp.generate_current().unwrap();
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    let (s, _, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": code}),
    )
    .await;
    assert_eq!(s, 200);
    let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
    let (s, body, _) = pc_call(
        &app,
        &format!("/auth/login-attempts/{id}/totp"),
        &secret,
        json!({"code": code}),
    )
    .await;
    assert_eq!((s, body["error"].as_str()), (401, Some("INVALID_MFA")));
    assert_eq!(
        events(&pool, user, "second_factor_failed").await,
        vec![json!({"factor": "totp", "count": 1})]
    );
}

#[sqlx::test]
async fn unfinished_attempt_notices_coalesce(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    for _ in 0..2 {
        let (_, step, secret) = login_native(&app, &name, PASSWORD).await;
        let id = attempt_id(&step["attempt"]);
        let (s, _, _) = pc_call(
            &app,
            &format!("/auth/login-attempts/{id}/cancel"),
            &secret,
            json!({}),
        )
        .await;
        assert_eq!(s, 204);
        // A second cancel records nothing new.
        pc_call(
            &app,
            &format!("/auth/login-attempts/{id}/cancel"),
            &secret,
            json!({}),
        )
        .await;
    }
    assert_eq!(events(&pool, user, "sign_in_not_finished").await.len(), 2);
    vc_server::security_events::notices::deliver_pending(&app.state)
        .await
        .unwrap();
    let notices: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM security_notices
          WHERE recipient_id = $1 AND event_type = 'sign_in_not_finished'",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(notices, 1, "coalesced into one notice");
}

#[sqlx::test]
async fn password_change_expires_open_attempts(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name) = password_user(&pool, PASSWORD).await;
    enable_totp(&pool, &app.config, user, &name).await;
    let (_, step, _) = login_native(&app, &name, PASSWORD).await;
    let id = attempt_id(&step["attempt"]);
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
    let state: String = sqlx::query_scalar("SELECT state FROM login_attempts WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "expired");
}
