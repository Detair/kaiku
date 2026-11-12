//! Reusable test helpers for HTTP integration tests.
//!
//! Provides `TestApp` for building and sending requests through the full axum router,
//! plus utilities for user creation, admin grants, and JWT generation.
//!
//! ## Integration test pattern
//!
//! Integration tests use `#[sqlx::test]`; each test receives a fresh `PgPool`
//! against an isolated database. Call [`TestApp::with_pool`] (or
//! [`TestApp::with_pool_and_screen_share_limiter`] /
//! [`TestApp::with_pool_and_config`]) to wire the axum router onto that pool.
//!
//! ## Test Servers
//!
//! Use [`spawn_test_server()`] when you need stateful middleware testing
//! (rate limiting, request IDs, etc.) instead of `tower::ServiceExt::oneshot`.
#![allow(dead_code)]

pub mod approvals;
pub mod attestation;
pub mod login;
pub mod migrations;
pub mod passkeys;
pub mod tokens;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{self, Method, Request, Response};
use axum::Router;
use http_body_util::BodyExt;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tower::ServiceExt;
use uuid::Uuid;
use vc_server::api::{create_router, AppState, AppStateConfig};
use vc_server::chat::S3Client;
use vc_server::config::Config;
use vc_server::db;
use vc_server::permissions::GuildPermissions;
use vc_server::voice::screen_share::ScreenShareLimiter;
use vc_server::voice::sfu::SfuServer;

/// Object storage endpoint for S3 tests: `TEST_S3_ENDPOINT` (e.g.
/// `http://127.0.0.1:55445`), default the dev compose `RustFS` on port 9000.
pub fn test_s3_endpoint() -> String {
    std::env::var("TEST_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string())
}

/// `host:port` of an object storage endpoint URL (scheme, path and
/// trailing slash removed; `localhost` as `127.0.0.1`).
fn endpoint_addr(endpoint: &str) -> String {
    let rest = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_port = rest.split('/').next().unwrap_or(rest);
    let host_port = host_port.replace("localhost", "127.0.0.1");
    if host_port.contains(':') {
        host_port
    } else if endpoint.starts_with("https://") {
        format!("{host_port}:443")
    } else {
        format!("{host_port}:80")
    }
}

/// Whether the `CI` environment variable marks a CI run.
fn running_in_ci(ci: Option<&str>) -> bool {
    ci.is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// What an object-storage test does: run (`Ok(true)`), skip (`Ok(false)`,
/// only outside CI) or fail (`Err`, CI without a reachable store).
fn object_store_decision(reachable: bool, ci: bool) -> Result<bool, &'static str> {
    match (reachable, ci) {
        (true, _) => Ok(true),
        (false, false) => Ok(false),
        (false, true) => Err("CI is set but the object storage for tests is not reachable"),
    }
}

/// Whether the object storage for tests is reachable.
///
/// In CI (`CI` set) an unreachable store fails the test instead of
/// skipping it, so the storage tests cannot pass without running. Locally
/// the test is skipped with one line on stderr.
pub async fn rustfs_available() -> bool {
    let endpoint = test_s3_endpoint();
    let reachable = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        tokio::net::TcpStream::connect(endpoint_addr(&endpoint)),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    match object_store_decision(
        reachable,
        running_in_ci(std::env::var("CI").ok().as_deref()),
    ) {
        Ok(true) => true,
        Ok(false) => {
            eprintln!(
                "object storage at {endpoint} not reachable: skipping this test \
                 (set TEST_S3_ENDPOINT; in CI it would fail)"
            );
            false
        }
        Err(msg) => panic!("{msg}: {endpoint}"),
    }
}

#[test]
fn object_store_tests_fail_in_ci_without_a_store() {
    assert_eq!(object_store_decision(true, true), Ok(true));
    assert_eq!(object_store_decision(true, false), Ok(true));
    assert_eq!(object_store_decision(false, false), Ok(false));
    assert!(object_store_decision(false, true).is_err());
    assert!(running_in_ci(Some("true")));
    assert!(running_in_ci(Some("1")));
    assert!(!running_in_ci(None));
    assert!(!running_in_ci(Some("")));
    assert!(!running_in_ci(Some("false")));
}

#[test]
fn endpoint_addr_ignores_path_and_trailing_slash() {
    assert_eq!(endpoint_addr("http://localhost:9000"), "127.0.0.1:9000");
    assert_eq!(endpoint_addr("http://127.0.0.1:55445/"), "127.0.0.1:55445");
    assert_eq!(endpoint_addr("http://rustfs:9000/base"), "rustfs:9000");
    assert_eq!(endpoint_addr("https://s3.example"), "s3.example:443");
}

// ============================================================================
// Test App
// ============================================================================

/// Aborts the test app's session-end listener when the app is dropped, so
/// its connection is gone before `#[sqlx::test]` drops the test database
/// (`PostgreSQL` waits up to 5 s for other backends, then refuses the drop).
pub struct AbortOnDrop(pub Option<JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(h) = self.0.take() {
            h.abort();
        }
    }
}

