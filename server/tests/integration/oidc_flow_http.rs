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
use vc_server::auth::oidc::{OidcFlowState, OidcProviderManager};
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
        link_user_id,
        client_redirect_uri: client.map(str::to_string),
        reauth: None,
    }
}

async fn get(app: &OidcApp, uri: &str) -> axum::response::Response {
    app.router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

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

/// The provider gets the server callback; the loopback is stored only as the
/// client return address.
#[sqlx::test]
async fn test_authorize_desktop_wires_provider_and_client_redirects(pool: PgPool) {
    let user_id = create_test_user(&pool).await.0;
    let mut methods = db::get_auth_methods_allowed(&pool).await.unwrap();
    methods.oidc = true;
    db::set_auth_methods_allowed(&pool, &methods, Some(user_id))
        .await
        .unwrap();
    let slug = format!("wire-{}", Uuid::new_v4().simple());
    let secret = OidcProviderManager::new(provider_key())
        .encrypt_secret("secret")
        .unwrap();
    db::create_oidc_provider(
        &pool,
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

    // Trailing slash on PUBLIC_URL must not produce "//auth/oidc/callback".
    let app = oidc_app(&pool, "https://chat.example.com/").await;
    let resp = get(
        &app,
        &format!(
            "/auth/oidc/authorize/{slug}?redirect_uri=http%3A%2F%2F127.0.0.1%3A54321%2Fcallback"
        ),
    )
    .await;
    assert!(resp.status().is_redirection(), "status {}", resp.status());

    let auth_url = openidconnect::url::Url::parse(&location(&resp)).unwrap();
    let q: std::collections::HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
    assert_eq!(
        q.get("redirect_uri").map(String::as_str),
        Some("https://chat.example.com/auth/oidc/callback")
    );

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
    assert_eq!(flow.client_redirect_uri.as_deref(), Some(LOOPBACK));
    assert_eq!(flow.link_user_id, None);
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
