//! Admin-created accounts with a forced password change (C4).
//!
//! `POST /api/admin/users` lets an elevated system admin create a local
//! account with a temporary password. Until the new user sets their own
//! password, only a small set of auth routes works and the WebSocket is
//! refused with close code 1008.

use std::time::Duration;

use futures::StreamExt;
use reqwest::StatusCode;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::chat::S3Client;
use vc_server::config::Config;
use vc_server::db;
use vc_server::voice::sfu::SfuServer;

use super::helpers::{
    create_elevated_session, create_test_user, generate_access_token, make_admin,
    spawn_test_server, ws_connect, TestApp, TestServer,
};

const TEMP_PASSWORD: &str = "temporary-pass-123";
const NEW_PASSWORD: &str = "my-own-secret-456";

/// Spin up a server and an elevated system admin; returns the admin's token.
async fn setup_admin(pool: &PgPool) -> (TestApp, TestServer, String) {
    setup_admin_with(pool, vc_server::config::Config::default_for_test()).await
}

async fn setup_admin_with(
    pool: &PgPool,
    config: vc_server::config::Config,
) -> (TestApp, TestServer, String) {
    let app = TestApp::with_pool_and_config(pool.clone(), config).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin_id, _) = create_test_user(pool).await;
    make_admin(pool, admin_id).await;
    let token = create_elevated_session(pool, &app.config, admin_id).await;
    (app, server, token)
}

async fn create_user(
    server: &TestServer,
    token: &str,
    username: &str,
    password: &str,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/api/admin/users", server.url))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "username": username,
            "display_name": "Oma Erna",
            "temporary_password": password,
        }))
        .send()
        .await
        .expect("POST /api/admin/users")
}

/// Log in without an `Origin` header so the refresh token comes back in the body.
async fn login(server: &TestServer, username: &str, password: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/auth/login", server.url))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .expect("POST /auth/login")
}

async fn create_and_login(server: &TestServer, admin_token: &str) -> serde_json::Value {
    let resp = create_user(server, admin_token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = login(server, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::OK);
    resp.json().await.expect("login JSON")
}

#[sqlx::test]
async fn admin_creates_user_201(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;

    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.expect("JSON");
    assert_eq!(body["username"], "oma");
    assert_eq!(body["display_name"], "Oma Erna");
    assert_eq!(body["is_banned"], false);
    assert!(body["id"].as_str().is_some());
    assert!(body["created_at"].as_str().is_some());

    let (must_change, auth_method): (bool, String) = sqlx::query_as(
        "SELECT must_change_password, auth_method::text FROM users WHERE username = 'oma'",
    )
    .fetch_one(&pool)
    .await
    .expect("user row");
    assert!(must_change);
    assert_eq!(auth_method, "local");

    // Audit entry exists and never contains the password.
    let details: Vec<Option<serde_json::Value>> = sqlx::query_scalar(
        "SELECT details FROM system_audit_log WHERE action = 'admin.users.create'",
    )
    .fetch_all(&pool)
    .await
    .expect("audit rows");
    assert_eq!(details.len(), 1);
    let rendered = serde_json::to_string(&details[0]).unwrap();
    assert!(rendered.contains("oma"));
    assert!(!rendered.contains(TEMP_PASSWORD));
}

#[sqlx::test]
async fn create_user_defaults_display_name_and_validates(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/admin/users", server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "username": "opa",
            "display_name": null,
            "temporary_password": TEMP_PASSWORD,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["display_name"], "opa");

    for bad in [
        serde_json::json!({"username": "Bad Name", "display_name": null, "temporary_password": TEMP_PASSWORD}),
        serde_json::json!({"username": "ok_name", "display_name": null, "temporary_password": "short"}),
        serde_json::json!({"username": "ok_name", "display_name": "x".repeat(65), "temporary_password": TEMP_PASSWORD}),
    ] {
        let resp = client
            .post(format!("{}/api/admin/users", server.url))
            .bearer_auth(&token)
            .json(&bad)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "body: {bad}");
    }
}

