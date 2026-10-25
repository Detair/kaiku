//! OIDC re-authentication for admin elevation (C24).
//!
//! A mock OIDC provider (discovery document, JWKS, token endpoint issuing
//! RS256 ID tokens) runs next to a real HTTP server. Each test starts a
//! re-authentication with `GET /auth/reauth/oidc/authorize/{provider}`, lets
//! the mock issue an ID token with chosen claims, and drives the shared
//! callback `GET /auth/oidc/callback` like the desktop client's localhost
//! listener would.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::auth::oidc::OidcProviderManager;
use vc_server::config::Config;
use vc_server::voice::sfu::SfuServer;

use super::helpers::{create_session_with_token, make_admin, spawn_test_server, TestServer};

const CLIENT_ID: &str = "kaiku-reauth-test";
const KEY_ID: &str = "test-key";
/// Localhost redirect, as the desktop client sends it (nothing listens).
const DESKTOP_REDIRECT: &str = "http://127.0.0.1:9/callback";

/// RSA signing key of the mock provider: (PKCS#1 PEM, n, e) base64url.
static SIGNING_KEY: LazyLock<(String, String, String)> = LazyLock::new(|| {
    let mut rng = rsa::rand_core::OsRng;
    let key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("generate RSA key");
    let pem = key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .expect("PEM")
        .to_string();
    let n = URL_SAFE_NO_PAD.encode(key.n().to_bytes_be());
    let e = URL_SAFE_NO_PAD.encode(key.e().to_bytes_be());
    (pem, n, e)
});

/// Claims the mock puts into the next ID token (`iss`, `aud`, `iat`, `exp`
/// are added by the mock).
#[derive(Clone)]
struct NextToken {
    sub: String,
    nonce: String,
    auth_time: Option<i64>,
}

#[derive(Clone)]
struct MockState {
    issuer: String,
    next: Arc<Mutex<Option<NextToken>>>,
}

