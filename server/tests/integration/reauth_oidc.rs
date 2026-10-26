//! OIDC re-authentication for admin elevation (C24).
//!
//! A mock OIDC provider (discovery, authorize, token, userinfo, JWKS; RS256 ID
//! tokens with claims chosen per test) runs next to a real Kaiku server whose
//! public URL is configured. Each flow is driven by following the real
//! redirect chain like a browser would: Kaiku start endpoint -> provider
//! `/authorize` -> Kaiku `/auth/oidc/callback` -> the desktop loopback return
//! address, whose query carries the result.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use axum::extract::{Query, State};
use axum::response::Redirect;
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::traits::PublicKeyParts;
use serde_json::{json, Value};
use sqlx::PgPool;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::auth::oidc::OidcProviderManager;
use vc_server::config::Config;
use vc_server::ratelimit::{RateLimitConfig, RateLimiter, RateLimits};
use vc_server::voice::sfu::SfuServer;

use super::helpers::{create_session_with_token, make_admin, serve_test_router, TestServer};

const CLIENT_ID: &str = "kaiku-reauth-test";
const KEY_ID: &str = "test-key";
const PASSWORD: &str = "Admin-Pass-42";
/// Desktop loopback return address (nothing listens; the test reads the
/// redirect that points here).
const DESKTOP_RETURN: &str = "http://127.0.0.1:9/callback";

/// An RSA signing key: (PKCS#1 PEM, n, e) base64url.
fn rsa_key() -> (String, String, String) {
    let mut rng = rsa::rand_core::OsRng;
    let key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("generate RSA key");
    let pem = key
        .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
        .expect("PEM")
        .to_string();
    let n = URL_SAFE_NO_PAD.encode(key.n().to_bytes_be());
    let e = URL_SAFE_NO_PAD.encode(key.e().to_bytes_be());
    (pem, n, e)
}

/// The provider's published key.
static SIGNING_KEY: LazyLock<(String, String, String)> = LazyLock::new(rsa_key);
/// A key the provider never published (bad-signature test).
static OTHER_KEY: LazyLock<(String, String, String)> = LazyLock::new(rsa_key);

/// Claims of the next ID token. `iss`, `aud`, `iat`, `exp` and (unless
/// overridden) the nonce from the authorize request are filled in by the mock.
#[derive(Clone, Default)]
struct NextToken {
    sub: String,
    auth_time: Option<i64>,
    nonce_override: Option<String>,
    sign_with_other_key: bool,
}

#[derive(Clone)]
struct MockState {
    issuer: String,
    next: Arc<Mutex<Option<NextToken>>>,
    /// Nonce of each issued authorization code.
    codes: Arc<Mutex<HashMap<String, String>>>,
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

/// Approves at once: redirects to `redirect_uri` with a fresh code.
async fn authorize(
    State(mock): State<MockState>,
    Query(params): Query<HashMap<String, String>>,
) -> Redirect {
    let code = Uuid::new_v4().to_string();
    mock.codes.lock().expect("lock").insert(
        code.clone(),
        params.get("nonce").cloned().unwrap_or_default(),
    );
    let mut target = openidconnect::url::Url::parse(&params["redirect_uri"]).expect("redirect_uri");
    target
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &params["state"]);
    Redirect::temporary(target.as_str())
}

async fn jwks() -> Json<Value> {
    let (_, n, e) = &*SIGNING_KEY;
    Json(json!({
        "keys": [{ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KEY_ID, "n": n, "e": e }]
    }))
}

fn next_token(mock: &MockState) -> NextToken {
    mock.next
        .lock()
        .expect("lock")
        .clone()
        .expect("test must set the next ID token")
}

