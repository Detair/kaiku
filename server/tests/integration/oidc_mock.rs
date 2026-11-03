//! A mock OIDC provider for the OIDC flow tests (shared with `oidc_handoff`).
//! The OIDC re-authentication tests (C24) were removed with the feature
//! (v1.0: elevation is a step-up, F7).
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
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::auth::oidc::OidcProviderManager;
use vc_server::config::Config;
use vc_server::ratelimit::{RateLimitConfig, RateLimiter, RateLimits};
use vc_server::voice::sfu::SfuServer;

use super::helpers::{serve_test_router, TestServer};

const CLIENT_ID: &str = "kaiku-reauth-test";
const KEY_ID: &str = "test-key";
const PASSWORD: &str = "Admin-Pass-42";
/// Desktop loopback return address (nothing listens; the test reads the
/// redirect that points here).
pub(super) const DESKTOP_RETURN: &str = "http://127.0.0.1:9/callback";

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
pub(super) struct NextToken {
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

pub(super) struct Ctx {
    pub(super) server: TestServer,
    pub(super) config: Arc<Config>,
    pub(super) pool: PgPool,
    pub(super) slug: String,
    next: Arc<Mutex<Option<NextToken>>>,
}

/// Start the mock provider and a Kaiku server that has it configured.
pub(super) async fn setup(pool: &PgPool) -> Ctx {
    setup_with(pool, false).await
}

pub(super) async fn setup_with(pool: &PgPool, rate_limited: bool) -> Ctx {
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
pub(super) async fn create_user(pool: &PgPool, password: bool) -> Uuid {
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

pub(super) async fn link(ctx: &Ctx, user_id: Uuid, sub: &str) {
    vc_server::db::insert_user_identity(&ctx.pool, user_id, &ctx.slug, sub, None)
        .await
        .expect("link identity");
}

pub(super) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

pub(super) fn query_of(url: &str) -> HashMap<String, String> {
    openidconnect::url::Url::parse(url)
        .expect("url")
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Result of following a flow's redirect chain.
pub(super) struct Chain {
    /// The Kaiku callback URL the provider sent the browser to.
    #[allow(dead_code)]
    pub(super) callback: String,
    /// Query of the final redirect to the desktop return address.
    pub(super) result: HashMap<String, String>,
}

/// Follow the provider's redirect to the server callback and return the
/// callback's own response (the v1.0 hand-off page or popup page).
pub(super) async fn callback_response(provider_url: &str) -> reqwest::Response {
    let resp = client().get(provider_url).send().await.expect("provider");
    let callback = resp
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .expect("provider redirect")
        .to_string();
    assert!(callback.contains("/auth/oidc/callback"), "{callback}");
    client().get(&callback).send().await.expect("callback")
}

/// The hidden form fields of a hand-off page.
pub(super) fn form_fields(html: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for part in html.split("<input type=\"hidden\" ").skip(1) {
        let name = part
            .split("name=\"")
            .nth(1)
            .and_then(|r| r.split('"').next());
        let value = part
            .split("value=\"")
            .nth(1)
            .and_then(|r| r.split('"').next());
        if let (Some(n), Some(v)) = (name, value) {
            out.insert(n.to_string(), v.to_string());
        }
    }
    out
}

/// Follow the redirects from the provider URL until the desktop return
/// address (the server callback is never called by hand).
pub(super) async fn follow(provider_url: &str) -> Chain {
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

pub(super) fn issue(ctx: &Ctx, next: NextToken) {
    *ctx.next.lock().expect("lock") = Some(next);
}

pub(super) fn fresh(sub: &str) -> NextToken {
    NextToken {
        sub: sub.to_string(),
        auth_time: Some(now() - 5),
        ..NextToken::default()
    }
}

#[sqlx::test]
async fn test_login_flow_does_not_force_a_fresh_login(pool: PgPool) {
    let ctx = setup(&pool).await;
    let resp = client()
        .get(format!(
            "{}/auth/oidc/authorize/{}",
            ctx.server.url, ctx.slug
        ))
        .query(&[
            ("loopback_port", "50123"),
            (
                "callback_path",
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            ),
            (
                "client_challenge",
                "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            ),
        ])
        .header("x-kaiku-client", "desktop")
        .send()
        .await
        .expect("authorize");
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.expect("json");
    let params = query_of(body["url"].as_str().expect("url"));
    assert!(!params.contains_key("prompt"), "login: no prompt=login");
    assert!(!params.contains_key("max_age"), "login: no max_age");
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
