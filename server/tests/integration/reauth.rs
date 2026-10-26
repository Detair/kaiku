//! Re-authentication for sensitive actions (C24).
//!
//! `POST /api/admin/elevate` asks for the password (and a TOTP code when MFA
//! is on), `POST /auth/mfa/backup-codes` asks for a current TOTP code, an
//! elevation belongs to the access token's session, and failed attempts on
//! these routes and on `POST /auth/me/password` are throttled.
//!
//! Each test runs a real HTTP server (handlers read the peer address) on a
//! fresh per-test database.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::config::Config;
use vc_server::ratelimit::{RateLimitConfig, RateLimiter, RateLimits};
use vc_server::voice::sfu::SfuServer;

use super::helpers::{
    create_session_with_token, make_admin, spawn_test_server, TestApp, TestServer,
};

const PASSWORD: &str = "Correct-Horse-42";

/// A server without rate limiting.
async fn plain_server(pool: &PgPool) -> (TestServer, Arc<Config>, AppState) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    (server, app.config, app.state)
}

/// A server with the default rate limits and failed-attempt blocking.
async fn rate_limited_server(pool: &PgPool) -> (TestServer, Arc<Config>) {
    let config = Config::default_for_test();
    let redis = vc_server::db::create_redis_client(&config.redis_url)
        .await
        .expect("Failed to connect to test Redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");
    let mut limiter = RateLimiter::new(
        redis.clone(),
        RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
            fail_open: false,
            trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
            allowlist: HashSet::new(),
            limits: RateLimits::default(),
        },
    );
    limiter.init().await.expect("Failed to initialize limiter");

    let state = AppState::new(AppStateConfig {
        db: pool.clone(),
        redis,
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter: Some(limiter),
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    });
    let server = spawn_test_server(create_router(state)).await;
    (server, Arc::new(config))
}

/// Create a local user whose password is [`PASSWORD`]; returns `(id, username)`.
async fn create_user_with_password(pool: &PgPool) -> (Uuid, String) {
    let username = format!("reauth_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let hash = vc_server::auth::hash_password(PASSWORD).expect("hash password");
    let user = vc_server::db::create_user(pool, &username, "Reauth User", None, &hash)
        .await
        .expect("create user");
    (user.id, username)
}

/// Turn MFA on for the user; returns the TOTP generator for its secret.
async fn enable_mfa(pool: &PgPool, config: &Config, user_id: Uuid, username: &str) -> TOTP {
    let secret = Secret::default();
    let secret_str = secret.to_encoded().to_string();
    let key = hex::decode(config.mfa_encryption_key.as_ref().expect("mfa key")).expect("hex key");
    let encrypted =
        vc_server::auth::mfa_crypto::encrypt_mfa_secret(&secret_str, &key).expect("encrypt");
    sqlx::query("UPDATE users SET mfa_secret = $1 WHERE id = $2")
        .bind(&encrypted)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("enable mfa");

    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret_str)
            .to_bytes()
            .expect("secret bytes"),
        Some("Kaiku".to_string()),
        username.to_string(),
    )
    .expect("totp")
}

/// A six-digit code that is not valid in any accepted TOTP step right now.
fn wrong_code(totp: &TOTP) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let valid: Vec<String> = [now - 60, now - 30, now, now + 30, now + 60]
        .iter()
        .map(|t| totp.generate(*t))
        .collect();
    (0..1_000_000u32)
        .map(|n| format!("{n:06}"))
        .find(|c| !valid.contains(c))
        .expect("a wrong code exists")
}

/// The code of the next 30-second step (valid now within the ±1 window).
fn next_step_code(totp: &TOTP) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    totp.generate(now + 30)
}