/// A test application wrapping the full axum router.
pub struct TestApp {
    /// The session-end listener (DC5), aborted on drop.
    pub session_end: AbortOnDrop,
    pub router: Router,
    pub pool: PgPool,
    pub config: Arc<Config>,
    /// The state behind `router`, for calling server functions directly
    /// (e.g. `vc_server::push::deliver`).
    pub state: AppState,
}

/// Test config with every optional feature (`KAIKU_OPTIONAL_FEATURES`) on.
/// The plain test config keeps the production default (all off).
pub fn all_features_config() -> Config {
    let mut config = Config::default_for_test();
    config.optional_features = vc_server::features::OptionalFeatures {
        forum: true,
        announcements: true,
        events: true,
        screening: true,
    };
    config
}

impl TestApp {
    /// [`TestApp::with_pool`] with every optional feature turned on, for tests
    /// of forum/announcement channels, scheduled events and screening.
    pub async fn with_all_features(pool: PgPool) -> Self {
        Self::with_pool_and_config(pool, all_features_config()).await
    }

    /// Canonical pool-injecting constructor for integration tests.
    ///
    /// Pass a pool from `#[sqlx::test]`'s fixture. The returned `TestApp`
    /// owns `pool` for the duration of the test.
    pub async fn with_pool(pool: PgPool) -> Self {
        let config = Config::default_for_test();
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to test Redis");
        let sfu =
            SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

        let state = AppState::new(AppStateConfig {
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
        let router = create_router(state.clone());
        let config = Arc::new(config);
        // Committed session ends reach the sockets through the listener
        // (DC5), as in `main.rs`.
        let session_end = AbortOnDrop(Some(
            vc_server::auth::revocation::spawn_session_end_listener(
                state.db.clone(),
                state.redis.clone(),
            ),
        ));

        Self {
            session_end,
            router,
            pool,
            config,
            state,
        }
    }

    /// Pool-injecting constructor wired with a real `ScreenShareLimiter`
    /// backed by the shared Redis.
    ///
    /// Use this for screen share tests over the WebSocket
    /// (`voice_screen_share_start` is refused while `screen_share_limiter` is
    /// `None`).
    pub async fn with_pool_and_screen_share_limiter(pool: PgPool) -> Self {
        let config = Config::default_for_test();
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to test Redis");
        let sfu =
            SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

        let mut limiter = ScreenShareLimiter::new(redis.clone());
        limiter
            .init()
            .await
            .expect("Failed to initialize ScreenShareLimiter");

        let state = AppState::new(AppStateConfig {
            db: pool.clone(),
            redis,
            config: config.clone(),
            s3: None,
            sfu,
            rate_limiter: None,
            screen_share_limiter: Some(limiter),
            email: None,
            oidc_manager: None,
            http_client: reqwest::Client::new(),
        });
        let router = create_router(state.clone());
        let config = Arc::new(config);
        // Committed session ends reach the sockets through the listener
        // (DC5), as in `main.rs`.
        let session_end = AbortOnDrop(Some(
            vc_server::auth::revocation::spawn_session_end_listener(
                state.db.clone(),
                state.redis.clone(),
            ),
        ));

        Self {
            session_end,
            router,
            pool,
            config,
            state,
        }
    }

    /// Create a test app with a custom config and a caller-supplied pool
    /// (typically from `#[sqlx::test]`).
    pub async fn with_pool_and_config(pool: PgPool, config: Config) -> Self {
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to test Redis");
        let sfu =
            SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

        let state = AppState::new(AppStateConfig {
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
        let router = create_router(state.clone());
        let config = Arc::new(config);
        // Committed session ends reach the sockets through the listener
        // (DC5), as in `main.rs`.
        let session_end = AbortOnDrop(Some(
            vc_server::auth::revocation::spawn_session_end_listener(
                state.db.clone(),
                state.redis.clone(),
            ),
        ));

        Self {
            session_end,
            router,
            pool,
            config,
            state,
        }
    }

    /// Pool-injecting constructor with S3 configured against an endpoint
    /// nothing listens on (`127.0.0.1:9`), so every object upload fails.
    ///
    /// Use this to exercise storage-failure paths without `RustFS`.
    pub async fn with_pool_and_unreachable_s3(pool: PgPool) -> Self {
        let mut config = Config::default_for_test();
        config.s3_endpoint = Some("http://127.0.0.1:9".to_string());
        config.s3_bucket = "unreachable".to_string();
        config.s3_access_key = Some("dummy-access-key".to_string());
        config.s3_secret_key = Some("dummy-secret-key".to_string());
        let s3 = S3Client::new(&config).expect("Failed to create S3 client for test");

        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to test Redis");
        let sfu =
            SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

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
        let router = create_router(state.clone());
        let config = Arc::new(config);
        // Committed session ends reach the sockets through the listener
        // (DC5), as in `main.rs`.
        let session_end = AbortOnDrop(Some(
            vc_server::auth::revocation::spawn_session_end_listener(
                state.db.clone(),
                state.redis.clone(),
            ),
        ));

        Self {
            session_end,
            router,
            pool,
            config,
            state,
        }
    }

    /// Build an HTTP request with the given method and URI.
    pub fn request(method: Method, uri: &str) -> http::request::Builder {
        Request::builder().method(method).uri(uri)
    }

    /// Send a request through the router via `tower::ServiceExt::oneshot`.
    pub async fn oneshot(&self, request: Request<Body>) -> Response<Body> {
        self.router
            .clone()
            .oneshot(request)
            .await
            .expect("oneshot request failed")
    }
}

/// Build a [`TestApp`] with S3 connected to a local `RustFS` instance,
/// using the provided pool.
///
/// Pool-injecting helper for tests that use `#[sqlx::test]` per-test
/// database isolation.
///
/// Requires `canis-dev-rustfs` running on `localhost:9000`.
/// Creates a unique test bucket per invocation; callers are responsible
/// for any bucket teardown they need.
///
/// Environment variables must be set before calling:
/// - `AWS_ACCESS_KEY_ID=rustfsdev`
/// - `AWS_SECRET_ACCESS_KEY=rustfsdev_secret`
pub async fn fresh_test_app_with_s3_and_pool(pool: PgPool) -> (TestApp, String) {
    fresh_test_app_with_s3_and_config(pool, Config::default_for_test()).await
}

/// Like [`fresh_test_app_with_s3_and_pool`], starting from `config` (its S3
/// settings are overwritten).
pub async fn fresh_test_app_with_s3_and_config(
    pool: PgPool,
    mut config: Config,
) -> (TestApp, String) {
    let bucket = format!("test-{}", Uuid::now_v7());
    config.s3_endpoint = Some(test_s3_endpoint());
    config.s3_bucket = bucket.clone();
    config.s3_access_key = Some("rustfsdev".to_string());
    config.s3_secret_key = Some("rustfsdev_secret".to_string());

    let s3 = S3Client::new(&config).expect("Failed to create S3 client for test");

    // Create the test bucket
    s3.create_bucket_if_not_exists()
        .await
        .expect("Failed to create test bucket");

    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("Failed to connect to test Redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

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
    let router = create_router(state.clone());
    let session_end = AbortOnDrop(Some(
        vc_server::auth::revocation::spawn_session_end_listener(
            state.db.clone(),
            state.redis.clone(),
        ),
    ));

    (
        TestApp {
            session_end,
            router,
            pool,
            config: Arc::new(config),
            state,
        },
        bucket,
    )
}

// ============================================================================
// Test Server (Issue #139)
// ============================================================================

/// A running test server bound to a random port.
pub struct TestServer {
    /// Server address (127.0.0.1:PORT).
    pub addr: SocketAddr,
    /// Base URL for HTTP requests (e.g., `http://127.0.0.1:12345`).
    pub url: String,
    /// Handle to the server task for cleanup.
    _handle: JoinHandle<()>,
}

/// Spawn a real HTTP server on a random port.
///
/// Use this instead of `oneshot` when testing stateful middleware behavior
/// (rate limiting, request IDs, CORS preflight caching, etc.) since `oneshot`
/// resets middleware state between requests.
///
/// # Example
///
/// ```ignore
/// #[sqlx::test]
/// async fn my_server_test(pool: PgPool) {
///     let app = TestApp::with_pool(pool.clone()).await;
///     let server = spawn_test_server(app.router.clone()).await;
///
///     let client = reqwest::Client::new();
///     let resp = client.get(format!("{}/api/health", server.url)).send().await?;
/// }
/// ```
pub async fn spawn_test_server(router: Router) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    serve_test_router(listener, router)
}

/// Like [`spawn_test_server`] on an already bound listener (for tests that
/// must know the server URL before building the router).
pub fn serve_test_router(listener: tokio::net::TcpListener, router: Router) -> TestServer {
    let addr = listener.local_addr().expect("Failed to get local addr");
    let url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("Test server failed");
    });

    TestServer {
        addr,
        url,
        _handle: handle,
    }
}

// ============================================================================
// User & Auth helpers
// ============================================================================

/// Namespace of the deterministic test session ids (DC7).
pub const TEST_SESSION_NS: Uuid = Uuid::from_u128(0x6b61_696b_7500_4000_8000_7465_7374_0000);

/// Deterministic UUID from the namespace and a name (SHA-256, version 8),
/// so tests need no `uuid` v5 feature.
fn derived_uuid(ns: Uuid, name: &[u8]) -> Uuid {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(ns.as_bytes());
    h.update(name);
    let d = h.finalize();
    let mut b = [0u8; 16];
    b.copy_from_slice(&d[..16]);
    b[6] = (b[6] & 0x0f) | 0x80; // version 8 (custom)
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 9562 variant
    Uuid::from_bytes(b)
}

/// The session id [`generate_access_token`] uses for `user_id` (DC7).
pub fn test_session_id(user_id: Uuid) -> Uuid {
    derived_uuid(TEST_SESSION_NS, user_id.as_bytes())
}

/// The fake key device behind the test session (no `approval_devices` row).
pub fn test_authenticator_id(user_id: Uuid) -> Uuid {
    derived_uuid(
        TEST_SESSION_NS,
        &[user_id.as_bytes().as_slice(), b"authenticator"].concat(),
    )
}

/// Wait (≤ 5 s) until at least `n` backends of this test database wait for
/// a lock: a request that blocked inside its transaction. Panics otherwise,
/// so a test can prove that a transaction takes a lock it should take.
pub async fn wait_for_lock_waiters(pool: &PgPool, n: i64) {
    for _ in 0..250 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity
              WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no transaction waited for a lock within 5 s");
}

/// Insert a live session row `id` for `user_id` with the given assurance.
/// `full` sessions get a (fake) key-device authenticator, as the CHECK
/// `full_needs_authenticator` requires.
pub async fn insert_session_row(pool: &PgPool, id: Uuid, user_id: Uuid, full: bool) {
    let (assurance, methods, class, kind, auth_id) = if full {
        (
            "full",
            vec!["device_key".to_string()],
            "android",
            Some("key_device"),
            Some(test_authenticator_id(user_id)),
        )
    } else {
        ("probation", vec!["qr".to_string()], "web", None, None)
    };
    let token_hash = vc_server::auth::hash_token(&format!("test_session_{}", Uuid::new_v4()));
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, assurance, auth_methods,
                               client_class, authenticator_type, authenticator_id,
                               absolute_expires_at)
         VALUES ($1, $2, $3, NOW() + INTERVAL '7 days', $4, $5, $6, $7, $8,
                 NOW() + INTERVAL '30 days')
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(user_id)
    .bind(&token_hash)
    .bind(assurance)
    .bind(&methods)
    .bind(class)
    .bind(kind)
    .bind(auth_id)
    .execute(pool)
    .await
    .expect("Failed to insert test session");
}

/// Make sure the deterministic test session of `user_id` exists (for users
/// created without [`create_test_user`]).
pub async fn ensure_test_session(pool: &PgPool, user_id: Uuid) {
    insert_session_row(pool, test_session_id(user_id), user_id, true).await;
}

/// Create a test user and return `(user_id, username)`.
///
/// Also inserts the user's deterministic `full` test session (DC7), so
/// [`generate_access_token`] works for the new user.
pub async fn create_test_user(pool: &PgPool) -> (Uuid, String) {
    const MAX_ATTEMPTS: usize = 6;
    let test_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("httptest_{test_id}");

    for attempt in 1..=MAX_ATTEMPTS {
        match db::create_user(pool, &username, "HTTP Test User", None, "hash").await {
            Ok(user) => {
                ensure_test_session(pool, user.id).await;
                return (user.id, username);
            }
            Err(sqlx::Error::PoolTimedOut) if attempt < MAX_ATTEMPTS => {
                tracing::warn!(
                    attempt,
                    max_attempts = MAX_ATTEMPTS,
                    "Pool timed out creating test user; retrying"
                );
                tokio::time::sleep(Duration::from_millis((attempt as u64) * 200)).await;
            }
            Err(err) => panic!("Failed to create test user: {err:?}"),
        }
    }

    unreachable!("create_test_user retry loop must return or panic")
}

/// Make `user_id` a B.0 *new* account: no password, TOTP, session,
/// recovery codes, identity or first activity (a test user starts with a
/// password hash and a session, which make it existing).
pub async fn make_new_account(pool: &PgPool, user_id: Uuid) {
    for sql in [
        "DELETE FROM sessions WHERE user_id = $1",
        "DELETE FROM recovery_codes WHERE user_id = $1",
        "DELETE FROM user_identities WHERE user_id = $1",
        "UPDATE users SET first_activity_at = NULL, password_hash = NULL, mfa_secret = NULL
          WHERE id = $1",
    ] {
        sqlx::query(sql)
            .bind(user_id)
            .execute(pool)
            .await
            .expect("make the account new");
    }
}

/// Grant system admin to a user.
pub async fn make_admin(pool: &PgPool, user_id: Uuid) {
    sqlx::query("INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to grant admin");
}

/// Generate an access token for the given user, bound to the user's
/// deterministic test session (DC7; [`create_test_user`] inserts it).
pub fn generate_access_token(config: &Config, user_id: Uuid) -> String {
    let pair = tokens::token_pair_for_session(
        user_id,
        test_session_id(user_id),
        &config.jwt_private_key,
        config.jwt_access_expiry,
        config.jwt_refresh_expiry,
    )
    .expect("Failed to generate token pair");
    pair.access_token
}

/// Delete a user by ID (cascades to friendships, reports, etc.).
pub async fn delete_user(pool: &PgPool, user_id: Uuid) {
    for attempt in 0..5 {
        match sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
        {
            Ok(_) => return,
            Err(sqlx::Error::PoolTimedOut) if attempt < 4 => {
                tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1))).await;
            }
            Err(err) => panic!("Failed to delete test user: {err}"),
        }
    }
}

