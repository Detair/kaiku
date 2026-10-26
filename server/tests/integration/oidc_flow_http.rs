//! OIDC authorize/callback HTTP tests that need no live identity provider.
//!
//! Covers the redirect wiring of `/auth/oidc/authorize` (what the provider and
//! the client each receive) and how the callback hands failures back to the
//! sign-in origin (desktop loopback, browser popup, identity-link flow).
//!
//! Run tests: `cargo test --test integration oidc_flow_http`

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fred::interfaces::KeysInterface;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::auth::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use vc_server::auth::oidc::{
    handoff_key, OidcFlowPurpose, OidcFlowState, OidcHandoffGrant, OidcProviderManager,
};
use vc_server::config::Config;
use vc_server::db;
use vc_server::voice::sfu::SfuServer;

use super::helpers::create_test_user;

const PUBLIC_URL: &str = "https://chat.example.com";
const LOOPBACK: &str = "http://127.0.0.1:54321/callback";

fn provider_key() -> Vec<u8> {
    vec![7u8; 32]
}

struct OidcApp {
    router: axum::Router,
    state: AppState,
    config: Config,
}

/// App with an OIDC manager (providers loaded from `pool`) and `PUBLIC_URL`.
async fn oidc_app(pool: &PgPool, public_url: &str) -> OidcApp {
    let mut config = Config::default_for_test();
    config.public_url = Some(public_url.to_string());
    // The default test MFA key is not 32 bytes; the OIDC flow state needs one.
    config.mfa_encryption_key = Some("11".repeat(32));
    config.cors_allowed_origins = vec![public_url.trim_end_matches('/').to_string()];
    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("Redis");
    let sfu = SfuServer::new(std::sync::Arc::new(config.clone()), None).expect("SfuServer");
    let manager = OidcProviderManager::new(provider_key());
    manager.load_providers(pool).await.expect("load providers");
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
    OidcApp {
        // The callback reads the peer address (re-authentication audit IP).
        router: create_router(state.clone()).layer(axum::extract::connect_info::MockConnectInfo(
            std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        )),
        state,
        config,
    }
}

fn enc_key(config: &Config) -> Vec<u8> {
    hex::decode(config.mfa_encryption_key.as_ref().expect("test MFA key")).unwrap()
}

fn redis_key(csrf_state: &str) -> String {
    format!(
        "oidc:state:{}",
        hex::encode(Sha256::digest(csrf_state.as_bytes()))
    )
}

/// Store a flow state the way `start_oidc_flow` does; returns the CSRF state.
async fn store_flow(app: &OidcApp, flow: &OidcFlowState) -> String {
    let csrf_state = format!("st-{}", Uuid::new_v4());
    let enc =
        encrypt_mfa_secret(&serde_json::to_string(flow).unwrap(), &enc_key(&app.config)).unwrap();
    app.state
        .redis
        .set::<(), _, _>(redis_key(&csrf_state), enc.as_str(), None, None, false)
        .await
        .unwrap();
    csrf_state
}

fn flow(client: Option<&str>, link_user_id: Option<Uuid>) -> OidcFlowState {
    OidcFlowState {
        slug: "not-loaded".to_string(),
        pkce_verifier: "v".to_string(),
        nonce: "n".to_string(),
        redirect_uri: format!("{PUBLIC_URL}/auth/oidc/callback"),
        created_at: 0,
        purpose: link_user_id.map_or(OidcFlowPurpose::Login, |user_id| OidcFlowPurpose::Link {
            user_id,
            session_id: None,
        }),
        client_redirect_uri: client.map(str::to_string),
        client_challenge: None,
    }
}

async fn get(app: &OidcApp, uri: &str) -> axum::response::Response {
    get_with(app, uri, &[]).await
}

