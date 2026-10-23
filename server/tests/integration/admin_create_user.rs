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
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin_id, _) = create_test_user(pool).await;
    make_admin(pool, admin_id).await;
    create_elevated_session(pool, admin_id).await;
    let token = generate_access_token(&app.config, admin_id);
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
    assert_eq!(body["error"], "elevation_required");

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
    assert_eq!(body["error"], "conflict");
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