/// Create an accepted friendship between two users.
pub async fn create_friendship(pool: &PgPool, user_a: Uuid, user_b: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO friendships (id, requester_id, addressee_id, status) VALUES ($1, $2, $3, 'accepted')",
    )
    .bind(id)
    .bind(user_a)
    .bind(user_b)
    .execute(pool)
    .await
    .expect("Failed to create friendship");
    id
}

/// Create a DM channel between two users and return the channel ID.
pub async fn create_dm_channel(pool: &PgPool, user_a: Uuid, user_b: Uuid) -> Uuid {
    let channel_id = Uuid::now_v7();
    sqlx::query("INSERT INTO channels (id, name, channel_type) VALUES ($1, 'DM', 'dm')")
        .bind(channel_id)
        .execute(pool)
        .await
        .expect("Failed to create DM channel");

    sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2), ($1, $3)")
        .bind(channel_id)
        .bind(user_a)
        .bind(user_b)
        .execute(pool)
        .await
        .expect("Failed to add DM participants");

    channel_id
}

/// Insert a login session row for `user_id` and return `(session_id, access_token)`.
///
/// The access token's `sid` is the new session, as after a real login.
pub async fn create_session_with_token(
    pool: &PgPool,
    config: &Config,
    user_id: Uuid,
) -> (Uuid, String) {
    session_with_token(pool, config, user_id, true).await
}

