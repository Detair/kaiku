//! Ending sessions immediately (C25).
//!
//! A global ban, a single-session revoke, "revoke all other sessions", a
//! password change with `revoke_others` and a logout end the affected sessions
//! right away: their access tokens are refused (401), their refresh is refused
//! (401, or 403 `ACCOUNT_BANNED` for a banned account) and their open sockets
//! receive an `error` event `SESSION_REVOKED` followed by close code 1008.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use reqwest::StatusCode;
use sqlx::PgPool;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
use vc_server::db;

use super::helpers::{
    create_elevated_session, create_test_user, expect_no_ws_event, generate_access_token,
    make_admin, next_ws_event, spawn_test_server, ws_connect, TestApp, TestServer, WsClient,
};

/// Upper bound for awaited WebSocket events (setup, `ready`).
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

/// How quickly an affected socket must be told and closed.
const REVOKE_TIMEOUT: Duration = Duration::from_secs(2);

const PASSWORD: &str = "family-pass-123";

struct Fixture {
    app: TestApp,
    server: TestServer,
    admin_token: String,
}

async fn setup(pool: &PgPool) -> Fixture {
    let app = TestApp::with_pool(pool.clone()).await;
    let server = spawn_test_server(app.router.clone()).await;
    let (admin_id, _) = create_test_user(pool).await;
    make_admin(pool, admin_id).await;
    let admin_token = create_elevated_session(pool, &app.config, admin_id).await;
    Fixture {
        app,
        server,
        admin_token,
    }
}

/// Create a local user with [`PASSWORD`]; returns `(user_id, username)`.
async fn create_password_user(pool: &PgPool) -> (Uuid, String) {
    let username = format!("rev_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let hash = vc_server::auth::hash_password(PASSWORD).expect("hash");
    let user = db::create_user(pool, &username, "Revocation Test", None, &hash)
        .await
        .expect("create user");
    (user.id, username)
}

/// One logged-in session.
struct Session {
    access: String,
    refresh: String,
}

/// Log in without an `Origin` header so the refresh token comes back in the body.
async fn login_raw(server: &TestServer, username: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/auth/login", server.url))
        .json(&serde_json::json!({ "username": username, "password": PASSWORD }))
        .send()
        .await
        .expect("POST /auth/login")
}

async fn login(server: &TestServer, username: &str) -> Session {
    let resp = login_raw(server, username).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("login JSON");
    Session {
        access: body["access_token"].as_str().expect("access").to_string(),
        refresh: body["refresh_token"].as_str().expect("refresh").to_string(),
    }
}

async fn refresh(server: &TestServer, refresh_token: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}/auth/refresh", server.url))
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .expect("POST /auth/refresh")
}

async fn me_status(server: &TestServer, token: &str) -> StatusCode {
    reqwest::Client::new()
        .get(format!("{}/auth/me", server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("GET /auth/me")
        .status()
}

async fn error_code(resp: reqwest::Response) -> String {
    let body: serde_json::Value = resp.json().await.expect("error JSON");
    body["error"].as_str().unwrap_or_default().to_string()
}

/// The session id (`sid` claim) carried by an access token.
fn session_id(app: &TestApp, access: &str) -> Uuid {
    let claims = vc_server::auth::jwt::validate_access_token(access, &app.config.jwt_public_key)
        .expect("valid access token");
    claims.sid.expect("sid").parse().expect("sid uuid")
}

async fn ban(fx: &Fixture, user_id: Uuid) {
    let resp = reqwest::Client::new()
        .post(format!("{}/api/admin/users/{user_id}/ban", fx.server.url))
        .bearer_auth(&fx.admin_token)
        .json(&serde_json::json!({ "reason": "test", "expires_at": null }))
        .send()
        .await
        .expect("POST ban");
    assert_eq!(resp.status(), StatusCode::OK);
}

async fn connect_ready(server: &TestServer, token: &str) -> WsClient {
    let mut ws = ws_connect(server, token).await;
    next_ws_event(&mut ws, "ready", EVENT_TIMEOUT).await;
    ws
}

/// Wait for the Close frame (other frames are skipped) and return it.
async fn next_close(ws: &mut WsClient, timeout: Duration) -> Option<CloseFrame> {
    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("WebSocket error while waiting for close: {e}"),
                None => panic!("WebSocket stream ended without a Close frame"),
            }
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .unwrap_or_else(|_| panic!("no Close frame within {timeout:?}"))
}

/// The socket gets `error` `SESSION_REVOKED`, then close 1008.
async fn expect_session_revoked(ws: &mut WsClient) {
    let event = next_ws_event(ws, "error", REVOKE_TIMEOUT).await;
    assert_eq!(
        event["code"], "SESSION_REVOKED",
        "unexpected error: {event}"
    );
    let frame = next_close(ws, REVOKE_TIMEOUT).await.expect("close frame");
    assert_eq!(u16::from(frame.code), 1008);
}

/// The socket is still open and served: a ping is answered with a pong.
async fn expect_still_open(ws: &mut WsClient) {
    expect_no_ws_event(ws, "error", Duration::from_millis(500)).await;
    ws.send(Message::Text(
        serde_json::json!({ "type": "ping" }).to_string().into(),
    ))
    .await
    .expect("send ping");
    next_ws_event(ws, "pong", EVENT_TIMEOUT).await;
}

#[sqlx::test]
async fn test_banned_user_cannot_login_or_refresh(pool: PgPool) {
    let fx = setup(&pool).await;
    let (user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;

    ban(&fx, user_id).await;

    let resp = login_raw(&fx.server, &username).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, "ACCOUNT_BANNED");

    let resp = refresh(&fx.server, &session.refresh).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, "ACCOUNT_BANNED");
}

