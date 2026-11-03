//! Re-authentication for sensitive actions (C24).
//!
//! An elevation belongs to the access token's session and a TOTP code is
//! accepted once at sign-in.
//!
//! Each test runs a real HTTP server (handlers read the peer address) on a
//! fresh per-test database.

use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;
use vc_server::api::AppState;
use vc_server::config::Config;

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

/// The code of the next 30-second step (valid now within the ±1 window).
fn next_step_code(totp: &TOTP) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    totp.generate(now + 30)
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

/// `DELETE /api/admin/elevate` ends only the caller's elevation; another
/// elevated session of the same admin stays elevated.
#[sqlx::test]
async fn test_de_elevation_is_bound_to_session(pool: PgPool) {
    let (server, config, state) = plain_server(&pool).await;
    let (admin, _) = create_user_with_password(&pool).await;
    make_admin(&pool, admin).await;
    let (session_a, token_a) = create_session_with_token(&pool, &config, admin).await;
    let (session_b, token_b) = create_session_with_token(&pool, &config, admin).await;
    // Elevation itself is a step-up now (see `step_up_grants`); elevate
    // both sessions directly.
    for sid in [session_a, session_b] {
        sqlx::query(
            "INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
             VALUES ($1, $2, '127.0.0.1'::inet, NOW() + INTERVAL '15 minutes', 'test')",
        )
        .bind(admin)
        .bind(sid)
        .execute(&pool)
        .await
        .unwrap();
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
