//! Registration tickets and passwordless accounts (F8 "New accounts", C27).
//!
//! Run with: `cargo test --test integration registration_tickets`

use axum::body::Body;
use axum::http::Method;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use super::enrollments::{claim_with, good, Phone};
use super::helpers::login::{peer_request, TEST_ORIGIN};
use super::helpers::passkeys::SoftAuthenticator;
use super::helpers::{body_to_json_lenient, create_test_user, TestApp};

async fn open_registration(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO server_config (key, value) VALUES ('registration_policy', '\"open\"')
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Register natively; returns `(status, body)`.
async fn register(app: &TestApp, body: Value) -> (u16, Value) {
    let req = peer_request(Method::POST, "/auth/register")
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Client", "android")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

async fn with_ticket(
    app: &TestApp,
    method: Method,
    uri: &str,
    ticket: &str,
    body: Value,
) -> (u16, Value) {
    let req = peer_request(method, uri)
        .header("Content-Type", "application/json")
        .header("X-Kaiku-Ticket", ticket)
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    let status = resp.status().as_u16();
    (status, body_to_json_lenient(resp).await)
}

/// A second user first, so the registering account is no first admin.
async fn not_first(pool: &PgPool) {
    create_test_user(pool).await;
}

#[sqlx::test]
async fn register_returns_a_ticket_not_a_session(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;
    let (s, body) = register(
        &app,
        json!({"username": "neu_a", "password": "zq8#vT4p-register"}),
    )
    .await;
    assert_eq!(s, 201, "{body}");
    assert_eq!(body["username"], "neu_a");
    assert_eq!(body["registration_ticket"].as_str().unwrap().len(), 43);
    assert!(body["ticket_expires_at"].is_string());
    assert!(body.get("access_token").is_none());
    let user: Uuid = body["user_id"].as_str().unwrap().parse().unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let (s, st) = with_ticket(
        &app,
        Method::GET,
        "/auth/registration",
        body["registration_ticket"].as_str().unwrap(),
        json!({}),
    )
    .await;
    assert_eq!((s, st["bound"].as_bool()), (200, Some(false)), "{st}");

    // A browser gets the path-scoped cookie instead.
    let req = peer_request(Method::POST, "/auth/register")
        .header("Content-Type", "application/json")
        .body(Body::from(json!({"username": "neu_web"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status().as_u16(), 201);
    let cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|c| c.contains("kaiku_ticket="))
        .expect("ticket cookie");
    assert!(cookie.contains("Path=/auth/registration") && cookie.contains("HttpOnly"));
    assert!(body_to_json_lenient(resp)
        .await
        .get("registration_ticket")
        .is_none());
}

#[sqlx::test]
async fn passwordless_registration_is_allowed(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;
    let (s, body) = register(&app, json!({"username": "ohne_pw"})).await;
    assert_eq!(s, 201, "{body}");
    let hash: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'ohne_pw'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(hash.is_none());
    // A password, when given, passes the policy.
    let (s, body) = register(&app, json!({"username": "mit_pw", "password": "iloveyou"})).await;
    assert_eq!(
        (s, body["error"].as_str(), body["reason"].as_str()),
        (400, Some("PASSWORD_REJECTED"), Some("breached"))
    );
}

#[sqlx::test]
async fn registration_policy_still_applies(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    sqlx::query(
        "INSERT INTO server_config (key, value) VALUES ('registration_policy', '\"closed\"')
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (s, body) = register(&app, json!({"username": "zu_spaet"})).await;
    assert_eq!(
        (s, body["error"].as_str()),
        (403, Some("REGISTRATION_DISABLED"))
    );
}

#[sqlx::test]
async fn ticket_binds_a_passkey_active(pool: PgPool) {
    let app = TestApp::with_passkeys(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;
    let (_, body) = register(&app, json!({"username": "pk_neu"})).await;
    let ticket = body["registration_ticket"].as_str().unwrap().to_string();
    let user: Uuid = body["user_id"].as_str().unwrap().parse().unwrap();
    let (s, start) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/passkey/start",
        &ticket,
        json!({"label": "Laptop"}),
    )
    .await;
    assert_eq!(s, 200, "{start}");
    let mut soft = SoftAuthenticator::new();
    let credential = soft.register(TEST_ORIGIN, &start["options"]);
    let (s, pk) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/passkey/finish",
        &ticket,
        json!({"ceremony_id": start["ceremony_id"], "credential": credential}),
    )
    .await;
    assert_eq!(s, 201, "{pk}");
    let state: String = sqlx::query_scalar("SELECT state FROM passkeys WHERE user_id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
    let (_, st) = with_ticket(&app, Method::GET, "/auth/registration", &ticket, json!({})).await;
    assert_eq!(st["bound"], true);
}

#[sqlx::test]
async fn ticket_binds_a_phone_by_setup_link_or_own_claim(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;

    // A setup link issued by the ticket, confirmed with the code.
    let (_, body) = register(&app, json!({"username": "phone_a"})).await;
    let ticket = body["registration_ticket"].as_str().unwrap().to_string();
    let user: Uuid = body["user_id"].as_str().unwrap().parse().unwrap();
    let (s, issued) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/enrollments",
        &ticket,
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
    let token = issued["setup_token"].as_str().unwrap();
    let phone = Phone::new();
    let (s, claimed) = claim_with(&app, token, &phone, "Pixel 8", good(), "per_use").await;
    assert_eq!(s, 201, "{claimed}");
    let id = issued["enrollment_id"].as_str().unwrap();
    let (s, done) = with_ticket(
        &app,
        Method::POST,
        &format!("/auth/registration/enrollments/{id}/confirm"),
        &ticket,
        json!({"code": claimed["code"]}),
    )
    .await;
    assert_eq!((s, done["state"].as_str()), (200, Some("done")), "{done}");
    let state: String = sqlx::query_scalar("SELECT state FROM approval_devices WHERE user_id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");

    // The Android app's own key: a claim with X-Kaiku-Ticket, done at once.
    let (_, body) = register(&app, json!({"username": "phone_b"})).await;
    let ticket = body["registration_ticket"].as_str().unwrap().to_string();
    let (_, issued) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/enrollments",
        &ticket,
        json!({}),
    )
    .await;
    let token = issued["setup_token"].as_str().unwrap().to_string();
    let phone = Phone::new();
    let own =
        super::enrollments::claim_own(&app, &token, &phone, ("X-Kaiku-Ticket", &ticket)).await;
    assert_eq!(own.0, 201, "{}", own.1);
    assert_eq!(own.1["state"], "done");
    assert_eq!(own.1["authenticator_state"], "active");
    assert!(own.1.get("code").is_none());
    let (_, st) = with_ticket(&app, Method::GET, "/auth/registration", &ticket, json!({})).await;
    assert_eq!(st["bound"], true);
}

#[sqlx::test]
async fn ticket_binds_totp_then_f4_with_bootstrap_open(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;
    let password = "zq8#vT4p-register";
    let (_, body) = register(&app, json!({"username": "totp_neu", "password": password})).await;
    let ticket = body["registration_ticket"].as_str().unwrap().to_string();
    let user: Uuid = body["user_id"].as_str().unwrap().parse().unwrap();
    let (s, setup) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/totp",
        &ticket,
        json!({}),
    )
    .await;
    assert_eq!(s, 200, "{setup}");
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        totp_rs::Secret::Encoded(setup["secret"].as_str().unwrap().to_string())
            .to_bytes()
            .unwrap(),
        Some("Kaiku".into()),
        "totp_neu".into(),
    )
    .unwrap();
    let (s, codes) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/totp/verify",
        &ticket,
        json!({"code": totp.generate_current().unwrap()}),
    )
    .await;
    assert_eq!(s, 200, "{codes}");
    assert_eq!(codes["recovery_codes"].as_array().map(Vec::len), Some(10));
    let open: bool = sqlx::query_scalar("SELECT bootstrap_open FROM users WHERE id = $1")
        .bind(user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(open);
    // Then F4.
    let resp =
        super::helpers::login::f4_sign_in(&app, user, "totp_neu", password, &totp, &[]).await;
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        super::helpers::body_to_json(resp).await["assurance"],
        "probation"
    );
}

#[sqlx::test]
async fn unbound_account_is_deleted_after_30_minutes(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;
    let (_, body) = register(&app, json!({"username": "weg_damit"})).await;
    let user: Uuid = body["user_id"].as_str().unwrap().parse().unwrap();
    let (_, kept) = register(&app, json!({"username": "bleibt"})).await;
    let kept: Uuid = kept["user_id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE registration_tickets SET bound_at = NOW() WHERE user_id = $1")
        .bind(kept)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE registration_tickets SET created_at = created_at - INTERVAL '31 minutes',
                expires_at = expires_at - INTERVAL '31 minutes'",
    )
    .execute(&pool)
    .await
    .unwrap();
    vc_server::auth::timers::run_once(&app.state).await.unwrap();
    let exists = |id: Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert!(!exists(user).await, "unbound account deleted");
    assert!(exists(kept).await, "bound account kept");
}