async fn get_with(app: &OidcApp, uri: &str, headers: &[(&str, &str)]) -> axum::response::Response {
    let mut req = Request::get(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    app.router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

/// Header the desktop app sends on every SSO start.
const DESKTOP: (&str, &str) = ("x-kaiku-client", "desktop");

fn location(resp: &axum::response::Response) -> String {
    resp.headers()
        .get("location")
        .expect("Location header")
        .to_str()
        .unwrap()
        .to_string()
}

async fn body_text(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Provider-denied callback: `?error=…&state=…`, no `code`.
fn denied_uri(csrf_state: &str) -> String {
    format!(
        "/auth/oidc/callback?error=access_denied&error_description=Provider%20says%20no&state={csrf_state}"
    )
}

// ============================================================================
// Authorize wiring
// ============================================================================

/// RFC 7636 appendix B verifier and its S256 challenge.
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
/// Desktop return address with a per-flow value.
const LOOPBACK_WITH_FLOW: &str = "http://127.0.0.1:54321/callback?flow=perflowvalue_0123456789";

/// Create an enabled OIDC provider with manual endpoints; returns its slug.
async fn create_manual_provider(pool: &PgPool) -> String {
    let user_id = create_test_user(pool).await.0;
    let mut methods = db::get_auth_methods_allowed(pool).await.unwrap();
    methods.oidc = true;
    db::set_auth_methods_allowed(pool, &methods, Some(user_id))
        .await
        .unwrap();
    let slug = format!("wire-{}", Uuid::new_v4().simple());
    let secret = OidcProviderManager::new(provider_key())
        .encrypt_secret("secret")
        .unwrap();
    db::create_oidc_provider(
        pool,
        db::CreateOidcProviderParams {
            slug: &slug,
            display_name: "Wire",
            icon_hint: None,
            provider_type: "custom",
            issuer_url: None,
            authorization_url: Some("https://idp.example.com/auth"),
            token_url: Some("https://idp.example.com/token"),
            userinfo_url: None,
            client_id: "client-id",
            client_secret_encrypted: &secret,
            scopes: "openid",
            created_by: Some(user_id),
        },
    )
    .await
    .unwrap();
    slug
}

fn authorize_uri(slug: &str, query: &[(&str, &str)]) -> String {
    let mut url = openidconnect::url::Url::parse("http://x/").unwrap();
    url.set_path(&format!("/auth/oidc/authorize/{slug}"));
    for (k, v) in query {
        url.query_pairs_mut().append_pair(k, v);
    }
    format!("{}?{}", url.path(), url.query().unwrap_or(""))
}

async fn error_code(resp: axum::response::Response) -> String {
    let body: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    body["error"].as_str().unwrap_or_default().to_string()
}

/// The provider gets the server callback; the loopback (with its per-flow
/// value) and the client challenge are stored only in the flow state.
#[sqlx::test]
async fn test_authorize_desktop_wires_provider_and_client_redirects(pool: PgPool) {
    let slug = create_manual_provider(&pool).await;

    // Trailing slash on PUBLIC_URL must not produce "//auth/oidc/callback".
    let app = oidc_app(&pool, "https://chat.example.com/").await;
    let resp = get_with(
        &app,
        &authorize_uri(
            &slug,
            &[
                ("redirect_uri", LOOPBACK_WITH_FLOW),
                ("client_challenge", CHALLENGE),
            ],
        ),
        &[DESKTOP],
    )
    .await;
    // A desktop start always answers with JSON, never a redirect.
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("location").is_none());
    let body: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    let provider_url = body["url"].as_str().expect("url").to_string();

    let auth_url = openidconnect::url::Url::parse(&provider_url).unwrap();
    let q: std::collections::HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
    assert_eq!(
        q.get("redirect_uri").map(String::as_str),
        Some("https://chat.example.com/auth/oidc/callback")
    );
    // Nothing of the desktop's values reaches the provider.
    assert!(!provider_url.contains(CHALLENGE));
    assert!(!provider_url.contains("perflowvalue"));

    let stored: Option<String> = app
        .state
        .redis
        .getdel(redis_key(&q["state"]))
        .await
        .unwrap();
    let flow: OidcFlowState = serde_json::from_str(
        &decrypt_mfa_secret(&stored.expect("flow state stored"), &enc_key(&app.config)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        flow.redirect_uri,
        "https://chat.example.com/auth/oidc/callback"
    );
    assert_eq!(
        flow.client_redirect_uri.as_deref(),
        Some(LOOPBACK_WITH_FLOW)
    );
    assert_eq!(flow.client_challenge.as_deref(), Some(CHALLENGE));
    assert_eq!(flow.purpose, OidcFlowPurpose::Login);
}

/// A desktop sign-in without a client challenge (an older app) is refused
/// with a code the app can show, instead of handing tokens to the browser.
#[sqlx::test]
async fn test_authorize_desktop_without_challenge_is_outdated(pool: PgPool) {
    let slug = create_manual_provider(&pool).await;
    let app = oidc_app(&pool, PUBLIC_URL).await;
    // An app that sends the desktop header but no challenge is outdated too.
    for headers in [&[][..], &[DESKTOP][..]] {
        let resp = get_with(
            &app,
            &authorize_uri(&slug, &[("redirect_uri", LOOPBACK)]),
            headers,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(resp.headers().get("location").is_none());
        assert_eq!(error_code(resp).await, "OIDC_CLIENT_OUTDATED");
    }
}

/// A desktop start (loopback `redirect_uri` or `client_challenge`) without the
/// desktop header — e.g. a link someone clicked in a browser — never
/// redirects to the provider.
#[sqlx::test]
async fn test_desktop_start_without_header_never_redirects(pool: PgPool) {
    let slug = create_manual_provider(&pool).await;
    let app = oidc_app(&pool, PUBLIC_URL).await;
    for query in [
        &[
            ("redirect_uri", LOOPBACK_WITH_FLOW),
            ("client_challenge", CHALLENGE),
        ][..],
        &[("client_challenge", CHALLENGE)][..],
        &[
            ("redirect_uri", LOOPBACK_WITH_FLOW),
            ("client_challenge", CHALLENGE),
            ("response", "json"),
        ][..],
    ] {
        let resp = get(&app, &authorize_uri(&slug, query)).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{query:?}");
        assert!(resp.headers().get("location").is_none(), "{query:?}");
        assert_eq!(error_code(resp).await, "OIDC_CLIENT_OUTDATED");
    }
    // Any other header value is not the desktop app either.
    let resp = get_with(
        &app,
        &authorize_uri(
            &slug,
            &[
                ("redirect_uri", LOOPBACK_WITH_FLOW),
                ("client_challenge", CHALLENGE),
            ],
        ),
        &[("x-kaiku-client", "web")],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(resp.headers().get("location").is_none());
}

/// Browsers cannot send the desktop header cross-site: the CORS preflight
/// does not allow it, for foreign origins and also for the Tauri webview
/// origins (the desktop sends it from Rust, which has no Origin and no
/// preflight).
#[sqlx::test]
async fn test_cors_preflight_does_not_allow_the_desktop_header(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    for origin in ["https://evil.example", "tauri://localhost", PUBLIC_URL] {
        let resp = app
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/auth/oidc/authorize/any")
                    .header("origin", origin)
                    .header("access-control-request-method", "GET")
                    .header("access-control-request-headers", "x-kaiku-client")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let allowed = resp
            .headers()
            .get("access-control-allow-headers")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        assert!(!allowed.contains("x-kaiku-client"), "{origin}: {allowed}");
        if origin == "https://evil.example" {
            assert!(
                resp.headers().get("access-control-allow-origin").is_none(),
                "foreign origin is not allowed"
            );
        }
    }
}

#[sqlx::test]
async fn test_authorize_rejects_malformed_or_misplaced_challenge(pool: PgPool) {
    let slug = create_manual_provider(&pool).await;
    let app = oidc_app(&pool, PUBLIC_URL).await;
    // Not an S256 digest.
    let resp = get_with(
        &app,
        &authorize_uri(
            &slug,
            &[("redirect_uri", LOOPBACK), ("client_challenge", "plain")],
        ),
        &[DESKTOP],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "VALIDATION_ERROR");
    // No loopback: there is no desktop listener to hand a code to.
    let resp = get_with(
        &app,
        &authorize_uri(&slug, &[("client_challenge", CHALLENGE)]),
        &[DESKTOP],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "VALIDATION_ERROR");
}

/// The browser flow is unchanged: no challenge needed, nothing stored for one.
#[sqlx::test]
async fn test_authorize_browser_flow_unchanged(pool: PgPool) {
    let slug = create_manual_provider(&pool).await;
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let resp = get(&app, &authorize_uri(&slug, &[])).await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    let auth_url = openidconnect::url::Url::parse(&location(&resp)).unwrap();
    let q: std::collections::HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
    let stored: Option<String> = app
        .state
        .redis
        .getdel(redis_key(&q["state"]))
        .await
        .unwrap();
    let flow: OidcFlowState = serde_json::from_str(
        &decrypt_mfa_secret(&stored.expect("flow state stored"), &enc_key(&app.config)).unwrap(),
    )
    .unwrap();
    assert_eq!(flow.client_redirect_uri, None);
    assert_eq!(flow.client_challenge, None);
}

// ============================================================================
// Hand-off code exchange
// ============================================================================

async fn post_exchange(app: &OidcApp, body: &serde_json::Value) -> axum::response::Response {
    app.router
        .clone()
        .oneshot(
            Request::post("/auth/oidc/desktop/exchange")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}

/// Store a hand-off grant the way the callback does; returns the code.
async fn store_grant(app: &OidcApp, user_id: Uuid, created_at: i64) -> String {
    let code = format!("code-{}", Uuid::new_v4().simple());
    let grant = OidcHandoffGrant {
        user_id,
        slug: "wire".to_string(),
        client_challenge: CHALLENGE.to_string(),
        created_at,
    };
    app.state
        .redis
        .set::<(), _, _>(
            handoff_key(&code),
            serde_json::to_string(&grant).unwrap().as_str(),
            None,
            None,
            false,
        )
        .await
        .unwrap();
    code
}

#[sqlx::test]
async fn test_exchange_redeems_a_fresh_grant_once(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let user_id = create_test_user(&pool).await.0;
    let code = store_grant(&app, user_id, chrono::Utc::now().timestamp()).await;
    let body = serde_json::json!({ "code": code, "verifier": VERIFIER });

    let resp = post_exchange(&app, &body).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("set-cookie").is_none(), "no cookie");
    let tokens: serde_json::Value = serde_json::from_str(&body_text(resp).await).unwrap();
    assert!(tokens["access_token"]
        .as_str()
        .is_some_and(|t| !t.is_empty()));
    assert!(tokens["refresh_token"]
        .as_str()
        .is_some_and(|t| !t.is_empty()));
    assert_eq!(tokens["token_type"], "Bearer");
    assert!(tokens["expires_in"].as_i64().is_some());
    assert!(tokens["setup_required"].is_boolean());
    assert_eq!(tokens["password_change_required"], false);

    let left: Option<String> = app.state.redis.get(handoff_key(&code)).await.unwrap();
    assert!(left.is_none(), "grant is consumed");
    let resp = post_exchange(&app, &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "OIDC_HANDOFF_INVALID");
}

#[sqlx::test]
async fn test_exchange_refuses_an_expired_grant(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let user_id = create_test_user(&pool).await.0;
    let code = store_grant(&app, user_id, chrono::Utc::now().timestamp() - 61).await;
    let resp = post_exchange(
        &app,
        &serde_json::json!({ "code": code, "verifier": VERIFIER }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "OIDC_HANDOFF_INVALID");
}

/// A wrong verifier fails and burns the code: the right one cannot follow.
#[sqlx::test]
async fn test_exchange_wrong_verifier_burns_the_code(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let user_id = create_test_user(&pool).await.0;
    let code = store_grant(&app, user_id, chrono::Utc::now().timestamp()).await;
    let wrong = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXK";
    let resp = post_exchange(
        &app,
        &serde_json::json!({ "code": code, "verifier": wrong }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "OIDC_HANDOFF_INVALID");
    let resp = post_exchange(
        &app,
        &serde_json::json!({ "code": code, "verifier": VERIFIER }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(resp).await, "OIDC_HANDOFF_INVALID");
}

#[sqlx::test]
async fn test_exchange_refuses_unknown_or_malformed_codes(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    for body in [
        serde_json::json!({ "code": "never-issued", "verifier": VERIFIER }),
        serde_json::json!({ "code": "", "verifier": VERIFIER }),
        serde_json::json!({ "code": "x".repeat(4096), "verifier": VERIFIER }),
    ] {
        let resp = post_exchange(&app, &body).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(error_code(resp).await, "OIDC_HANDOFF_INVALID");
    }
}

/// A banned account cannot redeem a grant issued just before the ban.
#[sqlx::test]
async fn test_exchange_refuses_a_banned_account(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let user_id = create_test_user(&pool).await.0;
    let code = store_grant(&app, user_id, chrono::Utc::now().timestamp()).await;
    sqlx::query("INSERT INTO global_bans (user_id, banned_by, reason) VALUES ($1, $1, 'test')")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
    let resp = post_exchange(
        &app,
        &serde_json::json!({ "code": code, "verifier": VERIFIER }),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, "ACCOUNT_BANNED");
}

// ============================================================================
// Callback failures return to the sign-in origin
// ============================================================================

#[sqlx::test]
async fn test_callback_provider_error_returns_to_desktop(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let st = store_flow(&app, &flow(Some(LOOPBACK), None)).await;

    let resp = get(&app, &denied_uri(&st)).await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        location(&resp),
        format!("{LOOPBACK}?error=OIDC_PROVIDER_ERROR")
    );

    // State is consumed (one-time use).
    let left: Option<String> = app.state.redis.get(redis_key(&st)).await.unwrap();
    assert!(left.is_none());
}

#[sqlx::test]
async fn test_callback_provider_error_returns_to_browser(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let st = store_flow(&app, &flow(None, None)).await;

    let resp = get(&app, &denied_uri(&st)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(html.contains("\"type\":\"oidc-callback\""), "{html}");
    assert!(html.contains("\"error\":\"OIDC_PROVIDER_ERROR\""), "{html}");
    assert!(
        !html.contains("Provider says no"),
        "provider text must not be echoed"
    );
}

#[sqlx::test]
async fn test_callback_provider_error_returns_to_desktop_link(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let st = store_flow(&app, &flow(Some(LOOPBACK), Some(Uuid::new_v4()))).await;

    let resp = get(&app, &denied_uri(&st)).await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        location(&resp),
        format!("{LOOPBACK}?link_error=OIDC_PROVIDER_ERROR")
    );
}

#[sqlx::test]
async fn test_callback_provider_error_returns_to_browser_link(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let st = store_flow(&app, &flow(None, Some(Uuid::new_v4()))).await;

    let resp = get(&app, &denied_uri(&st)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let html = body_text(resp).await;
    assert!(html.contains("\"type\":\"oidc-link-callback\""), "{html}");
    assert!(html.contains("\"success\":false"), "{html}");
    assert!(
        html.contains("\"error_code\":\"OIDC_PROVIDER_ERROR\""),
        "{html}"
    );
}

/// A failed code exchange (here: provider no longer loaded) also returns to
/// the origin instead of leaving the desktop waiting.
#[sqlx::test]
async fn test_callback_exchange_failure_returns_to_desktop(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let st = store_flow(&app, &flow(Some(LOOPBACK), None)).await;

    let resp = get(&app, &format!("/auth/oidc/callback?code=abc&state={st}")).await;
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        location(&resp),
        format!("{LOOPBACK}?error=OIDC_EXCHANGE_FAILED")
    );
}

/// Unknown state: nothing validated to return to, so no redirect.
#[sqlx::test]
async fn test_callback_error_with_unknown_state_does_not_redirect(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let resp = get(&app, &denied_uri("never-issued")).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(resp.headers().get("location").is_none());
}

// ============================================================================
// Request secrets stay out of tracing spans
// ============================================================================

/// Collects formatted tracing output of the current thread.
#[derive(Clone, Default)]
struct CapturedLogs(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl CapturedLogs {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Run `f` with a thread-local subscriber that formats every span field.
async fn capture<F: std::future::Future<Output = ()>>(f: F) -> String {
    let logs = CapturedLogs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let guard = tracing::subscriber::set_default(subscriber);
    f.await;
    drop(guard);
    logs.text()
}

const SECRET_COOKIE: &str = "refresh_token=cookie-secret-do-not-log";
const SECRET_BEARER: &str = "Bearer bearer-secret-do-not-log";

fn assert_no_secrets(logs: &str, span: &str) {
    assert!(logs.contains(span), "captured nothing for {span}: {logs}");
    assert!(!logs.contains("cookie-secret-do-not-log"), "{logs}");
    assert!(!logs.contains("bearer-secret-do-not-log"), "{logs}");
}

#[sqlx::test]
async fn test_authorize_span_has_no_request_secrets(pool: PgPool) {
    let slug = create_manual_provider(&pool).await;
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let logs = capture(async {
        let resp = get_with(
            &app,
            &authorize_uri(&slug, &[]),
            &[("cookie", SECRET_COOKIE), ("authorization", SECRET_BEARER)],
        )
        .await;
        assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    })
    .await;
    assert_no_secrets(&logs, "oidc_authorize");
}

#[sqlx::test]
async fn test_exchange_span_has_no_request_secrets(pool: PgPool) {
    let app = oidc_app(&pool, PUBLIC_URL).await;
    let user_id = create_test_user(&pool).await.0;
    let code = store_grant(&app, user_id, chrono::Utc::now().timestamp()).await;
    let logs = capture(async {
        let resp = app
            .router
            .clone()
            .oneshot(
                Request::post("/auth/oidc/desktop/exchange")
                    .header("content-type", "application/json")
                    .header("cookie", SECRET_COOKIE)
                    .header("authorization", SECRET_BEARER)
                    .body(Body::from(
                        serde_json::json!({ "code": code, "verifier": "A".repeat(43) }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    })
    .await;
    assert_no_secrets(&logs, "oidc_desktop_exchange");
}