/// Like [`create_session_with_token`], but the session is `probation`.
pub async fn create_probation_session_with_token(
    pool: &PgPool,
    config: &Config,
    user_id: Uuid,
) -> (Uuid, String) {
    session_with_token(pool, config, user_id, false).await
}

async fn session_with_token(
    pool: &PgPool,
    config: &Config,
    user_id: Uuid,
    full: bool,
) -> (Uuid, String) {
    let session_id = Uuid::now_v7();
    insert_session_row(pool, session_id, user_id, full).await;

    let pair = tokens::token_pair_for_session(
        user_id,
        session_id,
        &config.jwt_private_key,
        config.jwt_access_expiry,
        config.jwt_refresh_expiry,
    )
    .expect("Failed to generate token pair");
    (session_id, pair.access_token)
}

/// Turn a test session into a `full` one (as if a key device had created
/// it), for tests that act from a password sign-in on routes that need a
/// full session (C27). Its authenticator is the user's fake test device.
pub async fn make_session_full(pool: &PgPool, sid: Uuid) {
    sqlx::query(
        "UPDATE sessions SET assurance = 'full', auth_methods = '{device_key}',
                authenticator_type = 'key_device',
                authenticator_id = $2
          WHERE id = $1",
    )
    .bind(sid)
    .bind(test_authenticator_id(
        sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM sessions WHERE id = $1")
            .bind(sid)
            .fetch_one(pool)
            .await
            .expect("session"),
    ))
    .execute(pool)
    .await
    .expect("make the session full");
}