#[sqlx::test]
async fn test_ban_ends_existing_tokens_and_sockets(pool: PgPool) {
    let fx = setup(&pool).await;
    let (user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;
    let mut ws = connect_ready(&fx.server, &session.access).await;

    ban(&fx, user_id).await;

    assert_eq!(
        me_status(&fx.server, &session.access).await,
        StatusCode::UNAUTHORIZED
    );
    expect_session_revoked(&mut ws).await;
}

#[sqlx::test]
async fn test_password_change_revoke_others_with_bearer_only(pool: PgPool) {
    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let a = login(&fx.server, &username).await;
    let b = login(&fx.server, &username).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/me/password", fx.server.url))
        .bearer_auth(&a.access)
        .json(&serde_json::json!({
            "current_password": PASSWORD,
            "new_password": "another-pass-456",
            "revoke_others": true,
        }))
        .send()
        .await
        .expect("POST /auth/me/password");
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        me_status(&fx.server, &b.access).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(&fx.server, &b.refresh).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(me_status(&fx.server, &a.access).await, StatusCode::OK);
}

#[sqlx::test]
async fn test_revoke_one_session_closes_only_that_socket(pool: PgPool) {
    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let a = login(&fx.server, &username).await;
    let b = login(&fx.server, &username).await;
    let sid_b = session_id(&fx.app, &b.access);
    assert_ne!(session_id(&fx.app, &a.access), sid_b);

    let mut ws_a = connect_ready(&fx.server, &a.access).await;
    let mut ws_b = connect_ready(&fx.server, &b.access).await;

    let resp = reqwest::Client::new()
        .delete(format!("{}/auth/sessions/{sid_b}", fx.server.url))
        .bearer_auth(&a.access)
        .send()
        .await
        .expect("DELETE /auth/sessions/{id}");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    expect_session_revoked(&mut ws_b).await;
    expect_still_open(&mut ws_a).await;
    assert_eq!(me_status(&fx.server, &a.access).await, StatusCode::OK);
}

/// A socket opened before the client refreshed its tokens still belongs to
/// the same session, so revoking that session closes it.
#[sqlx::test]
async fn test_revoke_after_refresh_closes_socket(pool: PgPool) {
    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let a = login(&fx.server, &username).await;
    let b = login(&fx.server, &username).await;
    let mut ws_b = connect_ready(&fx.server, &b.access).await;

    let resp = refresh(&fx.server, &b.refresh).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("refresh JSON");
    let new_access = body["access_token"].as_str().expect("access").to_string();
    let sid_b = session_id(&fx.app, &new_access);
    assert_eq!(sid_b, session_id(&fx.app, &b.access));

    let resp = reqwest::Client::new()
        .delete(format!("{}/auth/sessions/{sid_b}", fx.server.url))
        .bearer_auth(&a.access)
        .send()
        .await
        .expect("DELETE /auth/sessions/{id}");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    expect_session_revoked(&mut ws_b).await;
    assert_eq!(
        me_status(&fx.server, &b.access).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        me_status(&fx.server, &new_access).await,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn test_logout_without_refresh_token_ends_current_session(pool: PgPool) {
    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/logout", fx.server.url))
        .bearer_auth(&session.access)
        .send()
        .await
        .expect("POST /auth/logout");
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        me_status(&fx.server, &session.access).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(&fx.server, &session.refresh).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// A device that was offline while its session ended (Review Focus 1): its
/// refresh and every way of opening a socket are refused, so the app lands on
/// the login screen instead of retrying.
#[sqlx::test]
async fn test_revoked_session_refresh_and_ws_are_refused(pool: PgPool) {
    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let a = login(&fx.server, &username).await;
    let b = login(&fx.server, &username).await;
    let sid_b = session_id(&fx.app, &b.access);

    let resp = reqwest::Client::new()
        .delete(format!("{}/auth/sessions/{sid_b}", fx.server.url))
        .bearer_auth(&a.access)
        .send()
        .await
        .expect("DELETE /auth/sessions/{id}");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        refresh(&fx.server, &b.refresh).await.status(),
        StatusCode::UNAUTHORIZED
    );

    // Header-based handshake: refused before the upgrade.
    let mut request = format!("ws://{}/ws", fx.server.addr)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("access_token.{}", b.access).parse().unwrap(),
    );
    match tokio_tungstenite::connect_async(request).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status().as_u16(), 401);
        }
        Err(e) => panic!("unexpected error: {e}"),
        Ok(_) => panic!("a revoked session must not be upgraded"),
    }

    // Frame-based auth: `error` SESSION_REVOKED, then close 1008.
    let mut ws = ws_connect(&fx.server, &b.access).await;
    expect_session_revoked(&mut ws).await;
}