async fn token(
    State(mock): State<MockState>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let next = next_token(&mock);
    let nonce = next.nonce_override.clone().unwrap_or_else(|| {
        mock.codes
            .lock()
            .expect("lock")
            .get(&form["code"])
            .cloned()
            .unwrap_or_default()
    });
    let now = chrono::Utc::now().timestamp();
    let mut claims = json!({
        "iss": mock.issuer,
        "sub": next.sub,
        "aud": CLIENT_ID,
        "iat": now,
        "exp": now + 300,
        "nonce": nonce,
    });
    if let Some(auth_time) = next.auth_time {
        claims["auth_time"] = json!(auth_time);
    }
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(KEY_ID.to_string());
    let pem = if next.sign_with_other_key {
        &OTHER_KEY.0
    } else {
        &SIGNING_KEY.0
    };
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(pem.as_bytes()).expect("key");
    let id_token = jsonwebtoken::encode(&header, &claims, &key).expect("sign");
    Json(json!({
        "access_token": "mock-access-token",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
}

async fn userinfo(State(mock): State<MockState>) -> Json<Value> {
    Json(json!({ "sub": next_token(&mock).sub }))
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
    setup_with(pool, false).await
}

async fn setup_with(pool: &PgPool, rate_limited: bool) -> Ctx {
    // Mock provider
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock");
    let issuer = format!("http://{}", listener.local_addr().expect("addr"));
    let next = Arc::new(Mutex::new(None));
    let mock = MockState {
        issuer: issuer.clone(),
        next: next.clone(),
        codes: Arc::new(Mutex::new(HashMap::new())),
    };
    let mock_router = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/authorize", get(authorize))
        .route("/jwks", get(jwks))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo))
        .with_state(mock);
    tokio::spawn(async move {
        axum::serve(listener, mock_router)
            .await
            .expect("mock server");
    });

    // Kaiku: bind first so its public URL (the provider redirect) is known.
    let kaiku_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind kaiku");
    let kaiku_addr = kaiku_listener.local_addr().expect("addr");
    let kaiku_url = format!("http://{kaiku_addr}");
    let mut config = Config::default_for_test();
    config.public_url = Some(kaiku_url.clone());

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
    let rate_limiter = if rate_limited {
        let mut limiter = RateLimiter::new(
            redis.clone(),
            RateLimitConfig {
                enabled: true,
                redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
                fail_open: false,
                trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
                allowlist: std::collections::HashSet::new(),
                limits: RateLimits::default(),
            },
        );
        limiter.init().await.expect("limiter");
        Some(limiter)
    } else {
        None
    };
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("sfu");
    let state = AppState::new(AppStateConfig {
        db: pool.clone(),
        redis,
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter,
        screen_share_limiter: None,
        email: None,
        oidc_manager: Some(manager),
        http_client: reqwest::Client::new(),
    });
    let server = serve_test_router(kaiku_listener, create_router(state));
    assert_eq!(server.url, kaiku_url);
    Ctx {
        server,
        config: Arc::new(config),
        pool: pool.clone(),
        slug,
        next,
    }
}

/// Create a user (with [`PASSWORD`] unless `password` is false).
async fn create_user(pool: &PgPool, password: bool) -> Uuid {
    let username = format!("oidcre_{}", &Uuid::new_v4().simple().to_string()[..10]);
    let hash = if password {
        vc_server::auth::hash_password(PASSWORD).expect("hash")
    } else {
        String::new()
    };
    let user = vc_server::db::create_user(pool, &username, "Reauth", None, &hash)
        .await
        .expect("create user");
    if !password {
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

/// Turn MFA on; returns the TOTP generator.
async fn enable_mfa(ctx: &Ctx, user_id: Uuid) -> TOTP {
    let secret = Secret::default();
    let secret_str = secret.to_encoded().to_string();
    let key = hex::decode(ctx.config.mfa_encryption_key.as_ref().expect("key")).expect("hex");
    let encrypted =
        vc_server::auth::mfa_crypto::encrypt_mfa_secret(&secret_str, &key).expect("encrypt");
    sqlx::query("UPDATE users SET mfa_secret = $1 WHERE id = $2")
        .bind(&encrypted)
        .bind(user_id)
        .execute(&ctx.pool)
        .await
        .expect("enable mfa");
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret_str).to_bytes().expect("bytes"),
        Some("Kaiku".to_string()),
        "user".to_string(),
    )
    .expect("totp")
}

async fn link(ctx: &Ctx, user_id: Uuid, sub: &str) {
    vc_server::db::insert_user_identity(&ctx.pool, user_id, &ctx.slug, sub, None)
        .await
        .expect("link identity");
}

/// An admin with a linked identity `sub` and two sessions (both started after
/// the link); returns `(admin, (session_a, token_a), (session_b, token_b))`.
async fn admin_with_two_sessions(ctx: &Ctx, sub: &str) -> (Uuid, (Uuid, String), (Uuid, String)) {
    let admin = create_user(&ctx.pool, true).await;
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

fn query_of(url: &str) -> HashMap<String, String> {
    openidconnect::url::Url::parse(url)
        .expect("url")
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// GET a JSON start endpoint (`response=json`); returns `(status, body)`.
async fn start_json(ctx: &Ctx, path: &str, token: &str, extra: &[(&str, &str)]) -> (u16, Value) {
    let mut query = vec![("redirect_uri", DESKTOP_RETURN), ("response", "json")];
    query.extend_from_slice(extra);
    let resp = client()
        .get(format!("{}{path}", ctx.server.url))
        .query(&query)
        .bearer_auth(token)
        .send()
        .await
        .expect("start");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// POST the re-authentication start with a JSON body (or `None`: no body);
/// returns `(status, body)`.
async fn post_reauth_start(ctx: &Ctx, token: &str, body: Option<Value>) -> (u16, Value) {
    let mut req = client()
        .post(format!(
            "{}/auth/reauth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .bearer_auth(token);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.expect("start reauth");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Desktop start body with an optional TOTP code.
fn start_body(mfa_code: Option<&str>) -> Value {
    json!({ "redirect_uri": DESKTOP_RETURN, "reason": null, "mfa_code": mfa_code })
}

/// Start a re-authentication; returns the provider URL.
async fn start_reauth(ctx: &Ctx, token: &str, mfa_code: Option<&str>) -> String {
    let (status, body) = post_reauth_start(ctx, token, Some(start_body(mfa_code))).await;
    assert_eq!(status, 200, "start reauth: {body}");
    body["url"].as_str().expect("url").to_string()
}

/// Result of following a flow's redirect chain.
struct Chain {
    /// The Kaiku callback URL the provider sent the browser to.
    callback: String,
    /// Query of the final redirect to the desktop return address.
    result: HashMap<String, String>,
}

/// Follow the redirects from the provider URL until the desktop return
/// address (the server callback is never called by hand).
async fn follow(provider_url: &str) -> Chain {
    let mut url = provider_url.to_string();
    let mut callback = None;
    for _ in 0..5 {
        if url.starts_with(DESKTOP_RETURN) {
            return Chain {
                callback: callback.expect("passed the server callback"),
                result: query_of(&url),
            };
        }
        if url.contains("/auth/oidc/callback") {
            callback = Some(url.clone());
        }
        let resp = client().get(&url).send().await.expect("follow");
        assert!(
            resp.status().is_redirection(),
            "{url} answered {} instead of a redirect",
            resp.status()
        );
        url = resp
            .headers()
            .get("location")
            .and_then(|l| l.to_str().ok())
            .expect("location")
            .to_string();
    }
    panic!("redirect chain did not reach the desktop return address");
}

fn issue(ctx: &Ctx, next: NextToken) {
    *ctx.next.lock().expect("lock") = Some(next);
}

fn fresh(sub: &str) -> NextToken {
    NextToken {
        sub: sub.to_string(),
        auth_time: Some(now() - 5),
        ..NextToken::default()
    }
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

async fn methods(ctx: &Ctx, token: &str) -> Value {
    let resp = client()
        .get(format!("{}/auth/reauth/methods", ctx.server.url))
        .bearer_auth(token)
        .send()
        .await
        .expect("methods");
    assert_eq!(resp.status().as_u16(), 200);
    resp.json().await.expect("json")
}

/// A six-digit code not valid in any accepted step right now.
fn wrong_code(totp: &TOTP) -> String {
    let t = u64::try_from(now()).expect("time");
    let valid: Vec<String> = [t - 60, t - 30, t, t + 30, t + 60]
        .iter()
        .map(|s| totp.generate(*s))
        .collect();
    (0..1_000_000u32)
        .map(|n| format!("{n:06}"))
        .find(|c| !valid.contains(c))
        .expect("wrong code")
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn reauth_error(chain: &Chain) -> Option<&str> {
    chain.result.get("reauth_error").map(String::as_str)
}

async fn last_elevation_method(ctx: &Ctx, user_id: Uuid) -> Option<String> {
    sqlx::query_scalar(
        "SELECT details->>'method' FROM system_audit_log
         WHERE actor_id = $1 AND action = 'admin.session.elevated'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&ctx.pool)
    .await
    .expect("audit")
    .flatten()
}

#[sqlx::test]
async fn test_oidc_reauth_elevates_only_this_session(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (admin, (_, token_a), (_, token_b)) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let provider_url = start_reauth(&ctx, &token_a, None).await;
    let params = query_of(&provider_url);
    assert_eq!(params.get("prompt").map(String::as_str), Some("login"));
    assert_eq!(params.get("max_age").map(String::as_str), Some("0"));
    // The provider returns to the server, never to the desktop address.
    assert_eq!(
        params.get("redirect_uri").map(String::as_str),
        Some(format!("{}/auth/oidc/callback", ctx.server.url).as_str())
    );

    issue(&ctx, fresh("sub-admin"));
    let chain = follow(&provider_url).await;
    assert_eq!(
        chain.result.get("reauth").map(String::as_str),
        Some("elevated"),
        "{:?}",
        chain.result
    );
    assert!(chain.result.contains_key("expires_at"));

    assert!(is_elevated(&ctx, &token_a).await, "session A is elevated");
    assert!(!is_elevated(&ctx, &token_b).await, "session B is not");
    assert_eq!(
        last_elevation_method(&ctx, admin).await.as_deref(),
        Some("oidc")
    );
}

#[sqlx::test]
async fn test_login_flow_does_not_force_a_fresh_login(pool: PgPool) {
    let ctx = setup(&pool).await;
    let resp = client()
        .get(format!(
            "{}/auth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&[("redirect_uri", DESKTOP_RETURN), ("response", "json")])
        .send()
        .await
        .expect("authorize");
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.expect("json");
    let params = query_of(body["url"].as_str().expect("url"));
    assert!(!params.contains_key("prompt"), "login: no prompt=login");
    assert!(!params.contains_key("max_age"), "login: no max_age");
}

#[sqlx::test]
async fn test_oidc_reauth_requires_auth_time(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let url = start_reauth(&ctx, &token, None).await;
    issue(
        &ctx,
        NextToken {
            auth_time: None,
            ..fresh("sub-admin")
        },
    );
    let chain = follow(&url).await;
    assert_eq!(reauth_error(&chain), Some("REAUTH_AUTH_TIME_MISSING"));
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_rejects_stale_or_future_auth_time(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    for auth_time in [now() - 6 * 60, now() + 120] {
        let url = start_reauth(&ctx, &token, None).await;
        issue(
            &ctx,
            NextToken {
                auth_time: Some(auth_time),
                ..fresh("sub-admin")
            },
        );
        let chain = follow(&url).await;
        assert_eq!(
            reauth_error(&chain),
            Some("REAUTH_STALE"),
            "auth_time {auth_time}"
        );
    }
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_rejects_bad_signature_and_wrong_nonce(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let bad_signature = NextToken {
        sign_with_other_key: true,
        ..fresh("sub-admin")
    };
    let wrong_nonce = NextToken {
        nonce_override: Some("not-the-nonce".to_string()),
        ..fresh("sub-admin")
    };
    for next in [bad_signature, wrong_nonce] {
        let url = start_reauth(&ctx, &token, None).await;
        issue(&ctx, next);
        let chain = follow(&url).await;
        assert_eq!(reauth_error(&chain), Some("OIDC_EXCHANGE_FAILED"));
    }
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_rejects_sub_of_another_user(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;
    let other = create_user(&ctx.pool, false).await;
    link(&ctx, other, "sub-other").await;

    for sub in ["sub-other", "sub-unlinked"] {
        let url = start_reauth(&ctx, &token, None).await;
        issue(&ctx, fresh(sub));
        let chain = follow(&url).await;
        assert_eq!(
            reauth_error(&chain),
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

    // Reused state: replaying the callback finds no state.
    let url = start_reauth(&ctx, &token_a, None).await;
    issue(&ctx, fresh("sub-admin"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("reauth").map(String::as_str),
        Some("elevated")
    );
    let resp = client().get(&chain.callback).send().await.expect("replay");
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
    let url = start_reauth(&ctx, &token_a, None).await;
    let resp = client()
        .delete(format!("{}/auth/sessions/{session_a}", ctx.server.url))
        .bearer_auth(&token_b)
        .send()
        .await
        .expect("revoke A");
    assert!(resp.status().is_success(), "revoke session A");
    issue(&ctx, fresh("sub-admin"));
    let chain = follow(&url).await;
    assert_eq!(reauth_error(&chain), Some("REAUTH_SESSION_INVALID"));
    assert!(
        !is_elevated(&ctx, &token_b).await,
        "session B is not elevated"
    );
}

#[sqlx::test]
async fn test_link_and_reauth_flows_do_not_cross(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (admin, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;
    // An admin links only from an elevated session.
    let resp = client()
        .post(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token)
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .expect("elevate");
    assert_eq!(resp.status().as_u16(), 200);

    // A link flow (same provider and sub) only links: no elevation, no login.
    let (status, body) = start_json(
        &ctx,
        &format!("/auth/me/identities/authorize/{}", ctx.slug),
        &token,
        &[],
    )
    .await;
    assert_eq!(status, 200, "{body}");
    issue(&ctx, fresh("sub-admin"));
    let chain = follow(body["url"].as_str().expect("url")).await;
    assert_eq!(
        chain.result.get("linked"),
        Some(&ctx.slug),
        "{:?}",
        chain.result
    );
    assert!(
        !chain.result.contains_key("access_token"),
        "a link never logs in"
    );
    assert!(!chain.result.contains_key("reauth"));
    let resp = client()
        .delete(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);
    assert!(!is_elevated(&ctx, &token).await);

    // A reauth flow only elevates: no tokens, no new link.
    let url = start_reauth(&ctx, &token, None).await;
    issue(&ctx, fresh("sub-admin"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("reauth").map(String::as_str),
        Some("elevated")
    );
    assert!(
        !chain.result.contains_key("access_token"),
        "reauth never logs in"
    );
    assert!(!chain.result.contains_key("linked"), "reauth never links");
    let identities: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_identities WHERE user_id = $1")
            .bind(admin)
            .fetch_one(&ctx.pool)
            .await
            .expect("count");
    assert_eq!(identities, 1);
}

#[sqlx::test]
async fn test_non_elevated_admin_cannot_link_and_late_identity_cannot_elevate(pool: PgPool) {
    let ctx = setup(&pool).await;
    let admin = create_user(&ctx.pool, true).await;
    make_admin(&ctx.pool, admin).await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, admin).await;

    // (1) A non-elevated admin session cannot start a link: the same 403 as
    // the admin middleware gives for non-elevated access.
    let (status, body) = start_json(
        &ctx,
        &format!("/auth/me/identities/authorize/{}", ctx.slug),
        &token,
        &[],
    )
    .await;
    assert_eq!(
        (status, body["error"].clone()),
        (403, json!("elevation_required")),
        "{body}"
    );

    // (2) An identity that reached the account some other way after this
    // session started, not while elevated, cannot elevate this session.
    vc_server::db::insert_user_identity(&ctx.pool, admin, &ctx.slug, "sub-new", None)
        .await
        .expect("insert identity");
    assert_eq!(methods(&ctx, &token).await["oidc"], json!([]));
    let (status, body) = post_reauth_start(&ctx, &token, Some(start_body(None))).await;
    assert_eq!(
        (status, body["error"].clone()),
        (400, json!("REAUTH_PROVIDER_UNAVAILABLE"))
    );

    // A session that starts after the link may use it (eligibility rule:
    // linked no later than the session's start, or while elevated).
    let (_, later) = create_session_with_token(&ctx.pool, &ctx.config, admin).await;
    assert_eq!(
        methods(&ctx, &later).await["oidc"][0]["provider_id"],
        json!(ctx.slug)
    );
}

/// Start a link for `token`; returns the provider URL.
async fn start_link(ctx: &Ctx, token: &str) -> String {
    let (status, body) = start_json(
        ctx,
        &format!("/auth/me/identities/authorize/{}", ctx.slug),
        token,
        &[],
    )
    .await;
    assert_eq!(status, 200, "start link: {body}");
    body["url"].as_str().expect("url").to_string()
}

async fn identity_count(ctx: &Ctx, user_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM user_identities WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(&ctx.pool)
        .await
        .expect("count")
}

/// The link callback re-checks the session that started the flow: once it is
/// revoked, the link is refused and no identity is inserted.
#[sqlx::test]
async fn test_link_refused_after_starting_session_revoked(pool: PgPool) {
    let ctx = setup(&pool).await;
    let user = create_user(&ctx.pool, true).await;
    let (session_a, token_a) = create_session_with_token(&ctx.pool, &ctx.config, user).await;
    let (_, token_b) = create_session_with_token(&ctx.pool, &ctx.config, user).await;

    let url = start_link(&ctx, &token_a).await;
    let resp = client()
        .delete(format!("{}/auth/sessions/{session_a}", ctx.server.url))
        .bearer_auth(&token_b)
        .send()
        .await
        .expect("revoke A");
    assert!(resp.status().is_success(), "revoke session A");

    issue(&ctx, fresh("sub-link"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("link_error").map(String::as_str),
        Some("LINK_SESSION_INVALID"),
        "{:?}",
        chain.result
    );
    assert!(!chain.result.contains_key("linked"));
    assert_eq!(identity_count(&ctx, user).await, 0, "no identity inserted");
}

/// A link state that names no session (legacy `link_user_id` form) is
/// refused in the callback.
#[sqlx::test]
async fn test_legacy_link_state_without_session_is_refused(pool: PgPool) {
    use sha2::{Digest, Sha256};
    use vc_server::auth::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};

    let ctx = setup(&pool).await;
    let user = create_user(&ctx.pool, true).await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, user).await;
    let url = start_link(&ctx, &token).await;

    // Rewrite the stored state into the legacy form (bare `link_user_id`).
    let state = query_of(&url).remove("state").expect("state");
    let key = format!(
        "oidc:state:{}",
        hex::encode(Sha256::digest(state.as_bytes()))
    );
    let enc_key = hex::decode(ctx.config.mfa_encryption_key.as_ref().expect("key")).expect("hex");
    let redis = vc_server::db::create_redis_client(&ctx.config.redis_url)
        .await
        .expect("Valkey");
    let stored: String = fred::interfaces::KeysInterface::get(&redis, &key)
        .await
        .expect("GET state");
    let mut flow: Value =
        serde_json::from_str(&decrypt_mfa_secret(&stored, &enc_key).expect("decrypt"))
            .expect("json");
    let object = flow.as_object_mut().expect("object");
    object.remove("purpose");
    object.insert("link_user_id".to_string(), json!(user));
    let rewritten = encrypt_mfa_secret(&flow.to_string(), &enc_key).expect("encrypt");
    let _: () = fred::interfaces::KeysInterface::set(&redis, &key, rewritten, None, None, false)
        .await
        .expect("SET state");

    issue(&ctx, fresh("sub-link"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("link_error").map(String::as_str),
        Some("LINK_SESSION_INVALID"),
        "{:?}",
        chain.result
    );
    assert_eq!(identity_count(&ctx, user).await, 0, "no identity inserted");
}

#[sqlx::test]
async fn test_identity_linked_while_elevated_can_elevate(pool: PgPool) {
    let ctx = setup(&pool).await;
    let admin = create_user(&ctx.pool, true).await;
    make_admin(&ctx.pool, admin).await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, admin).await;
    let resp = client()
        .post(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token)
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .expect("elevate");
    assert_eq!(resp.status().as_u16(), 200);

    let (_, body) = start_json(
        &ctx,
        &format!("/auth/me/identities/authorize/{}", ctx.slug),
        &token,
        &[],
    )
    .await;
    issue(&ctx, fresh("sub-new"));
    let chain = follow(body["url"].as_str().expect("url")).await;
    assert_eq!(chain.result.get("linked"), Some(&ctx.slug));

    assert_eq!(
        methods(&ctx, &token).await["oidc"][0]["provider_id"],
        json!(ctx.slug)
    );

    // Full chain: after de-elevating, the same session elevates again through
    // the identity it linked while elevated.
    let resp = client()
        .delete(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);
    assert!(!is_elevated(&ctx, &token).await);
    let url = start_reauth(&ctx, &token, None).await;
    issue(&ctx, fresh("sub-new"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("reauth").map(String::as_str),
        Some("elevated"),
        "{:?}",
        chain.result
    );
    assert!(is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_oidc_reauth_requires_totp_when_mfa_enabled(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (admin, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;
    let totp = enable_mfa(&ctx, admin).await;
    let (status, body) = post_reauth_start(&ctx, &token, Some(start_body(None))).await;
    assert_eq!(
        (status, body["error"].clone()),
        (400, json!("MFA_CODE_REQUIRED"))
    );
    let (status, body) =
        post_reauth_start(&ctx, &token, Some(start_body(Some(&wrong_code(&totp))))).await;
    assert_eq!(
        (status, body["error"].clone()),
        (401, json!("INVALID_CREDENTIALS"))
    );

    let code = totp.generate_current().expect("code");
    let url = start_reauth(&ctx, &token, Some(&code)).await;
    issue(&ctx, fresh("sub-admin"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("reauth").map(String::as_str),
        Some("elevated")
    );
    assert_eq!(
        last_elevation_method(&ctx, admin).await.as_deref(),
        Some("oidc+totp")
    );

    // The same code cannot start another re-authentication.
    let (status, _) = post_reauth_start(&ctx, &token, Some(start_body(Some(&code)))).await;
    assert_eq!(status, 401, "replayed TOTP code");
}

#[sqlx::test]
async fn test_failed_password_attempts_lock_oidc_start(pool: PgPool) {
    let ctx = setup_with(&pool, true).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    for attempt in 1..=10 {
        let resp = client()
            .post(format!("{}/api/admin/elevate", ctx.server.url))
            .bearer_auth(&token)
            .json(&json!({ "password": "wrong-password" }))
            .send()
            .await
            .expect("elevate");
        assert_eq!(resp.status().as_u16(), 401, "attempt {attempt}");
    }
    let (status, _) = post_reauth_start(&ctx, &token, Some(start_body(None))).await;
    assert_eq!(status, 429);
}

#[sqlx::test]
async fn test_oidc_reauth_start_takes_a_json_body(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    // No body, or invalid JSON: 400 VALIDATION_ERROR.
    let (status, body) = post_reauth_start(&ctx, &token, None).await;
    assert_eq!(
        (status, body["error"].clone()),
        (400, json!("VALIDATION_ERROR"))
    );
    let resp = client()
        .post(format!(
            "{}/auth/reauth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .bearer_auth(&token)
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .expect("start");
    assert_eq!(resp.status().as_u16(), 400);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["error"], "VALIDATION_ERROR");

    // The answer is always {"url"}; the old GET form is gone.
    let (status, body) = post_reauth_start(&ctx, &token, Some(json!({}))).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["url"].is_string());
    let resp = client()
        .get(format!(
            "{}/auth/reauth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("get");
    assert_eq!(resp.status().as_u16(), 405);
}

#[sqlx::test]
async fn test_oidc_reauth_callback_requires_totp_enabled_after_start(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (admin, (_, token), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    // Started without MFA; MFA is turned on before the provider returns.
    let url = start_reauth(&ctx, &token, None).await;
    enable_mfa(&ctx, admin).await;
    issue(&ctx, fresh("sub-admin"));
    let chain = follow(&url).await;
    assert_eq!(reauth_error(&chain), Some("MFA_CODE_REQUIRED"));
    assert!(!is_elevated(&ctx, &token).await);
}

#[sqlx::test]
async fn test_reauth_methods_match_account(pool: PgPool) {
    let ctx = setup(&pool).await;
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
    let full = create_user(&ctx.pool, true).await;
    enable_mfa(&ctx, full).await;
    link(&ctx, full, "sub-full").await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, full).await;
    assert_eq!(
        methods(&ctx, &token).await,
        json!({
            "password": true,
            "mfa_required": true,
            "oidc": [{ "provider_id": ctx.slug, "display_name": "Mock SSO" }],
        })
    );

    // Provider only: the password path points to the provider.
    let sso_admin = create_user(&ctx.pool, false).await;
    make_admin(&ctx.pool, sso_admin).await;
    link(&ctx, sso_admin, "sub-sso").await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, sso_admin).await;
    assert_eq!(
        methods(&ctx, &token).await,
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
    let bare_admin = create_user(&ctx.pool, false).await;
    make_admin(&ctx.pool, bare_admin).await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, bare_admin).await;
    assert_eq!(
        methods(&ctx, &token).await,
        json!({ "password": false, "mfa_required": false, "oidc": [] })
    );
    let (status, body) = elevate(token).await;
    assert_eq!(
        (status, body["error"].clone()),
        (400, json!("REAUTH_UNAVAILABLE"))
    );
}

/// A provider error (callback without `code`) on a re-authentication state
/// hands back `reauth_error=OIDC_PROVIDER_ERROR` to the desktop return
/// address, like the login and link flows, and elevates nothing.
#[sqlx::test]
async fn test_oidc_reauth_provider_error_hands_back(pool: PgPool) {
    let ctx = setup(&pool).await;
    let (_, (_, token_a), _) = admin_with_two_sessions(&ctx, "sub-admin").await;

    let provider_url = start_reauth(&ctx, &token_a, None).await;
    let state = query_of(&provider_url)
        .remove("state")
        .expect("state parameter");
    let resp = client()
        .get(format!("{}/auth/oidc/callback", ctx.server.url))
        .query(&[("state", state.as_str()), ("error", "access_denied")])
        .send()
        .await
        .expect("callback");
    assert!(resp.status().is_redirection(), "{}", resp.status());
    let location = resp
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .expect("location")
        .to_string();
    assert!(location.starts_with(DESKTOP_RETURN), "{location}");
    let result = query_of(&location);
    assert_eq!(
        result.get("reauth_error").map(String::as_str),
        Some("OIDC_PROVIDER_ERROR")
    );
    assert!(!result.contains_key("error"), "not a login hand-back");
    assert!(!is_elevated(&ctx, &token_a).await);
}

/// An admin whose session is no longer elevated when the provider returns
/// cannot finish a link started while elevated.
#[sqlx::test]
async fn test_admin_link_refused_when_elevation_ended_before_callback(pool: PgPool) {
    let ctx = setup(&pool).await;
    let admin = create_user(&ctx.pool, true).await;
    make_admin(&ctx.pool, admin).await;
    let (_, token) = create_session_with_token(&ctx.pool, &ctx.config, admin).await;
    let resp = client()
        .post(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token)
        .json(&json!({ "password": PASSWORD }))
        .send()
        .await
        .expect("elevate");
    assert_eq!(resp.status().as_u16(), 200);
    let url = start_link(&ctx, &token).await;

    let resp = client()
        .delete(format!("{}/api/admin/elevate", ctx.server.url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("de-elevate");
    assert_eq!(resp.status().as_u16(), 200);

    issue(&ctx, fresh("sub-new"));
    let chain = follow(&url).await;
    assert_eq!(
        chain.result.get("link_error").map(String::as_str),
        Some("elevation_required"),
        "{:?}",
        chain.result
    );
    assert_eq!(identity_count(&ctx, admin).await, 0, "no identity inserted");
}