/// A step-up grant (F7) for `action`/`target` on the full session `sid`, as
/// a `device_stepup` would store it. The session's authenticator gets an
/// active key-device row if it has none (the fake one of a test session),
/// so `consume_tx` finds it active. For tests of routes behind the step-up
/// guard; tests of the grant itself use `helpers::login::device_grant`.
pub async fn grant_step_up(pool: &PgPool, sid: Uuid, action: &str, target: &str) {
    let (user_id, kind, auth_id): (Uuid, Option<String>, Option<Uuid>) = sqlx::query_as(
        "SELECT user_id, authenticator_type, authenticator_id FROM sessions WHERE id = $1",
    )
    .bind(sid)
    .fetch_one(pool)
    .await
    .expect("session for the grant");
    let auth_id = auth_id.expect("a full session has an authenticator");
    if kind.as_deref() == Some("key_device") {
        let key = p256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
        let der = approvals::spki_der(&key);
        sqlx::query(
            "INSERT INTO approval_devices
                (id, user_id, device_id, public_key, public_key_hash, uv_class, label, state,
                 enrolled_via, activated_at)
             VALUES ($1, $2, $3, $4, $5, 'per_use', 'Test phone', 'active', 'cli', NOW())
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(auth_id)
        .bind(user_id)
        .bind(Uuid::new_v4())
        .bind(&der)
        .bind(<sha2::Sha256 as sha2::Digest>::digest(&der).to_vec())
        .execute(pool)
        .await
        .expect("test key device row");
    }
    sqlx::query(
        "INSERT INTO step_up_grants (id, session_id, user_id, action, target_hash, target, via,
                                     authenticator_type, authenticator_id, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, 'device', $7, $8, NOW(), NOW() + INTERVAL '300 seconds')",
    )
    .bind(Uuid::new_v4())
    .bind(sid)
    .bind(user_id)
    .bind(action)
    .bind(vc_server::approvals::context::target_hash(target).to_vec())
    .bind(target)
    .bind(kind.as_deref().unwrap_or("key_device"))
    .bind(auth_id)
    .execute(pool)
    .await
    .expect("insert step-up grant");
}