/// `details.method` of the user's latest elevation audit entry.
async fn last_elevation_method(pool: &PgPool, user_id: Uuid) -> Option<String> {
    sqlx::query_scalar(
        "SELECT details->>'method' FROM system_audit_log
         WHERE actor_id = $1 AND action = 'admin.session.elevated'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .expect("audit query")
    .flatten()
}

async fn post_json(
    server: &TestServer,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut req = client
        .post(format!("{}{path}", server.url))
        .bearer_auth(token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.expect("request");
    let status = resp.status().as_u16();
    let json = resp.json::<Value>().await.unwrap_or(Value::Null);
    (status, json)
}

async fn get_json(server: &TestServer, path: &str, token: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .get(format!("{}{path}", server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("request");
    let status = resp.status().as_u16();
    let json = resp.json::<Value>().await.unwrap_or(Value::Null);
    (status, json)
}

#[sqlx::test]
async fn test_elevate_requires_password(pool: PgPool) {
    let (server, config, _) = plain_server(&pool).await;
    let (admin, _) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let (_, token) = create_session_with_token(&pool, &config, admin).await;

    let (status, body) = post_json(&server, "/api/admin/elevate", &token, Some(json!({}))).await;
    assert_eq!(status, 400, "missing password: {body}");
    assert_eq!(body["error"], "VALIDATION_ERROR");

    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": "wrong-password" })),
    )
    .await;
    assert_eq!(status, 401, "wrong password: {body}");
    assert_eq!(body["error"], "INVALID_CREDENTIALS");

    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "reason": "test" })),
    )
    .await;
    assert_eq!(status, 200, "right password: {body}");
    assert_eq!(body["elevated"], true);
}

#[sqlx::test]
async fn test_elevate_requires_totp_when_mfa_enabled(pool: PgPool) {
    let (server, config, _) = plain_server(&pool).await;
    let (admin, username) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let totp = enable_mfa(&pool, &config, admin, &username).await;
    let (_, token) = create_session_with_token(&pool, &config, admin).await;

    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, 400, "no code: {body}");
    assert_eq!(body["error"], "MFA_CODE_REQUIRED");

    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "mfa_code": wrong_code(&totp) })),
    )
    .await;
    assert_eq!(status, 401, "wrong code: {body}");
    assert_eq!(body["error"], "INVALID_CREDENTIALS");

    let code = totp.generate_current().expect("code");
    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "mfa_code": code })),
    )
    .await;
    assert_eq!(status, 200, "valid code: {body}");
}

#[sqlx::test]
async fn test_elevation_is_bound_to_session(pool: PgPool) {
    let (server, config, state) = plain_server(&pool).await;
    let (admin, _) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let (session_a, token_a) = create_session_with_token(&pool, &config, admin).await;
    let (session_b, token_b) = create_session_with_token(&pool, &config, admin).await;

    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token_a,
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, 200, "elevate on A: {body}");

    // `GET /api/admin/users` is open to non-elevated admins; the report list
    // is an elevated route.
    let (status, body) = get_json(&server, "/api/admin/reports", &token_b).await;
    assert_eq!(status, 403, "session B must not be elevated: {body}");
    assert_eq!(body["error"], "elevation_required");

    let (status, body) = get_json(&server, "/api/admin/reports", &token_a).await;
    assert_eq!(status, 200, "session A is elevated: {body}");

    let (_, status_b) = get_json(&server, "/api/admin/status", &token_b).await;
    assert_eq!(status_b["is_elevated"], false);
    let (_, status_a) = get_json(&server, "/api/admin/status", &token_a).await;
    assert_eq!(status_a["is_elevated"], true);

    // The WebSocket admin-event check follows the same session binding.
    assert!(vc_server::admin::is_elevated_admin(&state.redis, &pool, admin, session_a).await);
    assert!(!vc_server::admin::is_elevated_admin(&state.redis, &pool, admin, session_b).await);

    // De-elevating ends the elevation (and its cached status) for A.
    let resp = reqwest::Client::new()
        .delete(format!("{}/api/admin/elevate", server.url))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);
    let (status, _) = get_json(&server, "/api/admin/reports", &token_a).await;
    assert_eq!(status, 403);
    assert!(!vc_server::admin::is_elevated_admin(&state.redis, &pool, admin, session_a).await);
}

