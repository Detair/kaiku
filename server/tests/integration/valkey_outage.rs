//! Behaviour while Valkey stops answering (and, at the end, while the
//! database is unavailable during WebSocket auth).
//!
//! The server's Valkey client talks to the dev Valkey through a local TCP
//! proxy. `freeze` makes the proxy stop forwarding while keeping every socket
//! open, which is what a hung or unreachable Valkey looks like to the client:
//! commands are written but never answered.
//!
//! Every request below is wrapped in [`HANG_GUARD`]; without a command timeout
//! the server would wait for Valkey forever and the guard would fire.
//!
//! Run with: `cargo nextest run -p vc-server --test integration valkey_outage`

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Method, StatusCode};
use futures::StreamExt;
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::config::Config;
use vc_server::db;
use vc_server::ratelimit::{RateLimitConfig, RateLimiter, RateLimits};
use vc_server::voice::sfu::SfuServer;

use super::helpers::{
    body_to_json, create_test_user, generate_access_token, spawn_test_server, ws_connect,
    TestServer,
};

/// Upper bound for any request while Valkey hangs. Well above the 3 s default
/// command timeout, well below "the server hangs".
const HANG_GUARD: Duration = Duration::from_secs(10);

/// TCP proxy in front of the dev Valkey that can be frozen.
struct ValkeyProxy {
    url: String,
    frozen: Arc<AtomicBool>,
}

impl ValkeyProxy {
    async fn spawn() -> Self {
        let upstream_url = Config::default_for_test().redis_url;
        let mut url = reqwest::Url::parse(&upstream_url).expect("REDIS_URL is a URL");
        let upstream = format!(
            "{}:{}",
            url.host_str().unwrap_or("localhost"),
            url.port().unwrap_or(6379)
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
        let port = listener.local_addr().expect("proxy addr").port();
        url.set_host(Some("127.0.0.1")).expect("set proxy host");
        url.set_port(Some(port)).expect("set proxy port");

        let frozen = Arc::new(AtomicBool::new(false));
        let accept_frozen = frozen.clone();
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let frozen = accept_frozen.clone();
                let upstream = upstream.clone();
                tokio::spawn(async move {
                    if frozen.load(Ordering::SeqCst) {
                        // Accept, then never answer.
                        let _held = client;
                        std::future::pending::<()>().await;
                        return;
                    }
                    let Ok(server) = TcpStream::connect(&upstream).await else {
                        return;
                    };
                    let (client_rd, client_wr) = client.into_split();
                    let (server_rd, server_wr) = server.into_split();
                    tokio::join!(
                        pump(client_rd, server_wr, frozen.clone()),
                        pump(server_rd, client_wr, frozen),
                    );
                });
            }
        });

        Self {
            url: url.to_string(),
            frozen,
        }
    }

    /// Stop forwarding in both directions; sockets stay open.
    fn freeze(&self) {
        self.frozen.store(true, Ordering::SeqCst);
    }
}

async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    frozen: Arc<AtomicBool>,
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if frozen.load(Ordering::SeqCst) {
            // Swallow this and everything after it, keep both sockets open.
            let _keep = (from, to);
            std::future::pending::<()>().await;
            return;
        }
        if to.write_all(&buf[..n]).await.is_err() {
            return;
        }
    }
}

/// App state whose Valkey client goes through `proxy`.
async fn state_via_proxy(
    pool: PgPool,
    proxy: &ValkeyProxy,
    fail_open: Option<bool>,
) -> (AppState, Config) {
    let config = Config::default_for_test();
    let redis = db::create_redis_client(&proxy.url)
        .await
        .expect("connect to Valkey through the proxy");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("SfuServer");

    let rate_limiter = match fail_open {
        Some(fail_open) => {
            let mut limiter = RateLimiter::new(
                redis.clone(),
                RateLimitConfig {
                    enabled: true,
                    redis_key_prefix: format!("test:rl:{}", uuid::Uuid::new_v4()),
                    fail_open,
                    trusted_proxies: vc_server::ratelimit::TrustedProxies::default(),
                    allowlist: HashSet::new(),
                    limits: RateLimits::default(),
                },
            );
            limiter.init().await.expect("load rate limit scripts");
            Some(limiter)
        }
        None => None,
    };

    let state = AppState::new(AppStateConfig {
        db: pool,
        redis,
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter,
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    });
    (state, config)
}

async fn spawn_via_proxy(
    pool: PgPool,
    proxy: &ValkeyProxy,
    fail_open: Option<bool>,
) -> (TestServer, Config) {
    let (state, config) = state_via_proxy(pool, proxy, fail_open).await;
    (spawn_test_server(create_router(state)).await, config)
}