/// Create an elevated admin session (valid for 15 minutes).
///
/// Creates a session row (required FK) and elevates it. Returns an access
/// token of that session: an elevation only counts for its own session.
pub async fn create_elevated_session(pool: &PgPool, config: &Config, user_id: Uuid) -> String {
    let (session_id, token) = create_session_with_token(pool, config, user_id).await;

    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, elevated_at, expires_at, reason) VALUES ($1, $2, '127.0.0.1'::inet, NOW(), NOW() + INTERVAL '15 minutes', 'test')",
    )
    .bind(user_id)
    .bind(session_id)
    .execute(pool)
    .await
    .expect("Failed to create elevated session");
    token
}

/// Create a test report and return the report ID.
pub async fn create_test_report(pool: &PgPool, reporter_id: Uuid, target_id: Uuid) -> Uuid {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO user_reports (reporter_id, target_type, target_user_id, category, description) VALUES ($1, 'user', $2, 'harassment', 'Test report') RETURNING id",
    )
    .bind(reporter_id)
    .bind(target_id)
    .fetch_one(pool)
    .await
    .expect("Failed to create test report");
    row.0
}

/// Collect a response body and parse it as JSON.
pub async fn body_to_json(response: Response<Body>) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("Failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        let preview = String::from_utf8_lossy(&bytes);
        panic!("Failed to parse response as JSON: {e}\nBody: {preview}")
    })
}

/// Like [`body_to_json`], but an empty or non-JSON body is `Value::Null`
/// (404s of unknown routes have no body).
pub async fn body_to_json_lenient(response: Response<Body>) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("Failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

// ============================================================================
// Data helpers (guilds, channels, messages)
// ============================================================================

/// Create a guild with the given owner and return its ID.
pub async fn create_guild(pool: &PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::now_v7();
    let name = format!("TestGuild_{}", &guild_id.to_string()[..8]);

    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind(&name)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to create guild");

    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to add guild member");

    guild_id
}

/// Create a guild with an `@everyone` role that has the given permissions.
///
/// Combines [`create_guild`] + role creation in one call.
pub async fn create_guild_with_default_role(
    pool: &PgPool,
    owner_id: Uuid,
    everyone_perms: GuildPermissions,
) -> Uuid {
    let guild_id = create_guild(pool, owner_id).await;
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default) VALUES ($1, $2, '@everyone', $3, 0, true)",
    )
    .bind(Uuid::now_v7())
    .bind(guild_id)
    .bind(everyone_perms.to_db())
    .execute(pool)
    .await
    .expect("Failed to create @everyone role");
    guild_id
}

/// Create a text channel in a guild and return its ID.
pub async fn create_channel(pool: &PgPool, guild_id: Uuid, name: &str) -> Uuid {
    let channel_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, 'text')",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(name)
    .execute(pool)
    .await
    .expect("Failed to create channel");

    channel_id
}

/// Create a voice channel in a guild and return its ID.
pub async fn create_voice_channel(pool: &PgPool, guild_id: Uuid, name: &str) -> Uuid {
    let channel_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, 'voice')",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(name)
    .execute(pool)
    .await
    .expect("Failed to create voice channel");
    channel_id
}

/// Insert a message and return its ID.
pub async fn insert_message(pool: &PgPool, channel_id: Uuid, user_id: Uuid, content: &str) -> Uuid {
    let msg_id = Uuid::now_v7();

    sqlx::query("INSERT INTO messages (id, channel_id, user_id, content) VALUES ($1, $2, $3, $4)")
        .bind(msg_id)
        .bind(channel_id)
        .bind(user_id)
        .bind(content)
        .execute(pool)
        .await
        .expect("Failed to insert message");

    msg_id
}

/// Insert an encrypted message and return its ID.
pub async fn insert_encrypted_message(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
) -> Uuid {
    let msg_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, encrypted, nonce) VALUES ($1, $2, $3, $4, true, 'dGVzdF9ub25jZQ==')",
    )
    .bind(msg_id)
    .bind(channel_id)
    .bind(user_id)
    .bind(content)
    .execute(pool)
    .await
    .expect("Failed to insert encrypted message");

    msg_id
}

/// Insert a file attachment for a message.
pub async fn insert_attachment(pool: &PgPool, message_id: Uuid) {
    sqlx::query(
        "INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key) VALUES ($1, 'test.png', 'image/png', 1024, 'uploads/test.png')",
    )
    .bind(message_id)
    .execute(pool)
    .await
    .expect("Failed to insert attachment");
}

/// Add a user as a guild member.
pub async fn add_guild_member(pool: &PgPool, guild_id: Uuid, user_id: Uuid) {
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to add guild member");
}