#[sqlx::test]
async fn test_attachment_download_token_respects_revocation(pool: PgPool) {
    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;
    let download = format!(
        "{}/api/messages/attachments/{}/download?token={}",
        fx.server.url,
        Uuid::now_v7(),
        session.access
    );

    // While the session is live the token passes the caller checks (this
    // server has no storage configured, which is checked next).
    let resp = reqwest::get(&download).await.expect("download");
    assert_eq!(error_code(resp).await, "STORAGE_NOT_CONFIGURED");

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/logout", fx.server.url))
        .bearer_auth(&session.access)
        .json(&serde_json::json!({ "refresh_token": session.refresh }))
        .send()
        .await
        .expect("POST /auth/logout");
    assert_eq!(resp.status(), StatusCode::OK);

    // An ended session is an authentication failure (C25): 401.
    let resp = reqwest::get(&download).await.expect("download");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(resp).await, "INVALID_TOKEN");
}

/// A `?token=` download fails closed when the revocation list cannot be read.
#[sqlx::test]
async fn test_attachment_download_token_fails_closed_without_valkey(pool: PgPool) {
    let (server, config) = server_with_failing_valkey(&pool).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);
    let download = format!(
        "{}/api/messages/attachments/{}/download?token={token}",
        server.url,
        Uuid::now_v7(),
    );

    let resp = reqwest::get(&download).await.expect("download");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_code(resp).await, "SERVICE_UNAVAILABLE");
}

/// A ban closes every open socket of the user, also one whose session row is
/// already gone.
#[sqlx::test]
async fn test_ban_closes_socket_without_session_rows(pool: PgPool) {
    let fx = setup(&pool).await;
    let (user_id, _) = create_test_user(&pool).await;
    // A valid token whose session has no row.
    let token = generate_access_token(&fx.app.config, user_id);
    let mut ws = connect_ready(&fx.server, &token).await;
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    ban(&fx, user_id).await;

    expect_session_revoked(&mut ws).await;
    let resp = reqwest::Client::new()
        .get(format!("{}/auth/me", fx.server.url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, "ACCOUNT_BANNED");
}

#[sqlx::test]
async fn test_bulk_ban_ends_sessions_and_sockets(pool: PgPool) {
    let fx = setup(&pool).await;
    let (user_a, name_a) = create_password_user(&pool).await;
    let (user_b, name_b) = create_password_user(&pool).await;
    let a = login(&fx.server, &name_a).await;
    let b = login(&fx.server, &name_b).await;
    let mut ws_a = connect_ready(&fx.server, &a.access).await;
    let mut ws_b = connect_ready(&fx.server, &b.access).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/api/admin/users/bulk-ban", fx.server.url))
        .bearer_auth(&fx.admin_token)
        .json(&serde_json::json!({
            "user_ids": [user_a, user_b],
            "reason": "test",
            "expires_at": null,
        }))
        .send()
        .await
        .expect("POST bulk-ban");
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["banned_count"], 2, "{body}");

    expect_session_revoked(&mut ws_a).await;
    expect_session_revoked(&mut ws_b).await;
    for s in [&a, &b] {
        assert_eq!(
            me_status(&fx.server, &s.access).await,
            StatusCode::UNAUTHORIZED
        );
        let resp = refresh(&fx.server, &s.refresh).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(error_code(resp).await, "ACCOUNT_BANNED");
    }
}