/// The session denylist check fails closed: an authenticated request gets a
/// quick 503 instead of hanging or being let through unchecked.
#[sqlx::test]
async fn authenticated_request_fails_fast_with_503_when_valkey_hangs(pool: PgPool) {
    let proxy = ValkeyProxy::spawn().await;
    let (state, config) = state_via_proxy(pool.clone(), &proxy, None).await;
    let router = create_router(state);
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);

    proxy.freeze();

    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/auth/me")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let started = Instant::now();
    let response = tokio::time::timeout(HANG_GUARD, tower::ServiceExt::oneshot(router, request))
        .await
        .expect("authenticated request hung while Valkey was unavailable")
        .unwrap();

    assert!(started.elapsed() < HANG_GUARD);
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_to_json(response).await;
    assert_eq!(body["error"], "SERVICE_UNAVAILABLE", "{body}");
}

/// General rate limits fail open with the default config
/// (`RATE_LIMIT_FAIL_OPEN` unset): the route keeps working.
#[sqlx::test]
async fn general_rate_limited_route_fails_open_when_valkey_hangs(pool: PgPool) {
    let proxy = ValkeyProxy::spawn().await;
    let (server, _config) = spawn_via_proxy(pool, &proxy, Some(false)).await;

    proxy.freeze();

    let started = Instant::now();
    let response = tokio::time::timeout(
        HANG_GUARD,
        reqwest::get(format!("{}/api/discover/guilds", server.url)),
    )
    .await
    .expect("rate-limited request hung while Valkey was unavailable")
    .expect("request failed");

    assert!(started.elapsed() < HANG_GUARD);
    assert_eq!(response.status(), 200);
}

/// Authentication rate limits fail closed: login answers 503 quickly.
#[sqlx::test]
async fn login_fails_closed_with_503_when_valkey_hangs(pool: PgPool) {
    let proxy = ValkeyProxy::spawn().await;
    let (server, _config) = spawn_via_proxy(pool.clone(), &proxy, Some(false)).await;
    let (_, username) = create_test_user(&pool).await;

    proxy.freeze();

    let started = Instant::now();
    let response = tokio::time::timeout(
        HANG_GUARD,
        reqwest::Client::new()
            .post(format!("{}/auth/login", server.url))
            .json(&serde_json::json!({ "username": username, "password": "wrong-password" }))
            .send(),
    )
    .await
    .expect("login hung while Valkey was unavailable")
    .expect("request failed");

    assert!(started.elapsed() < HANG_GUARD);
    assert_eq!(response.status(), 503);
    let body: serde_json::Value = response.json().await.expect("JSON body");
    assert_eq!(body["error"], "SERVICE_UNAVAILABLE", "{body}");
}

/// Registration (an authentication rate limit without the IP-block layer)
/// also fails closed with 503.
#[sqlx::test]
async fn register_fails_closed_with_503_when_valkey_hangs(pool: PgPool) {
    let proxy = ValkeyProxy::spawn().await;
    let (server, _config) = spawn_via_proxy(pool, &proxy, Some(false)).await;

    proxy.freeze();

    let response = tokio::time::timeout(
        HANG_GUARD,
        reqwest::Client::new()
            .post(format!("{}/auth/register", server.url))
            .json(&serde_json::json!({
                "username": "outage_user",
                "password": "correct-horse-battery-staple"
            }))
            .send(),
    )
    .await
    .expect("register hung while Valkey was unavailable")
    .expect("request failed");

    assert_eq!(response.status(), 503);
    let body: serde_json::Value = response.json().await.expect("JSON body");
    assert_eq!(body["error"], "SERVICE_UNAVAILABLE", "{body}");
}

/// Header-based WebSocket auth cannot check the session denylist: the upgrade
/// is refused with the shared JSON 503 body.
#[sqlx::test]
async fn websocket_header_auth_answers_503_when_valkey_hangs(pool: PgPool) {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Error as WsError;

    let proxy = ValkeyProxy::spawn().await;
    let (server, config) = spawn_via_proxy(pool.clone(), &proxy, None).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);

    proxy.freeze();

    let mut request = format!("ws://{}/ws", server.addr)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("access_token.{token}").parse().unwrap(),
    );
    let result = tokio::time::timeout(HANG_GUARD, tokio_tungstenite::connect_async(request))
        .await
        .expect("WebSocket header auth hung while Valkey was unavailable");

    let Err(WsError::Http(response)) = result else {
        panic!("expected an HTTP error response, got {result:?}");
    };
    assert_eq!(response.status(), 503);
    let body: serde_json::Value =
        serde_json::from_slice(response.body().as_deref().unwrap_or_default()).expect("JSON body");
    assert_eq!(body["error"], "SERVICE_UNAVAILABLE", "{body}");
}