/// Insert a message with a custom timestamp and return its ID.
pub async fn insert_message_at(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
    created_at: &str,
) -> Uuid {
    let msg_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, created_at) VALUES ($1, $2, $3, $4, $5::timestamptz)",
    )
    .bind(msg_id)
    .bind(channel_id)
    .bind(user_id)
    .bind(content)
    .bind(created_at)
    .execute(pool)
    .await
    .expect("Failed to insert message with timestamp");

    msg_id
}

/// Insert a soft-deleted message and return its ID.
pub async fn insert_deleted_message(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
) -> Uuid {
    let msg_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, deleted_at) VALUES ($1, $2, $3, $4, NOW())",
    )
    .bind(msg_id)
    .bind(channel_id)
    .bind(user_id)
    .bind(content)
    .execute(pool)
    .await
    .expect("Failed to insert deleted message");

    msg_id
}

/// Delete a DM channel by ID (cascades messages and participants).
pub async fn delete_dm_channel(pool: &PgPool, channel_id: Uuid) {
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel_id)
        .execute(pool)
        .await
        .ok();
}

/// Delete connection session data (metrics + session row).
pub async fn delete_connection_data(pool: &PgPool, session_id: Uuid) {
    sqlx::query("DELETE FROM connection_metrics WHERE session_id = $1")
        .bind(session_id)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM connection_sessions WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await
        .ok();
}

/// Delete a guild (cascades channels, messages, members).
pub async fn delete_guild(pool: &PgPool, guild_id: Uuid) {
    sqlx::query("DELETE FROM channels WHERE guild_id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .ok();
}

// ============================================================================
// Bot & Webhook helpers
// ============================================================================

/// Create a bot application, bot user, and return `(app_id, bot_user_id, token)`.
pub async fn create_bot_application(pool: &PgPool, owner_id: Uuid) -> (Uuid, Uuid, String) {
    let app_id = Uuid::now_v7();
    let bot_user_id = Uuid::now_v7();
    let bot_username_seed = bot_user_id.simple().to_string();
    let bot_username = format!("bot_{}", &bot_username_seed[20..]);

    // Create bot user
    sqlx::query(
        "INSERT INTO users (id, username, display_name, password_hash, is_bot, bot_owner_id, status) VALUES ($1, $2, $3, 'bot_token_only', true, $4, 'offline')",
    )
    .bind(bot_user_id)
    .bind(&bot_username)
    .bind(format!("{bot_username} (Bot)"))
    .bind(owner_id)
    .execute(pool)
    .await
    .expect("Failed to create bot user");

    // Create application with dummy token hash
    let token = format!("{bot_user_id}.test_secret");
    let token_hash = vc_server::auth::hash_token(&token);

    sqlx::query(
        "INSERT INTO bot_applications (id, owner_id, name, bot_user_id, token_hash, gateway_intents) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(app_id)
    .bind(owner_id)
    .bind(&bot_username)
    .bind(bot_user_id)
    .bind(&token_hash)
    .bind(&["commands".to_string()] as &[String])
    .execute(pool)
    .await
    .expect("Failed to create bot application");

    (app_id, bot_user_id, token)
}

/// Create a webhook for an application and return the webhook ID.
pub async fn create_test_webhook(pool: &PgPool, app_id: Uuid, url: &str, events: &[&str]) -> Uuid {
    let webhook_id = Uuid::now_v7();
    let secret = "test_signing_secret_0123456789abcdef0123456789abcdef";

    sqlx::query(
        "INSERT INTO webhooks (id, application_id, url, signing_secret, subscribed_events, active) VALUES ($1, $2, $3, $4, $5::webhook_event_type[], true)",
    )
    .bind(webhook_id)
    .bind(app_id)
    .bind(url)
    .bind(secret)
    .bind(events)
    .execute(pool)
    .await
    .expect("Failed to create test webhook");

    webhook_id
}

/// Install a bot application in a guild.
pub async fn install_bot_in_guild(pool: &PgPool, guild_id: Uuid, app_id: Uuid, user_id: Uuid) {
    sqlx::query(
        "INSERT INTO guild_bot_installations (guild_id, application_id, installed_by) VALUES ($1, $2, $3)",
    )
    .bind(guild_id)
    .bind(app_id)
    .bind(user_id)
    .execute(pool)
    .await
    .expect("Failed to install bot in guild");
}

/// Delete a bot application (cascades to webhooks, users, etc.).
pub async fn delete_bot_application(pool: &PgPool, app_id: Uuid) {
    // Delete guild installations first
    sqlx::query("DELETE FROM guild_bot_installations WHERE application_id = $1")
        .bind(app_id)
        .execute(pool)
        .await
        .ok();
    // Delete the application (cascades to webhooks, slash commands)
    sqlx::query("DELETE FROM bot_applications WHERE id = $1")
        .bind(app_id)
        .execute(pool)
        .await
        .ok();
}

// ============================================================================
// WebSocket client harness
// ============================================================================

/// WebSocket client connected to a [`TestServer`].
pub type WsClient =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Connect to `ws://{addr}/ws` and authenticate with a post-connect
/// `{"type":"authenticate","token":…}` frame.
///
/// Does not wait for `ready`; use [`next_ws_event`] with `"ready"` for that.
pub async fn ws_connect(server: &TestServer, token: &str) -> WsClient {
    use futures::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let url = format!("ws://{}/ws", server.addr);
    let (mut ws, _response) = tokio_tungstenite::connect_async(url)
        .await
        .expect("WebSocket connect failed");
    let auth = serde_json::json!({ "type": "authenticate", "token": token }).to_string();
    ws.send(Message::Text(auth.into()))
        .await
        .expect("Failed to send authenticate frame");
    ws
}

/// Wait for the next server event whose `type` equals `event_type`.
///
/// Other events and control frames are skipped. Panics if the socket closes
/// or no matching event arrives within `timeout`.
pub async fn next_ws_event(
    ws: &mut WsClient,
    event_type: &str,
    timeout: Duration,
) -> serde_json::Value {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&text).expect("server sent invalid JSON");
                    if value["type"] == event_type {
                        return value;
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    panic!("WebSocket closed while waiting for {event_type:?}: {frame:?}")
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("WebSocket error while waiting for {event_type:?}: {e}"),
                None => panic!("WebSocket stream ended while waiting for {event_type:?}"),
            }
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .unwrap_or_else(|_| panic!("timed out after {timeout:?} waiting for {event_type:?}"))
}

/// Assert that no server event whose `type` equals `event_type` arrives within
/// `timeout`.
///
/// Other events and control frames are skipped. Panics if a matching event
/// arrives, or if the socket closes or errors (the check would be vacuous).
pub async fn expect_no_ws_event(ws: &mut WsClient, event_type: &str, timeout: Duration) {
    use futures::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    let wait = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&text).expect("server sent invalid JSON");
                    if value["type"] == event_type {
                        panic!("unexpected {event_type:?} event: {value}");
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    panic!("WebSocket closed while checking for {event_type:?}: {frame:?}")
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("WebSocket error while checking for {event_type:?}: {e}"),
                None => panic!("WebSocket stream ended while checking for {event_type:?}"),
            }
        }
    };
    // Timing out is the expected outcome.
    let _ = tokio::time::timeout(timeout, wait).await;
}