/// `DELETE /api/admin/elevate` ends only the caller's elevation; another
/// elevated session of the same admin stays elevated.
#[sqlx::test]
async fn test_de_elevation_is_bound_to_session(pool: PgPool) {
    let (server, config, state) = plain_server(&pool).await;
    let (admin, _) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let (session_a, token_a) = create_session_with_token(&pool, &config, admin).await;
    let (session_b, token_b) = create_session_with_token(&pool, &config, admin).await;
    for token in [&token_a, &token_b] {
        let (status, body) = post_json(
            &server,
            "/api/admin/elevate",
            token,
            Some(json!({ "password": PASSWORD })),
        )
        .await;
        assert_eq!(status, 200, "elevate: {body}");
    }

    let resp = reqwest::Client::new()
        .delete(format!("{}/api/admin/elevate", server.url))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);

    let (status, _) = get_json(&server, "/api/admin/reports", &token_a).await;
    assert_eq!(status, 403, "session A is no longer elevated");
    let (status, body) = get_json(&server, "/api/admin/reports", &token_b).await;
    assert_eq!(status, 200, "session B stays elevated: {body}");
    assert!(!vc_server::admin::is_elevated_admin(&state.redis, &pool, admin, session_a).await);
    assert!(vc_server::admin::is_elevated_admin(&state.redis, &pool, admin, session_b).await);
}

#[sqlx::test]
async fn test_backup_code_regeneration_requires_totp(pool: PgPool) {
    let (server, config, _) = plain_server(&pool).await;
    let (user, username) = create_user_with_password(&pool).await;
    let totp = enable_mfa(&pool, &config, user, &username).await;
    let (_, token) = create_session_with_token(&pool, &config, user).await;

    // The first verify hands out the initial backup codes.
    let code = totp.generate_current().expect("code");
    let (status, body) = post_json(
        &server,
        "/auth/mfa/verify",
        &token,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, 200, "first verify: {body}");
    let backup_code = body["backup_codes"][0]
        .as_str()
        .expect("initial backup codes")
        .to_string();

    let (status, body) = post_json(&server, "/auth/mfa/backup-codes", &token, None).await;
    assert_eq!(status, 400, "no body: {body}");

    let (status, body) = post_json(
        &server,
        "/auth/mfa/backup-codes",
        &token,
        Some(json!({ "mfa_code": backup_code })),
    )
    .await;
    assert_eq!(status, 401, "backup code is not accepted: {body}");
    assert_eq!(body["error"], "INVALID_CREDENTIALS");

    // The code used for /mfa/verify cannot be used again; the next step's
    // code (accepted within the ±1 step window) can.
    let (status, body) = post_json(
        &server,
        "/auth/mfa/backup-codes",
        &token,
        Some(json!({ "mfa_code": code })),
    )
    .await;
    assert_eq!(status, 401, "replayed TOTP code: {body}");
    let code = next_step_code(&totp);
    let (status, body) = post_json(
        &server,
        "/auth/mfa/backup-codes",
        &token,
        Some(json!({ "mfa_code": code })),
    )
    .await;
    assert_eq!(status, 200, "valid TOTP: {body}");
    assert_eq!(body["codes"].as_array().map(Vec::len), Some(10));
}

#[sqlx::test]
async fn test_reauth_attempts_are_rate_limited(pool: PgPool) {
    let (server, config) = rate_limited_server(&pool).await;
    let (admin, username) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let totp = enable_mfa(&pool, &config, admin, &username).await;
    let (_, token) = create_session_with_token(&pool, &config, admin).await;
    let bad = wrong_code(&totp);

    for attempt in 1..=10 {
        let (status, body) = post_json(
            &server,
            "/api/admin/elevate",
            &token,
            Some(json!({ "password": PASSWORD, "mfa_code": bad })),
        )
        .await;
        assert_eq!(status, 401, "attempt {attempt}: {body}");
    }

    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "mfa_code": bad })),
    )
    .await;
    assert_eq!(status, 429, "11th attempt: {body}");

    let code = totp.generate_current().expect("code");
    let (status, body) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "mfa_code": code })),
    )
    .await;
    assert_eq!(status, 429, "a correct attempt is still refused: {body}");

    let (_, status_body) = get_json(&server, "/api/admin/status", &token).await;
    assert_eq!(status_body["is_elevated"], false, "never elevated");
}

#[sqlx::test]
async fn test_password_change_is_rate_limited(pool: PgPool) {
    let (server, config) = rate_limited_server(&pool).await;
    let (user, _) = create_user_with_password(&pool).await;
    let (_, token) = create_session_with_token(&pool, &config, user).await;
    let wrong = json!({ "current_password": "wrong-password", "new_password": "Brand-New-Pass-1" });

    for attempt in 1..=10 {
        let (status, body) =
            post_json(&server, "/auth/me/password", &token, Some(wrong.clone())).await;
        assert_eq!(status, 401, "attempt {attempt}: {body}");
    }

    let (status, body) = post_json(&server, "/auth/me/password", &token, Some(wrong)).await;
    assert_eq!(status, 429, "11th attempt: {body}");

    let (status, body) = post_json(
        &server,
        "/auth/me/password",
        &token,
        Some(json!({ "current_password": PASSWORD, "new_password": "Brand-New-Pass-1" })),
    )
    .await;
    assert_eq!(status, 429, "a correct attempt is still refused: {body}");
}