#[sqlx::test]
async fn test_password_reset_ends_sessions_and_sockets(pool: PgPool) {
    let fx = setup(&pool).await;
    let (user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;
    let mut ws = connect_ready(&fx.server, &session.access).await;

    let raw = format!("reset-{}", Uuid::new_v4());
    db::create_password_reset_token(
        &pool,
        user_id,
        &vc_server::auth::hash_token(&raw),
        chrono::Utc::now() + chrono::Duration::minutes(30),
    )
    .await
    .expect("reset token");

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/reset-password", fx.server.url))
        .json(&serde_json::json!({ "token": raw, "new_password": "reset-pass-789" }))
        .send()
        .await
        .expect("POST /auth/reset-password");
    assert_eq!(resp.status(), StatusCode::OK);

    expect_session_revoked(&mut ws).await;
    assert_eq!(
        me_status(&fx.server, &session.access).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(&fx.server, &session.refresh).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// With `revoke_others` the caller's own session must be identifiable;
/// otherwise the request is refused and nothing changes.
#[sqlx::test]
async fn test_password_change_revoke_others_requires_current_session(pool: PgPool) {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use vc_server::auth::jwt::{Claims, TokenType};

    let fx = setup(&pool).await;
    let (user_id, username) = create_password_user(&pool).await;
    let other = login(&fx.server, &username).await;

    // An access token without a `sid` claim, and no refresh token sent.
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user_id.to_string(),
        exp: now + 600,
        iat: now,
        typ: TokenType::Access,
        jti: None,
        sid: None,
    };
    let pem = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &fx.app.config.jwt_private_key,
    )
    .unwrap();
    let token = encode(
        &Header::new(Algorithm::EdDSA),
        &claims,
        &EncodingKey::from_ed_pem(&pem).unwrap(),
    )
    .unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/auth/me/password", fx.server.url))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "current_password": PASSWORD,
            "new_password": "another-pass-456",
            "revoke_others": true,
        }))
        .send()
        .await
        .expect("POST /auth/me/password");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "VALIDATION_ERROR");

    // Nothing changed: the old password still works, the other session too.
    assert_eq!(
        login_raw(&fx.server, &username).await.status(),
        StatusCode::OK
    );
    assert_eq!(me_status(&fx.server, &other.access).await, StatusCode::OK);
}