// ============================================================================
// FCM mock (local axum on 127.0.0.1; no test reaches Google)
// ============================================================================

include!("../../fixtures/fcm_test_key.rs");

/// How the FCM mock answers `messages:send`.
#[derive(Clone, Copy, Debug)]
pub enum FcmMockResponse {
    /// 200 with a message name.
    Ok,
    /// The given status and body for every send.
    Status(u16, &'static str),
}

/// A running FCM mock. Serves `/token` (`OAuth2`) and
/// `/v1/projects/{p}/messages:send`.
pub struct FcmMock {
    pub base_url: String,
    /// Bodies of every `messages:send` request, in order.
    pub received: Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    _handle: JoinHandle<()>,
}

impl FcmMock {
    /// The mock's `OAuth2` token endpoint.
    pub fn token_uri(&self) -> String {
        format!("{}/token", self.base_url)
    }
}

#[derive(Clone)]
struct FcmMockState {
    responder: FcmMockResponse,
    received: Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
}

pub async fn spawn_fcm_mock(responder: FcmMockResponse) -> FcmMock {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::routing::post;

    async fn token() -> axum::Json<serde_json::Value> {
        axum::Json(serde_json::json!({
            "access_token": "mock-access-token",
            "expires_in": 3599,
            "token_type": "Bearer"
        }))
    }

    async fn send(
        State(st): State<FcmMockState>,
        Path((_project, action)): Path<(String, String)>,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> (StatusCode, String) {
        if action != "messages:send" {
            return (StatusCode::NOT_FOUND, String::new());
        }
        st.received.lock().unwrap().push(body);
        match st.responder {
            FcmMockResponse::Ok => (
                StatusCode::OK,
                r#"{"name":"projects/kaiku-test/messages/1"}"#.to_string(),
            ),
            FcmMockResponse::Status(code, body) => {
                (StatusCode::from_u16(code).unwrap(), body.to_string())
            }
        }
    }

    let received = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/token", post(token))
        .route("/v1/projects/{project}/{action}", post(send))
        .with_state(FcmMockState {
            responder,
            received: received.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind FCM mock");
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    FcmMock {
        base_url: format!("http://{addr}"),
        received,
        _handle: handle,
    }
}

/// A service account for the FCM mock, signed with the test-only RSA key in
/// `tests/fixtures/fcm_test_key.rs` (the shared JWT test key is Ed25519).
pub fn test_service_account(token_uri: String) -> vc_server::push::fcm::ServiceAccountKey {
    vc_server::push::fcm::ServiceAccountKey {
        project_id: "kaiku-test".into(),
        client_email: "push@kaiku-test.iam.gserviceaccount.com".into(),
        private_key: TEST_FCM_RSA_PRIVATE_KEY_PEM.into(),
        private_key_id: "test-key-id".into(),
        token_uri,
    }
}
