//! Re-authentication for sensitive actions (C24).
//!
//! An elevation belongs to the access token's session and a TOTP code is
//! accepted once at sign-in.
//!
//! Each test runs a real HTTP server (handlers read the peer address) on a
//! fresh per-test database.

use std::sync::Arc;

use serde_json::Value;
use sqlx::PgPool;
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