/// A session whose id is denylisted cannot be refreshed even if its row is
/// still there.
#[sqlx::test]
async fn test_refresh_refused_for_denylisted_session(pool: PgPool) {
    use fred::interfaces::KeysInterface;

    let fx = setup(&pool).await;
    let (_user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;
    let sid = session_id(&fx.app, &session.access);
    fx.app
        .state
        .redis
        .set::<(), _, _>(
            format!("canis:revoked_sid:{sid}"),
            "1",
            Some(fred::types::Expiration::EX(60)),
            None,
            false,
        )
        .await
        .unwrap();

    assert_eq!(
        refresh(&fx.server, &session.refresh).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// Retrying an account deletion that is already scheduled (e.g. after the
/// first attempt could not reach Valkey) still ends every session: the open
/// socket closes and the caller's token stops working, and the answer stays
/// 409.
#[sqlx::test]
async fn test_deletion_retry_ends_sessions_again(pool: PgPool) {
    let fx = setup(&pool).await;
    let (user_id, username) = create_password_user(&pool).await;
    let session = login(&fx.server, &username).await;
    let mut ws = connect_ready(&fx.server, &session.access).await;

    // State left by a first attempt whose session ending failed: deletion
    // scheduled, session row still present (nothing denylisted, no notice).
    sqlx::query(
        "UPDATE users SET deletion_requested_at = NOW(),
                          deletion_scheduled_at = NOW() + INTERVAL '30 days'
         WHERE id = $1",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let resp = reqwest::Client::new()
        .post(format!("{}/api/me/delete-account", fx.server.url))
        .bearer_auth(&session.access)
        .json(&serde_json::json!({ "password": PASSWORD, "confirm": "DELETE" }))
        .send()
        .await
        .expect("POST /api/me/delete-account");
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    expect_session_revoked(&mut ws).await;
    assert_eq!(
        me_status(&fx.server, &session.access).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        refresh(&fx.server, &session.refresh).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

/// A server whose Valkey client fails every command (it is connected, then
/// closed; a short command timeout turns the wait into an error).
async fn server_with_failing_valkey(
    pool: &PgPool,
) -> (TestServer, std::sync::Arc<vc_server::config::Config>) {
    use fred::prelude::{ClientLike, Config as ValkeyConfig, PerformanceConfig};

    let config = vc_server::config::Config::default_for_test();
    let perf = PerformanceConfig {
        default_command_timeout: Duration::from_millis(300),
        ..PerformanceConfig::default()
    };
    let redis = fred::clients::Client::new(
        ValkeyConfig::from_url(&config.redis_url).expect("Valkey URL"),
        Some(perf),
        None,
        None,
    );
    redis.connect();
    redis.wait_for_connect().await.expect("connect Valkey");
    redis.quit().await.expect("quit Valkey");

    server_with_valkey(pool, config, redis).await
}

/// A server whose Valkey client can read (so the fail-closed session
/// denylist check in `require_auth` passes) but is refused `SET` and
/// `PUBLISH`, so ending sessions fails after the handler's commit. Uses a
/// dedicated Valkey ACL user.
async fn server_with_write_refusing_valkey(
    pool: &PgPool,
) -> (TestServer, std::sync::Arc<vc_server::config::Config>) {
    use fred::prelude::{ClientLike, Config as ValkeyConfig};

    const ACL_USER: &str = "kaiku-test-no-writes";
    const ACL_PASSWORD: &str = "kaiku-test-no-writes";

    let config = vc_server::config::Config::default_for_test();
    let admin = db::create_redis_client(&config.redis_url)
        .await
        .expect("Valkey");
    let _: () = admin
        .custom(
            fred::types::CustomCommand::new_static("ACL", None, false),
            vec![
                "SETUSER",
                ACL_USER,
                "reset",
                "on",
                &format!(">{ACL_PASSWORD}"),
                "~*",
                "&*",
                "+@all",
                "-set",
                "-publish",
            ],
        )
        .await
        .expect("ACL SETUSER");
    let _ = admin.quit().await;

    let mut valkey_config = ValkeyConfig::from_url(&config.redis_url).expect("Valkey URL");
    valkey_config.username = Some(ACL_USER.to_string());
    valkey_config.password = Some(ACL_PASSWORD.to_string());
    let redis = fred::clients::Client::new(valkey_config, None, None, None);
    redis.connect();
    redis.wait_for_connect().await.expect("connect Valkey");

    server_with_valkey(pool, config, redis).await
}

/// A test server on `pool` that uses `redis` as its Valkey client.
async fn server_with_valkey(
    pool: &PgPool,
    config: vc_server::config::Config,
    redis: fred::clients::Client,
) -> (TestServer, std::sync::Arc<vc_server::config::Config>) {
    let sfu = vc_server::voice::sfu::SfuServer::new(std::sync::Arc::new(config.clone()), None)
        .expect("SfuServer");
    let state = vc_server::api::AppState::new(vc_server::api::AppStateConfig {
        db: pool.clone(),
        redis,
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter: None,
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    });
    let server = spawn_test_server(vc_server::api::create_router(state)).await;
    (server, std::sync::Arc::new(config))
}

/// When ending the sessions fails after the ban committed (Valkey refuses
/// the denylist write), the ban and its audit entry still exist and the
/// request answers 500 so the admin can retry. (With Valkey fully down the
/// request is already refused with 503 by the fail-closed denylist check.)
#[sqlx::test]
async fn test_ban_is_audited_when_ending_sessions_fails(pool: PgPool) {
    let (server, config) = server_with_write_refusing_valkey(&pool).await;
    let (admin_id, _) = create_test_user(&pool).await;
    make_admin(&pool, admin_id).await;
    let admin_token = create_elevated_session(&pool, &config, admin_id).await;
    let (user_id, _) = create_test_user(&pool).await;

    let resp = tokio::time::timeout(
        EVENT_TIMEOUT,
        reqwest::Client::new()
            .post(format!("{}/api/admin/users/{user_id}/ban", server.url))
            .bearer_auth(&admin_token)
            .json(&serde_json::json!({ "reason": "test", "expires_at": null }))
            .send(),
    )
    .await
    .expect("ban request must not hang")
    .expect("POST ban");
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let banned: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM global_bans WHERE user_id = $1)")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(banned, "the committed ban must stand");
    let audited: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM system_audit_log WHERE action = 'admin.users.ban' AND target_id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1, "the committed ban must be audited");
}
