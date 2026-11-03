//! Helpers for the v1.0 login tests (Tasks 1–14).

use sqlx::PgPool;
use uuid::Uuid;
use vc_server::config::Config;

use super::TestApp;

/// The documentation origin every login test signs against.
pub const TEST_ORIGIN: &str = "https://chat.example.com";

/// The test config with the login origin set (as `Config::default_for_test`).
pub fn login_config() -> Config {
    let config = Config::default_for_test();
    assert_eq!(config.public_origin.origin, TEST_ORIGIN);
    config
}

/// A test app on [`login_config`].
pub async fn login_app(pool: PgPool) -> TestApp {
    TestApp::with_pool_and_config(pool, login_config()).await
}

/// The socket peer the login tests come from (documentation range).
pub const TEST_PEER: ([u8; 4], u16) = ([203, 0, 113, 9], 40000);

/// A request builder carrying `ConnectInfo` (the public auth routes read
/// the socket peer for per-IP limits and audit).
pub fn peer_request(method: axum::http::Method, uri: &str) -> axum::http::request::Builder {
    TestApp::request(method, uri).extension(axum::extract::ConnectInfo(std::net::SocketAddr::from(
        TEST_PEER,
    )))
}

/// A Valkey client that fails every command: connected, then closed, with
/// a short command timeout (Valkey "down" for the code under test).
pub async fn failing_valkey(config: &Config) -> fred::clients::Client {
    use fred::prelude::{ClientLike, Config as ValkeyConfig, PerformanceConfig};
    let perf = PerformanceConfig {
        default_command_timeout: std::time::Duration::from_millis(300),
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
    redis
}

/// An `AppState` on `pool` with the given Valkey client (no S3, no limiter).
pub fn state_with_redis(
    pool: PgPool,
    config: Config,
    redis: fred::clients::Client,
) -> vc_server::api::AppState {
    let sfu = vc_server::voice::sfu::SfuServer::new(std::sync::Arc::new(config.clone()), None)
        .expect("SfuServer");
    vc_server::api::AppState::new(vc_server::api::AppStateConfig {
        db: pool,
        redis,
        config,
        s3: None,
        sfu,
        rate_limiter: None,
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    })
}

/// The `AuthUser` `require_auth` builds for the session `sid`.
pub async fn auth_user(app: &TestApp, sid: Uuid) -> vc_server::auth::AuthUser {
    let s = vc_server::auth::session_check::load_live(&app.pool, sid)
        .await
        .unwrap()
        .expect("live session");
    let meta = vc_server::auth::SessionMeta::from(&s);
    let mut auth = vc_server::auth::AuthUser::from(s.user);
    auth.session_id = Some(meta.id);
    auth.session = Some(meta);
    auth
}

/// A device step-up grant for `action`/`target` on the key device's session.
pub async fn device_grant(
    app: &TestApp,
    dev: &super::approvals::KeyDeviceFixture,
    action: vc_server::auth::step_up::StepUpAction,
    target: &str,
) {
    let auth = auth_user(app, dev.session_id.unwrap()).await;
    let target = vc_server::auth::step_up::StepUpTarget::parse(action, target).unwrap();
    let mut tx = app.pool.begin().await.unwrap();
    vc_server::auth::step_up::grant_tx(
        &mut tx,
        &auth,
        action,
        &target,
        vc_server::auth::step_up::StepUpVia::Device,
        vc_server::auth::strong::AuthenticatorRef::key_device(dev.row_id),
    )
    .await
    .expect("grant");
    tx.commit().await.unwrap();
}

/// Turn TOTP on for the user; returns the generator for its secret.
pub async fn enable_totp(
    pool: &PgPool,
    config: &Config,
    user_id: Uuid,
    username: &str,
) -> totp_rs::TOTP {
    use totp_rs::{Algorithm, Secret, TOTP};
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
        .expect("enable totp");
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret_str).to_bytes().expect("secret"),
        Some("Kaiku".to_string()),
        username.to_string(),
    )
    .expect("totp")
}

/// A user with the password `password` (Argon2id), returns `(id, username)`.
pub async fn password_user(pool: &PgPool, password: &str) -> (Uuid, String) {
    let (id, username) = super::create_test_user(pool).await;
    let hash = vc_server::auth::hash_password(password).expect("hash");
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&hash)
        .bind(id)
        .execute(pool)
        .await
        .expect("set password");
    (id, username)
}

/// Tokens of a fresh sign-in session for `user_id`, minted by the server's
/// `issue_session` as F3/F4 do after their second step (for tests whose
/// subject is not the sign-in flow itself): `(access, refresh, session_id)`.
pub async fn signed_in(
    state: &vc_server::api::AppState,
    user_id: Uuid,
    method: vc_server::auth::session_issue::SignInMethod,
) -> (String, String, Uuid) {
    let client = vc_server::auth::session_issue::SessionClient::default();
    let issued = vc_server::auth::session_issue::issue_session(
        &vc_server::auth::ctx::AuthCtx::from(state),
        &vc_server::auth::session_issue::NewSession {
            user_id,
            method,
            client: &client,
            client_class: vc_server::auth::client_class::ClientClass::Web,
            authenticator: None,
        },
    )
    .await
    .expect("issue session");
    (
        issued.tokens.access_token.clone(),
        issued.tokens.refresh_token.clone(),
        issued.session_id,
    )
}

/// A real F4 sign-in through the router: `POST /auth/login` (202
/// `LoginStep`), then the attempt's TOTP step; returns the TOTP step's
/// response (the `AuthResponse`, refresh token in the body for native
/// clients, the refresh cookie for browsers). `headers` go on both calls
/// (`X-Kaiku-Client`, `Origin`, …). The account's TOTP replay entry is
/// cleared first, so a test may sign the same account in several times.
pub async fn f4_sign_in(
    app: &TestApp,
    user_id: Uuid,
    username: &str,
    password: &str,
    totp: &totp_rs::TOTP,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    use axum::body::Body;
    use fred::interfaces::KeysInterface;
    let _: Result<(), _> = app
        .state
        .redis
        .del::<(), _>(format!("mfa:totp_step:{user_id}"))
        .await;
    let native = headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("x-kaiku-client") && *v != "web");
    let verifier = vc_server::approvals::secrets::b64u_encode(&rand::random::<[u8; 32]>());
    let challenge = vc_server::approvals::secrets::b64u_encode(
        &<sha2::Sha256 as sha2::Digest>::digest(verifier.as_bytes()),
    );
    let mut body = serde_json::json!({"username": username, "password": password});
    if native {
        body["client_challenge"] = serde_json::json!(challenge);
    }
    let mut req = peer_request(axum::http::Method::POST, "/auth/login")
        .header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = app
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await;
    assert_eq!(resp.status().as_u16(), 202, "password step");
    let cookie = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|c| c.contains("kaiku_attempt="))
        .map(|c| c.split(';').next().unwrap().to_string());
    let step = super::body_to_json(resp).await;
    let id = step["attempt"]["attempt_id"]
        .as_str()
        .expect("attempt")
        .to_string();
    let mut body = serde_json::json!({"code": totp.generate_current().unwrap()});
    if native {
        body["verifier"] = serde_json::json!(verifier);
    }
    let mut req = peer_request(
        axum::http::Method::POST,
        &format!("/auth/login-attempts/{id}/totp"),
    )
    .header("Content-Type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    if let Some(c) = cookie {
        req = req.header("Cookie", c);
    }
    app.oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
}