/// WebSocket post-connect auth cannot check the session denylist: the socket
/// is closed with 1013 (try again later) instead of hanging or being admitted.
#[sqlx::test]
async fn websocket_auth_closes_with_try_again_later_when_valkey_hangs(pool: PgPool) {
    let proxy = ValkeyProxy::spawn().await;
    let (server, config) = spawn_via_proxy(pool.clone(), &proxy, None).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);

    proxy.freeze();

    let mut ws = ws_connect(&server, &token).await;
    let close = tokio::time::timeout(HANG_GUARD, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    })
    .await
    .expect("WebSocket auth hung while Valkey was unavailable");

    let code: u16 = close.expect("close frame carries a code").code.into();
    assert_eq!(code, 1013);
}

// ============================================================================
// Database unavailable during WebSocket auth
// ============================================================================

/// A server whose database pool is closed (every query fails at once) while
/// Valkey works; `pool` stays open for test setup.
async fn spawn_with_closed_db(pool: &PgPool) -> (TestServer, Config) {
    let closed = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("second pool");
    closed.close().await;
    spawn_with_db(closed).await
}

/// A server on `db` with a working Valkey.
async fn spawn_with_db(db: PgPool) -> (TestServer, Config) {
    let config = Config::default_for_test();
    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("Valkey");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("SfuServer");
    let state = AppState::new(AppStateConfig {
        db,
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
    (spawn_test_server(create_router(state)).await, config)
}

/// A database error while validating the post-connect `Authenticate` frame
/// closes with 1013 (try again later), not 1008 (sign in again).
#[sqlx::test]
async fn websocket_auth_closes_with_try_again_later_when_db_fails(pool: PgPool) {
    let (server, config) = spawn_with_closed_db(&pool).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);

    let mut ws = ws_connect(&server, &token).await;
    let close = tokio::time::timeout(HANG_GUARD, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {}
                other => panic!("socket ended without a close frame: {other:?}"),
            }
        }
    })
    .await
    .expect("WebSocket auth hung while the database was unavailable");

    let code: u16 = close.expect("close frame carries a code").code.into();
    assert_eq!(code, 1013);
}

/// A database error during header-based WebSocket auth answers the shared
/// JSON 503 `SERVICE_UNAVAILABLE` body.
#[sqlx::test]
async fn websocket_header_auth_answers_503_when_db_fails(pool: PgPool) {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Error as WsError;

    let (server, config) = spawn_with_closed_db(&pool).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);

    let mut request = format!("ws://{}/ws", server.addr)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("access_token.{token}").parse().unwrap(),
    );
    let result = tokio::time::timeout(HANG_GUARD, tokio_tungstenite::connect_async(request))
        .await
        .expect("WebSocket header auth hung while the database was unavailable");

    let Err(WsError::Http(response)) = result else {
        panic!("expected an HTTP error response, got {result:?}");
    };
    assert_eq!(response.status(), 503);
    let body: serde_json::Value =
        serde_json::from_slice(response.body().as_deref().unwrap_or_default()).expect("JSON body");
    assert_eq!(body["error"], "SERVICE_UNAVAILABLE", "{body}");
}

/// A database that accepts no further connection (every one is held, so the
/// account lookup waits) must not hold the header-auth upgrade open: it
/// answers the shared JSON 503 body after the lookup budget.
#[sqlx::test]
async fn websocket_header_auth_answers_503_when_db_stalls(pool: PgPool) {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Error as WsError;

    let stalled = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(60))
        .connect_with((*pool.connect_options()).clone())
        .await
        .expect("single-connection pool");
    let (server, config) = spawn_with_db(stalled.clone()).await;
    let (user_id, _) = create_test_user(&pool).await;
    let token = generate_access_token(&config, user_id);
    // Hold the only connection for the rest of the test.
    let _held = stalled.acquire().await.expect("hold the connection");

    let mut request = format!("ws://{}/ws", server.addr)
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        format!("access_token.{token}").parse().unwrap(),
    );
    let result = tokio::time::timeout(HANG_GUARD, tokio_tungstenite::connect_async(request))
        .await
        .expect("WebSocket header auth waited on a stalled database");

    let Err(WsError::Http(response)) = result else {
        panic!("expected an HTTP error response, got {result:?}");
    };
    assert_eq!(response.status(), 503);
    let body: serde_json::Value =
        serde_json::from_slice(response.body().as_deref().unwrap_or_default()).expect("JSON body");
    assert_eq!(body["error"], "SERVICE_UNAVAILABLE", "{body}");
}