async fn discovery(State(mock): State<MockState>) -> Json<Value> {
    let issuer = &mock.issuer;
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "userinfo_endpoint": format!("{issuer}/userinfo"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn jwks() -> Json<Value> {
    let (_, n, e) = &*SIGNING_KEY;
    Json(json!({
        "keys": [{ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KEY_ID, "n": n, "e": e }]
    }))
}

async fn token(State(mock): State<MockState>) -> Json<Value> {
    let next = mock
        .next
        .lock()
        .expect("lock")
        .clone()
        .expect("test must set the next ID token");
    let now = chrono::Utc::now().timestamp();
    let mut claims = json!({
        "iss": mock.issuer,
        "sub": next.sub,
        "aud": CLIENT_ID,
        "iat": now,
        "exp": now + 300,
        "nonce": next.nonce,
    });
    if let Some(auth_time) = next.auth_time {
        claims["auth_time"] = json!(auth_time);
    }
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(KEY_ID.to_string());
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(SIGNING_KEY.0.as_bytes()).expect("key");
    let id_token = jsonwebtoken::encode(&header, &claims, &key).expect("sign");
    Json(json!({
        "access_token": "mock-access-token",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
}

struct Ctx {
    server: TestServer,
    config: Arc<Config>,
    pool: PgPool,
    slug: String,
    next: Arc<Mutex<Option<NextToken>>>,
}

/// Start the mock provider and a Kaiku server that has it configured.
async fn setup(pool: &PgPool) -> Ctx {
    // Mock provider
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let issuer = format!("http://{}", listener.local_addr().expect("addr"));
    let next = Arc::new(Mutex::new(None));
    let mock = MockState {
        issuer: issuer.clone(),
        next: next.clone(),
    };
    let mock_router = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .with_state(mock);
    tokio::spawn(async move {
        axum::serve(listener, mock_router)
            .await
            .expect("mock server");
    });

    // Kaiku with the provider configured (the provider redirect is the
    // server callback under PUBLIC_URL; tests call the callback directly).
    let mut config = Config::default_for_test();
    config.public_url = Some("http://127.0.0.1".to_string());
    let key = hex::decode(config.mfa_encryption_key.as_ref().expect("key")).expect("hex");
    let manager = OidcProviderManager::new(key);
    let slug = format!("mock-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let secret = manager.encrypt_secret("client-secret").expect("encrypt");
    vc_server::db::create_oidc_provider(
        pool,
        vc_server::db::CreateOidcProviderParams {
            slug: &slug,
            display_name: "Mock SSO",
            icon_hint: Some("key"),
            provider_type: "custom",
            issuer_url: Some(&issuer),
            authorization_url: None,
            token_url: None,
            userinfo_url: None,
            client_id: CLIENT_ID,
            client_secret_encrypted: &secret,
            scopes: "openid",
            created_by: None,
        },
    )
    .await
    .expect("create provider");
    manager.load_providers(pool).await.expect("load providers");
    assert!(
        manager.issues_id_tokens(&slug).await,
        "discovery must succeed"
    );

    let mut methods = vc_server::db::get_auth_methods_allowed(pool)
        .await
        .expect("methods");
    methods.oidc = true;
    vc_server::db::set_auth_methods_allowed(pool, &methods, None)
        .await
        .expect("enable oidc");

    let redis = vc_server::db::create_redis_client(&config.redis_url)
        .await
        .expect("redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("sfu");
    let state = AppState::new(AppStateConfig {
        db: pool.clone(),
        redis,
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter: None,
        screen_share_limiter: None,
        email: None,
        oidc_manager: Some(manager),
        http_client: reqwest::Client::new(),
    });
    let server = spawn_test_server(create_router(state)).await;
    Ctx {
        server,
        config: Arc::new(config),
        pool: pool.clone(),
        slug,
        next,
    }
}

/// Create a user (with a password unless `password` is `None`).
async fn create_user(pool: &PgPool, password: Option<&str>) -> Uuid {
    let username = format!("oidcre_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let hash = password.map_or_else(String::new, |p| {
        vc_server::auth::hash_password(p).expect("hash")
    });
    let user = vc_server::db::create_user(pool, &username, "Reauth", None, &hash)
        .await
        .expect("create user");
    if password.is_none() {
        sqlx::query(
            "UPDATE users SET password_hash = NULL, auth_method = 'oidc', external_id = id::text WHERE id = $1",
        )
            .bind(user.id)
            .execute(pool)
            .await
            .expect("clear password");
    }
    user.id
}

async fn link(ctx: &Ctx, user_id: Uuid, sub: &str) {
    vc_server::db::insert_user_identity(&ctx.pool, user_id, &ctx.slug, sub, None)
        .await
        .expect("link identity");
}

/// An admin with a linked identity `sub` and two sessions; returns
/// `(admin, (session_a, token_a), (session_b, token_b))`.
async fn admin_with_two_sessions(ctx: &Ctx, sub: &str) -> (Uuid, (Uuid, String), (Uuid, String)) {
    let admin = create_user(&ctx.pool, Some("Admin-Pass-42")).await;
    make_admin(&ctx.pool, admin).await;
    link(ctx, admin, sub).await;
    let a = create_session_with_token(&ctx.pool, &ctx.config, admin).await;
    let b = create_session_with_token(&ctx.pool, &ctx.config, admin).await;
    (admin, a, b)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

/// Start a re-authentication; returns the provider URL's query parameters.
async fn start(ctx: &Ctx, token: &str) -> HashMap<String, String> {
    let resp = client()
        .get(format!(
            "{}/auth/reauth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&[("redirect_uri", DESKTOP_REDIRECT), ("response", "json")])
        .bearer_auth(token)
        .send()
        .await
        .expect("start");
    assert_eq!(resp.status().as_u16(), 200, "start reauth");
    let body: Value = resp.json().await.expect("json");
    let url =
        openidconnect::url::Url::parse(body["url"].as_str().expect("url")).expect("parse url");
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Run the callback for `state`; returns `(status, query of the redirect)`.
async fn callback(ctx: &Ctx, state: &str) -> (u16, HashMap<String, String>) {
    let resp = client()
        .get(format!("{}/auth/oidc/callback", ctx.server.url))
        .query(&[("code", "mock-code"), ("state", state)])
        .send()
        .await
        .expect("callback");
    let status = resp.status().as_u16();
    let query = resp
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .and_then(|l| openidconnect::url::Url::parse(l).ok())
        .map(|u| {
            u.query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default();
    (status, query)
}

fn issue(ctx: &Ctx, sub: &str, nonce: &str, auth_time: Option<i64>) {
    *ctx.next.lock().expect("lock") = Some(NextToken {
        sub: sub.to_string(),
        nonce: nonce.to_string(),
        auth_time,
    });
}

async fn is_elevated(ctx: &Ctx, token: &str) -> bool {
    let resp = client()
        .get(format!("{}/api/admin/reports", ctx.server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("reports");
    resp.status().as_u16() == 200
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[sqlx::test]
async fn test_oidc_reauth_elevates_only_this_session(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token_a), (_, token_b)) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let params = start(&ctx, &token_a).await;
    assert_eq!(params.get("prompt").map(String::as_str), Some("login"));
    assert_eq!(params.get("max_age").map(String::as_str), Some("0"));

    issue(&ctx, "sub-admin", &params["nonce"], Some(now() - 10));
    let (status, query) = callback(&ctx, &params["state"]).await;
    assert_eq!(status, 307);
    assert_eq!(
        query.get("reauth").map(String::as_str),
        Some("elevated"),
        "{query:?}"
    );
    assert!(query.contains_key("expires_at"));

    assert!(is_elevated(&ctx, &token_a).await, "session A is elevated");
    assert!(!is_elevated(&ctx, &token_b).await, "session B is not");
}

#[sqlx::test]
async fn test_oidc_reauth_requires_auth_time(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let params = start(&ctx, &token).await;
    issue(&ctx, "sub-admin", &params["nonce"], None);
    let (_, query) = callback(&ctx, &params["state"]).await;
    assert_eq!(
        query.get("reauth_error").map(String::as_str),
        Some("REAUTH_AUTH_TIME_MISSING")
    );
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_rejects_stale_auth_time(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let params = start(&ctx, &token).await;
    issue(&ctx, "sub-admin", &params["nonce"], Some(now() - 6 * 60));
    let (_, query) = callback(&ctx, &params["state"]).await;
    assert_eq!(
        query.get("reauth_error").map(String::as_str),
        Some("REAUTH_STALE")
    );
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_rejects_sub_of_another_user(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;
    let other = create_user(&ctx.pool, None).await;
    link(&ctx, other, "sub-other").await;

    for sub in ["sub-other", "sub-unlinked"] {
        let params = start(&ctx, &token).await;
        issue(&ctx, sub, &params["nonce"], Some(now()));
        let (_, query) = callback(&ctx, &params["state"]).await;
        assert_eq!(
            query.get("reauth_error").map(String::as_str),
            Some("REAUTH_IDENTITY_MISMATCH"),
            "sub {sub}"
        );
    }
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_state_is_single_use_and_session_bound(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (session_a, token_a), (_, token_b)) = admin_with_two_sessions(&ctx, "sub-admin").await;

    // Reused state: the second callback finds no state.
    let params = start(&ctx, &token_a).await;
    issue(&ctx, "sub-admin", &params["nonce"], Some(now()));
    let (_, query) = callback(&ctx, &params["state"]).await;
    assert_eq!(query.get("reauth").map(String::as_str), Some("elevated"));
    let resp = client()
        .get(format!("{}/auth/oidc/callback", ctx.server.url))
        .query(&[("code", "mock-code"), ("state", params["state"].as_str())])
        .send()
        .await
        .expect("replay");
    assert_eq!(resp.status().as_u16(), 400);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["error"], "OIDC_STATE_MISMATCH");

    // A state bound to a session that has ended elevates nothing, and never
    // another session of the same admin.
    let resp = client()
        .delete(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token_a)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);
    let params = start(&ctx, &token_a).await;
    let resp = client()
        .delete(format!("{}/auth/sessions/{session_a}", ctx.server.url))
        .bearer_auth(&token_b)
        .send()
        .await
        .expect("revoke A");
    assert!(resp.status().is_success(), "revoke session A");
    issue(&ctx, "sub-admin", &params["nonce"], Some(now()));
    let (_, query) = callback(&ctx, &params["state"]).await;
    assert_eq!(
        query.get("reauth_error").map(String::as_str),
        Some("REAUTH_SESSION_INVALID")
    );
    assert!(
        !is_elevated(&ctx, &token_b).await,
        "session B is not elevated"
    );
}

#[sqlx::test]
async fn test_reauth_methods_match_account(pool: PgPool) {
    let ctx = setup(&pool).await;
    let methods = |token: String| {
        let url = format!("{}/auth/reauth/methods", ctx.server.url);
        async move {
            let resp = client()
                .get(url)
                .bearer_auth(token)
                .send()
                .await
                .expect("methods");
            assert_eq!(resp.status().as_u16(), 200);
            resp.json::<Value>().await.expect("json")
        }
    };
    let elevate = |token: String| {
        let url = format!("{}/api/admin/elevate", ctx.server.url);
        async move {
            let resp = client()
                .post(url)
                .bearer_auth(token)
                .json(&json!({ "password": "anything" }))
                .send()
                .await
                .expect("elevate");
            let status = resp.status().as_u16();
            (status, resp.json::<Value>().await.expect("json"))
        }
    };

    // Password + MFA + linked provider.
    let full = create_user(&ctx.pool, Some("Full-Pass-42")).await;
    sqlx::query("UPDATE users SET mfa_secret = 'x' WHERE id = $1")
        .bind(full)
        .execute(&ctx.pool)
        .await
        .expect("mfa on");
    link(&ctx, full, "sub-full").await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, full).await;
    assert_eq!(
        methods(token).await,
        json!({
            "password": true,
            "mfa_required": true,
            "oidc": [{ "provider_id": ctx.slug, "display_name": "Mock SSO" }],
        })
    );

    // Provider only: the password path points to the provider.
    let sso_admin = create_user(&ctx.pool, None).await;
    make_admin(&ctx.pool, sso_admin).await;
    link(&ctx, sso_admin, "sub-sso").await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, sso_admin).await;
    assert_eq!(
        methods(token.clone()).await,
        json!({
            "password": false,
            "mfa_required": false,
            "oidc": [{ "provider_id": ctx.slug, "display_name": "Mock SSO" }],
        })
    );
    let (status, body) = elevate(token).await;
    assert_eq!(
        (status, body["error"].clone()),
        (400, json!("REAUTH_OIDC_REQUIRED"))
    );

    // Neither a password nor a linked provider.
    let bare_admin = create_user(&ctx.pool, None).await;
    make_admin(&ctx.pool, bare_admin).await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, bare_admin).await;
    assert_eq!(
        methods(token.clone()).await,
        json!({ "password": false, "mfa_required": false, "oidc": [] })
    );
    let (status, body) = elevate(token).await;
    assert_eq!(
        (status, body["error"].clone()),
        (400, json!("REAUTH_UNAVAILABLE"))
    );
}