#[sqlx::test]
async fn test_totp_code_is_accepted_once(pool: PgPool) {
    let (server, config, _) = plain_server(&pool).await;
    let (admin, username) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let totp = enable_mfa(&pool, &config, admin, &username).await;
    let (_, token) = create_session_with_token(&pool, &config, admin).await;
    let code = totp.generate_current().expect("code");
    let body = json!({ "password": PASSWORD, "mfa_code": code });

    let (status, _) = post_json(&server, "/api/admin/elevate", &token, Some(body.clone())).await;
    assert_eq!(status, 200);
    let resp = reqwest::Client::new()
        .delete(format!("{}/api/admin/elevate", server.url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);

    let (status, body) = post_json(&server, "/api/admin/elevate", &token, Some(body)).await;
    assert_eq!(status, 401, "replayed code: {body}");
    assert_eq!(body["error"], "INVALID_CREDENTIALS");
    let (status, _) = post_json(
        &server,
        "/auth/mfa/backup-codes",
        &token,
        Some(json!({ "mfa_code": code })),
    )
    .await;
    assert_eq!(status, 401, "replayed code on backup codes");
}

#[sqlx::test]
async fn test_elevation_audit_records_verified_factors(pool: PgPool) {
    let (server, config, _) = plain_server(&pool).await;

    let (plain, _) = create_user_with_password(&pool).await;
    make_admin(&pool, plain).await;
    let (_, token) = create_session_with_token(&pool, &config, plain).await;
    // An mfa_code sent for an account without MFA is not a verified factor.
    let (status, _) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "mfa_code": "123456" })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        last_elevation_method(&pool, plain).await.as_deref(),
        Some("password")
    );

    let (mfa_admin, username) = create_user_with_password(&pool).await;
    make_admin(&pool, mfa_admin).await;
    let totp = enable_mfa(&pool, &config, mfa_admin, &username).await;
    let (_, token) = create_session_with_token(&pool, &config, mfa_admin).await;
    let (status, _) = post_json(
        &server,
        "/api/admin/elevate",
        &token,
        Some(json!({ "password": PASSWORD, "mfa_code": totp.generate_current().expect("code") })),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        last_elevation_method(&pool, mfa_admin).await.as_deref(),
        Some("password+totp")
    );
}

/// A TOTP code works once at sign-in: the same code again is refused with the
/// wrong-code error, and the next step's code still works.
#[sqlx::test]
async fn test_login_totp_code_is_accepted_once(pool: PgPool) {
    let (server, config, _) = plain_server(&pool).await;
    let (user, username) = create_user_with_password(&pool).await;
    let totp = enable_mfa(&pool, &config, user, &username).await;
    let code = totp.generate_current().expect("code");

    let login = |code: String| {
        let url = format!("{}/auth/login", server.url);
        let body = json!({ "username": username, "password": PASSWORD, "mfa_code": code });
        async move {
            let resp = reqwest::Client::new()
                .post(url)
                .json(&body)
                .send()
                .await
                .expect("login request");
            let status = resp.status().as_u16();
            (status, resp.json::<Value>().await.unwrap_or(Value::Null))
        }
    };

    let (status, body) = login(code.clone()).await;
    assert_eq!(status, 200, "first use: {body}");
    let (status, body) = login(code).await;
    assert_eq!(status, 401, "replayed code: {body}");
    assert_eq!(body["error"], "INVALID_MFA");

    // A wrong password with a fresh code must not consume that code.
    let next = next_step_code(&totp);
    let resp = reqwest::Client::new()
        .post(format!("{}/auth/login", server.url))
        .json(&json!({ "username": username, "password": "Wrong-Pass-1", "mfa_code": next }))
        .send()
        .await
        .expect("login request");
    assert_eq!(resp.status().as_u16(), 401);
    let (status, body) = login(next).await;
    assert_eq!(status, 200, "code unused after a wrong password: {body}");
}