#[sqlx::test]
async fn create_user_requires_elevation(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin_id, _) = create_test_user(&pool).await;
    make_admin(&pool, admin_id).await;
    let token = generate_access_token(&app.config, admin_id);

    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "ELEVATION_REQUIRED");

    // Listing users stays available without elevation.
    let resp = reqwest::Client::new()
        .get(format!("{}/api/admin/users", server.url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[sqlx::test]
async fn create_user_non_admin_forbidden(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&app.config, user_id);

    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn create_user_duplicate_username_409(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;

    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "CONFLICT");
}

#[sqlx::test]
async fn create_user_works_with_registration_closed(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    sqlx::query(
        "UPDATE server_config SET value = '\"closed\"'::jsonb WHERE key = 'registration_policy'",
    )
    .execute(&pool)
    .await
    .expect("close registration");

    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/register", server.url))
        .json(&serde_json::json!({ "username": "stranger", "password": TEMP_PASSWORD }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn login_reports_password_change_required(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;

    let body = create_and_login(&server, &token).await;
    assert_eq!(body["password_change_required"], true);

    // Ordinary accounts report false.
    let resp = reqwest::Client::new()
        .post(format!("{}/auth/register", server.url))
        .json(&serde_json::json!({ "username": "normalo", "password": TEMP_PASSWORD }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["password_change_required"], false);
    let body: serde_json::Value = login(&server, "normalo", TEMP_PASSWORD)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["password_change_required"], false);
}

#[sqlx::test]
async fn pending_user_only_reaches_allowed_routes(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    let client = reqwest::Client::new();

    let login_body = create_and_login(&server, &token).await;
    let access = login_body["access_token"].as_str().unwrap().to_string();
    let refresh = login_body["refresh_token"].as_str().unwrap().to_string();

    // GET /auth/me is allowed and reports the pending change.
    let resp = client
        .get(format!("{}/auth/me", server.url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let me: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(me["username"], "oma");
    assert_eq!(me["password_change_required"], true);

    // Every other protected route is refused.
    for (method, path) in [
        (reqwest::Method::GET, "/api/guilds"),
        (reqwest::Method::POST, "/auth/me"),
        (reqwest::Method::GET, "/auth/sessions"),
        (reqwest::Method::POST, "/auth/qr/create"),
    ] {
        let resp = client
            .request(method.clone(), format!("{}{path}", server.url))
            .bearer_auth(&access)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{method} {path}");
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "PASSWORD_CHANGE_REQUIRED", "{method} {path}");
    }

    // Refresh is allowed and carries the flag.
    let resp = client
        .post(format!("{}/auth/refresh", server.url))
        .json(&serde_json::json!({ "refresh_token": refresh }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let refreshed: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(refreshed["password_change_required"], true);
    let access = refreshed["access_token"].as_str().unwrap().to_string();

    // Re-using the temporary password is rejected.
    let resp = client
        .post(format!("{}/auth/me/password", server.url))
        .bearer_auth(&access)
        .json(&serde_json::json!({
            "current_password": TEMP_PASSWORD,
            "new_password": TEMP_PASSWORD,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let resp = client
        .post(format!("{}/auth/me/password", server.url))
        .bearer_auth(&access)
        .json(&serde_json::json!({
            "current_password": TEMP_PASSWORD,
            "new_password": NEW_PASSWORD,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The same access token now reaches the rest of the API.
    let resp = client
        .get(format!("{}/auth/me", server.url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let me: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(me["password_change_required"], false);
    let resp = client
        .get(format!("{}/api/guilds", server.url))
        .bearer_auth(&access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = login(&server, "oma", NEW_PASSWORD)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["password_change_required"], false);
}

#[sqlx::test]
async fn pending_user_can_logout(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    let login_body = create_and_login(&server, &token).await;
    let access = login_body["access_token"].as_str().unwrap();
    let refresh = login_body["refresh_token"].as_str().unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/logout", server.url))
        .bearer_auth(access)
        .json(&serde_json::json!({ "refresh_token": refresh }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn pending_user_id(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE username = 'oma'")
        .fetch_one(pool)
        .await
        .expect("oma exists")
}

#[sqlx::test]
async fn pending_user_websocket_refused(pool: PgPool) {
    let (app, server, token) = setup_admin(&pool).await;
    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let user_token = generate_access_token(&app.config, pending_user_id(&pool).await);

    let mut ws = ws_connect(&server, &user_token).await;
    let mut saw_error = false;
    let close_code = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                    assert_ne!(v["type"], "ready", "pending user must not get ready");
                    if v["type"] == "error" {
                        assert_eq!(v["code"], "PASSWORD_CHANGE_REQUIRED");
                        saw_error = true;
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    return frame.map(|f| u16::from(f.code));
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("WebSocket error: {e}"),
                None => return None,
            }
        }
    })
    .await
    .expect("socket should close");
    assert!(saw_error, "expected PASSWORD_CHANGE_REQUIRED error event");
    assert_eq!(close_code, Some(1008));
}

#[sqlx::test]
async fn pending_user_websocket_header_auth_refused(pool: PgPool) {
    let (app, server, token) = setup_admin(&pool).await;
    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let user_token = generate_access_token(&app.config, pending_user_id(&pool).await);

    let mut request = format!("ws://{}/ws", server.addr)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("access_token.{user_token}").parse().unwrap(),
    );
    match tokio_tungstenite::connect_async(request).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status().as_u16(), 403);
        }
        Err(e) => panic!("unexpected error: {e}"),
        Ok(_) => panic!("pending user must not be upgraded"),
    }
}

/// Build a router with an S3 client that is never contacted: the download
/// handler only needs storage to be configured before it checks the caller.
async fn app_with_unreachable_s3(pool: &PgPool) -> (axum::Router, std::sync::Arc<Config>) {
    let mut config = Config::default_for_test();
    config.s3_endpoint = Some("http://127.0.0.1:9".to_string());
    config.s3_access_key = Some("unused".to_string());
    config.s3_secret_key = Some("unused".to_string());
    let s3 = S3Client::new(&config).expect("S3 client");
    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("Redis");
    let sfu = SfuServer::new(std::sync::Arc::new(config.clone()), None).expect("SfuServer");
    let state = AppState::new(AppStateConfig {
        db: pool.clone(),
        redis,
        config: config.clone(),
        s3: Some(s3),
        sfu,
        rate_limiter: None,
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    });
    (create_router(state), std::sync::Arc::new(config))
}

#[sqlx::test]
async fn pending_user_query_token_download_refused(pool: PgPool) {
    use tower::ServiceExt;

    let (router, config) = app_with_unreachable_s3(&pool).await;
    let (pending_id, _) = create_test_user(&pool).await;
    sqlx::query("UPDATE users SET must_change_password = true WHERE id = $1")
        .bind(pending_id)
        .execute(&pool)
        .await
        .unwrap();
    let (normal_id, _) = create_test_user(&pool).await;
    let attachment_id = Uuid::now_v7();

    let download = |token: String| {
        axum::http::Request::builder()
            .uri(format!(
                "/api/messages/attachments/{attachment_id}/download?token={token}"
            ))
            .body(axum::body::Body::empty())
            .unwrap()
    };

    let resp = router
        .clone()
        .oneshot(download(generate_access_token(&config, pending_id)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = super::helpers::body_to_json(resp).await;
    assert_eq!(body["error"], "PASSWORD_CHANGE_REQUIRED");

    // Control: an ordinary user without access gets the plain FORBIDDEN code.
    let resp = router
        .oneshot(download(generate_access_token(&config, normal_id)))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = super::helpers::body_to_json(resp).await;
    assert_eq!(body["error"], "FORBIDDEN");
}

/// Without S3 configured, a pending user on the deprecated `?token=` path must
/// still see PASSWORD_CHANGE_REQUIRED, not STORAGE_NOT_CONFIGURED.
#[sqlx::test]
async fn pending_user_query_token_download_refused_without_s3(pool: PgPool) {
    use tower::ServiceExt;

    let app = TestApp::with_pool(pool.clone()).await;
    let (pending_id, _) = create_test_user(&pool).await;
    sqlx::query("UPDATE users SET must_change_password = true WHERE id = $1")
        .bind(pending_id)
        .execute(&pool)
        .await
        .unwrap();
    let token = generate_access_token(&app.config, pending_id);
    let resp = app
        .router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!(
                    "/api/messages/attachments/{}/download?token={token}",
                    Uuid::now_v7()
                ))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body = super::helpers::body_to_json(resp).await;
    assert_eq!(body["error"], "PASSWORD_CHANGE_REQUIRED");
}

/// The account insert and its audit entry are atomic: when the audit write
/// fails, no account is left behind and a retry succeeds.
#[sqlx::test]
async fn audit_failure_rolls_back_created_account(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;

    sqlx::query(
        r"CREATE FUNCTION test_fail_audit() RETURNS trigger AS $$
          BEGIN RAISE EXCEPTION 'audit write blocked'; END $$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r"CREATE TRIGGER test_fail_audit BEFORE INSERT ON system_audit_log
          FOR EACH ROW WHEN (NEW.action = 'admin.users.create')
          EXECUTE FUNCTION test_fail_audit()",
    )
    .execute(&pool)
    .await
    .unwrap();

    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert!(resp.status().is_server_error(), "got {}", resp.status());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE username = 'oma'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "account must be rolled back");

    sqlx::query("DROP TRIGGER test_fail_audit ON system_audit_log")
        .execute(&pool)
        .await
        .unwrap();
    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED, "retry must succeed");
}

// ---------------------------------------------------------------------------
// C23: guild_ids on admin-created accounts
// ---------------------------------------------------------------------------

async fn create_user_in_guilds(
    server: &TestServer,
    token: &str,
    username: &str,
    guild_ids: serde_json::Value,
) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/api/admin/users", server.url))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "username": username,
            "display_name": null,
            "temporary_password": TEMP_PASSWORD,
            "guild_ids": guild_ids,
        }))
        .send()
        .await
        .expect("POST /api/admin/users")
}

async fn guild_state(pool: &PgPool, guild_id: Uuid, username: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT gm.membership_state::text FROM guild_members gm
         JOIN users u ON u.id = gm.user_id
         WHERE gm.guild_id = $1 AND u.username = $2",
    )
    .bind(guild_id)
    .bind(username)
    .fetch_optional(pool)
    .await
    .expect("membership query")
}

#[sqlx::test]
async fn test_create_user_joins_requested_guilds(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    let (owner_id, _) = create_test_user(&pool).await;
    let guild_a = super::helpers::create_guild(&pool, owner_id).await;
    let guild_b = super::helpers::create_guild(&pool, owner_id).await;
    let guild_c = super::helpers::create_guild(&pool, owner_id).await;

    let resp = create_user_in_guilds(
        &server,
        &token,
        "oma",
        serde_json::json!([guild_a, guild_b]),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    assert_eq!(
        guild_state(&pool, guild_a, "oma").await.as_deref(),
        Some("active")
    );
    assert_eq!(
        guild_state(&pool, guild_b, "oma").await.as_deref(),
        Some("active")
    );
    assert_eq!(guild_state(&pool, guild_c, "oma").await, None);

    // The audit entry records the servers.
    let details: serde_json::Value = sqlx::query_scalar(
        "SELECT details FROM system_audit_log WHERE action = 'admin.users.create'",
    )
    .fetch_one(&pool)
    .await
    .expect("audit row");
    let mut logged: Vec<String> = details["guild_ids"]
        .as_array()
        .expect("guild_ids in audit")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    logged.sort();
    let mut expected = vec![guild_a.to_string(), guild_b.to_string()];
    expected.sort();
    assert_eq!(logged, expected);

    // After the password change the new user sees the servers.
    let login_body: serde_json::Value = login(&server, "oma", TEMP_PASSWORD)
        .await
        .json()
        .await
        .unwrap();
    let access = login_body["access_token"].as_str().unwrap();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/me/password", server.url))
        .bearer_auth(access)
        .json(&serde_json::json!({
            "current_password": TEMP_PASSWORD,
            "new_password": NEW_PASSWORD,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = client
        .get(format!("{}/api/guilds", server.url))
        .bearer_auth(access)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let guilds: serde_json::Value = resp.json().await.unwrap();
    let ids: Vec<&str> = guilds
        .as_array()
        .expect("guild list")
        .iter()
        .filter_map(|g| g["id"].as_str())
        .collect();
    assert_eq!(ids.len(), 2, "guilds: {guilds}");
    assert!(ids.contains(&guild_a.to_string().as_str()));
    assert!(ids.contains(&guild_b.to_string().as_str()));
}

#[sqlx::test]
async fn test_create_user_with_unknown_guild_creates_nothing(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    let (owner_id, _) = create_test_user(&pool).await;
    let guild = super::helpers::create_guild(&pool, owner_id).await;

    let resp = create_user_in_guilds(
        &server,
        &token,
        "oma",
        serde_json::json!([guild, Uuid::now_v7()]),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "VALIDATION_ERROR");
    assert_eq!(body["message"], "unknown server");

    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'oma'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 0);
    let audit: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM system_audit_log WHERE action = 'admin.users.create'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, 0);
    assert_eq!(guild_state(&pool, guild, "oma").await, None);

    // The same username can be created afterwards.
    let resp = create_user_in_guilds(&server, &token, "oma", serde_json::json!([guild])).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[sqlx::test]
async fn test_create_user_without_guild_ids_unchanged(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;

    // Field omitted entirely (the helper above sends no guild_ids).
    let resp = create_user(&server, &token, "oma", TEMP_PASSWORD).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let memberships: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM guild_members gm JOIN users u ON u.id = gm.user_id
         WHERE u.username = 'oma'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(memberships, 0);
    let details: serde_json::Value = sqlx::query_scalar(
        "SELECT details FROM system_audit_log WHERE action = 'admin.users.create'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(details["guild_ids"], serde_json::json!([]));
}

#[sqlx::test]
async fn test_create_user_guild_ids_duplicates_empty_and_limit(pool: PgPool) {
    let (_app, server, token) = setup_admin(&pool).await;
    let (owner_id, _) = create_test_user(&pool).await;
    let guild = super::helpers::create_guild(&pool, owner_id).await;

    // Duplicates are deduplicated, not an error.
    let resp = create_user_in_guilds(
        &server,
        &token,
        "oma",
        serde_json::json!([guild, guild, guild]),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(
        guild_state(&pool, guild, "oma").await.as_deref(),
        Some("active")
    );
    let details: serde_json::Value = sqlx::query_scalar(
        "SELECT details FROM system_audit_log WHERE action = 'admin.users.create'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(details["guild_ids"], serde_json::json!([guild.to_string()]));

    // An explicit empty list works.
    let resp = create_user_in_guilds(&server, &token, "opa", serde_json::json!([])).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    // More than 10 entries is a validation error and creates nothing.
    let many: Vec<Uuid> = (0..11).map(|_| guild).collect();
    let resp = create_user_in_guilds(&server, &token, "tante", serde_json::json!(many)).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "VALIDATION_ERROR");
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'tante'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

#[sqlx::test]
async fn test_create_user_full_guild_creates_nothing(pool: PgPool) {
    let mut config = vc_server::config::Config::default_for_test();
    config.max_members_per_guild = 2;
    let (_app, server, token) = setup_admin_with(&pool, config).await;
    let (owner_id, _) = create_test_user(&pool).await;
    let (other_id, _) = create_test_user(&pool).await;
    // Guild A has room (1/2), guild B is full (2/2).
    let guild_a = super::helpers::create_guild(&pool, owner_id).await;
    let guild_b = super::helpers::create_guild(&pool, owner_id).await;
    super::helpers::add_guild_member(&pool, guild_b, other_id).await;

    let resp = create_user_in_guilds(
        &server,
        &token,
        "oma",
        serde_json::json!([guild_a, guild_b]),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "VALIDATION_ERROR");

    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'oma'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(users, 0, "no account is created");
    let members_a: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM guild_members WHERE guild_id = $1")
            .bind(guild_a)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        members_a, 1,
        "no membership is added in the guild with room"
    );
}

#[sqlx::test]
async fn test_create_user_member_is_active_even_with_screening_on(pool: PgPool) {
    // Screening is on for the server, so only the admin path keeps it active.
    let (_app, server, token) =
        setup_admin_with(&pool, super::helpers::all_features_config()).await;
    let (owner_id, _) = create_test_user(&pool).await;
    let guild = super::helpers::create_guild(&pool, owner_id).await;
    sqlx::query("UPDATE guilds SET screening_enabled = true WHERE id = $1")
        .bind(guild)
        .execute(&pool)
        .await
        .unwrap();

    let resp = create_user_in_guilds(&server, &token, "oma", serde_json::json!([guild])).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(
        guild_state(&pool, guild, "oma").await.as_deref(),
        Some("active")
    );
}
