//! Self-service recovery (F6) with recovery codes.
//!
//! Run with: `cargo test --test integration recovery_requests`

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::account_requests::{admin_with_device, post, PASSWORD};
use super::enrollments::{claim_own, Phone};
use super::helpers::approvals::KeyDeviceFixture;
use super::helpers::login::{enable_totp, password_user};
use super::helpers::TestApp;

/// An account with a password, TOTP and fresh recovery codes, plus an admin
/// with a key phone as the canceller.
async fn account(app: &TestApp) -> (Uuid, String, totp_rs::TOTP, Vec<String>) {
    admin_with_device(app).await;
    let (user, name) = password_user(&app.pool, PASSWORD).await;
    let totp = enable_totp(&app.pool, &app.config, user, &name).await;
    let ctx = vc_server::auth::ctx::AuthCtx::from(&app.state);
    let mut tx = app.pool.begin().await.unwrap();
    let codes = vc_server::auth::recovery_codes::replace_tx(&mut tx, &ctx, user)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (user, name, totp, codes)
}

async fn start(app: &TestApp, name: &str, code: &str, second: Value, lost: &str) -> (u16, Value) {
    post(
        app,
        "/auth/account-requests",
        None,
        &[("X-Kaiku-Client", "android")],
        json!({"username": name, "purpose": "recovery", "recovery_code": code,
               "second": second, "lost": lost}),
    )
    .await
}

async fn codes_left(pool: &PgPool, user: Uuid) -> i64 {
    let mut conn = pool.acquire().await.unwrap();
    vc_server::auth::recovery_codes::left_conn(&mut conn, user)
        .await
        .unwrap()
}

async fn forget_totp_step(app: &TestApp, user: Uuid) {
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user}"))
        .await;
}

#[sqlx::test]
async fn recovery_code_is_checked_first(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, _) = account(&app).await;
    let (s1, b1) = start(
        &app,
        &name,
        "AAAAA-AAAAA-AAA",
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    let (s2, b2) = start(
        &app,
        "no_such_user_x",
        "AAAAA-AAAAA-AAA",
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(
        (s1, &b1["error"]),
        (401, &json!("INVALID_CREDENTIALS")),
        "{b1}"
    );
    assert_eq!((s1, b1), (s2, b2), "the same answer as an unknown user");
    assert_eq!(codes_left(&pool, user).await, 10);
    let starts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM account_request_starts WHERE user_id = $1")
            .bind(user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(starts, 0, "nothing counted per account");
}

#[sqlx::test]
async fn valid_code_is_consumed_when_the_second_factor_fails(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": "not-the-password"}),
        "none",
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str(), b["recovery_codes_left"].as_i64()),
        (401, Some("REQUEST_FACTOR_INVALID"), Some(9)),
        "{b}"
    );
    assert_eq!(codes_left(&pool, user).await, 9);
}

#[sqlx::test]
async fn totp_counts_in_any_state(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, codes) = account(&app).await;
    sqlx::query("UPDATE users SET totp_state = 'throttled' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    forget_totp_step(&app, user).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "totp", "code": totp.generate_current().unwrap()}),
        "none",
    )
    .await;
    assert_eq!(s, 201, "{b}");
    assert_eq!(b["request"]["purpose"], "recovery");
    assert!(b["request_secret"].as_str().is_some());
}

#[sqlx::test]
async fn password_must_be_active(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    sqlx::query("UPDATE users SET password_state = 'compromised' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("REQUEST_FACTOR_INVALID")),
        "{b}"
    );
}

#[sqlx::test]
async fn start_budget_is_three_per_delay_period(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    for code in &codes[..3] {
        let (s, _) = start(
            &app,
            &name,
            code,
            json!({"type": "password", "password": "wrong-one"}),
            "none",
        )
        .await;
        assert_eq!(s, 401);
    }
    assert_eq!(codes_left(&pool, user).await, 7);
    // The 4th: refused before any code is consumed.
    let (s, b) = start(
        &app,
        &name,
        &codes[3],
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(
        (s, b["error"].as_str()),
        (401, Some("INVALID_CREDENTIALS")),
        "{b}"
    );
    assert_eq!(codes_left(&pool, user).await, 7);
}

#[sqlx::test]
async fn all_key_devices_are_named_lost(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    let a = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let b = KeyDeviceFixture::insert(&pool, &app.config, user, "active").await;
    let (s, body) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": PASSWORD}),
        "all_key_devices",
    )
    .await;
    assert_eq!(s, 201, "{body}");
    assert_eq!(body["request"]["lost_key_devices"], 2);
    let lost: Vec<Uuid> = sqlx::query_scalar(
        "SELECT unnest(lost_device_ids) FROM account_requests WHERE user_id = $1",
    )
    .bind(user)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(lost.contains(&a.row_id) && lost.contains(&b.row_id));
}

#[sqlx::test]
async fn works_while_the_password_path_is_closed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, totp, codes) = account(&app).await;
    sqlx::query("UPDATE users SET password_state = 'throttled' WHERE id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .unwrap();
    forget_totp_step(&app, user).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "totp", "code": totp.generate_current().unwrap()}),
        "none",
    )
    .await;
    assert_eq!(s, 201, "{b}");
}

#[sqlx::test]
async fn full_recovery_to_a_new_phone(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user, name, _, codes) = account(&app).await;
    let (s, b) = start(
        &app,
        &name,
        &codes[0],
        json!({"type": "password", "password": PASSWORD}),
        "none",
    )
    .await;
    assert_eq!(s, 201, "{b}");
    let secret = b["request_secret"].as_str().unwrap().to_string();
    let id: Uuid = b["request"]["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE account_requests SET ready_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    let hdr = [("X-Kaiku-Request-Secret", secret.as_str())];
    let (s, issued) = post(
        &app,
        "/auth/account-requests/current/enrollments",
        None,
        &hdr,
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let phone = Phone::new();
    let (s, own) = claim_own(
        &app,
        issued["setup_token"].as_str().unwrap(),
        &phone,
        ("X-Kaiku-Request-Secret", &secret),
    )
    .await;
    assert_eq!(
        (s, own["authenticator_state"].as_str()),
        (201, Some("pending")),
        "{own}"
    );
    let (s, done) = post(
        &app,
        "/auth/account-requests/current/finish",
        None,
        &hdr,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{done}");
    assert_eq!(done["recovery_codes"].as_array().unwrap().len(), 10);
    // The new phone signs in.
    let (s, c) = post(
        &app,
        "/auth/device-login/challenge",
        None,
        &[],
        json!({"user_id": user, "device_id": phone.device_id}),
    )
    .await;
    assert_eq!(s, 201, "{c}");
}