#[sqlx::test]
async fn first_admin_must_bind_a_strong_authenticator(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    // A fresh server: this account becomes the first admin.
    let (s, body) = register(&app, json!({"username": "erster"})).await;
    assert_eq!(s, 201, "{body}");
    let ticket = body["registration_ticket"].as_str().unwrap().to_string();
    let (s, err) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/totp",
        &ticket,
        json!({}),
    )
    .await;
    assert_eq!((s, err["error"].as_str()), (403, Some("TOTP_UNAVAILABLE")));
    // A key device is fine.
    let (s, issued) = with_ticket(
        &app,
        Method::POST,
        "/auth/registration/enrollments",
        &ticket,
        json!({}),
    )
    .await;
    assert_eq!(s, 201, "{issued}");
}

#[sqlx::test]
async fn expired_ticket_is_refused(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    open_registration(&pool).await;
    not_first(&pool).await;
    let (_, body) = register(&app, json!({"username": "late_one"})).await;
    let ticket = body["registration_ticket"].as_str().unwrap().to_string();
    sqlx::query("UPDATE registration_tickets SET expires_at = NOW() - INTERVAL '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    for (m, uri) in [
        (Method::GET, "/auth/registration"),
        (Method::POST, "/auth/registration/totp"),
        (Method::POST, "/auth/registration/enrollments"),
    ] {
        let (s, b) = with_ticket(&app, m, uri, &ticket, json!({})).await;
        assert_eq!(
            (s, b["error"].as_str()),
            (404, Some("REGISTRATION_TICKET_INVALID")),
            "{uri}: {b}"
        );
    }
}
