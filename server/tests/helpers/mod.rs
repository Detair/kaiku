//! Reusable test helpers for HTTP integration tests.
//!
//! Provides `TestApp` for building and sending requests through the full axum router,
//! plus utilities for user creation, admin grants, and JWT generation.
//!
//! ## Shared Resources
//!
//! Use [`shared_pool()`] and [`shared_redis()`] to avoid creating new connections per test.
//!
//! ## Cleanup Guards
//!
//! Use [`CleanupGuard`] for RAII-based cleanup that runs even if a test panics.
//!
//! ## Test Servers
//!
//! Use [`spawn_test_server()`] when you need stateful middleware testing
//! (rate limiting, request IDs, etc.) instead of `tower::ServiceExt::oneshot`.
#![allow(dead_code)]

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{self, Method, Request, Response};
use axum::Router;
use http_body_util::BodyExt;
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use tower::ServiceExt;
use uuid::Uuid;
use vc_server::api::{create_router, AppState};
use vc_server::auth::jwt;
use vc_server::config::Config;
use vc_server::db;
use vc_server::permissions::GuildPermissions;
use vc_server::voice::sfu::SfuServer;

// ============================================================================
// Shared resources (Issue #138)
// ============================================================================

/// Shared database pool across all tests in the same binary.
static SHARED_POOL: OnceCell<PgPool> = OnceCell::const_new();

/// Shared Redis client across all tests in the same binary.
static SHARED_REDIS: OnceCell<fred::clients::Client> = OnceCell::const_new();

/// Shared config across all tests in the same binary.
static SHARED_CONFIG: OnceCell<Config> = OnceCell::const_new();

/// Get or create a shared database pool.
///
/// Reuses a single pool across all test cases in the same binary,
/// avoiding connection exhaustion from creating pools per-test.
pub async fn shared_pool() -> &'static PgPool {
    SHARED_POOL
        .get_or_init(|| async {
            let config = shared_config().await;
            db::create_pool(&config.database_url)
                .await
                .expect("Failed to connect to test DB")
        })
        .await
}

/// Get or create a shared Redis client.
pub async fn shared_redis() -> &'static fred::clients::Client {
    SHARED_REDIS
        .get_or_init(|| async {
            let config = shared_config().await;
            db::create_redis_client(&config.redis_url)
                .await
                .expect("Failed to connect to test Redis")
        })
        .await
}

/// Get or create a shared config.
pub async fn shared_config() -> &'static Config {
    SHARED_CONFIG
        .get_or_init(|| async { Config::default_for_test() })
        .await
}

// ============================================================================
// Cleanup Guard (Issue #137)
// ============================================================================

/// Async cleanup action type.
type CleanupAction = Box<dyn FnOnce(PgPool) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send>;

/// RAII guard that runs cleanup actions on drop, even if the test panics.
///
/// # Example
///
/// ```ignore
/// let mut guard = CleanupGuard::new(app.pool.clone());
/// guard.delete_user(user_id);
/// guard.restore_setup_complete(prev);
///
/// // Test assertions here — cleanup runs even if these panic
/// assert_eq!(resp.status(), 200);
/// // guard dropped here → cleanup runs
/// ```
pub struct CleanupGuard {
    pool: PgPool,
    actions: Vec<CleanupAction>,
}

impl CleanupGuard {
    /// Create a new cleanup guard for the given pool.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            actions: Vec::new(),
        }
    }

    /// Register a generic async cleanup action.
    pub fn add<F, Fut>(&mut self, action: F)
    where
        F: FnOnce(PgPool) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.actions
            .push(Box::new(move |pool| Box::pin(action(pool))));
    }

    /// Register cleanup to delete a user by ID.
    pub fn delete_user(&mut self, user_id: Uuid) {
        self.add(move |pool| async move {
            let _ = sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user_id)
                .execute(&pool)
                .await;
        });
    }

    /// Register cleanup to restore `setup_complete` to a previous value.
    pub fn restore_setup_complete(&mut self, value: bool) {
        self.add(move |pool| async move {
            let _ = sqlx::query(
                "UPDATE server_config SET value = $1::jsonb WHERE key = 'setup_complete'",
            )
            .bind(serde_json::json!(value))
            .execute(&pool)
            .await;
        });
    }

    /// Register cleanup to restore default config values.
    pub fn restore_config_defaults(&mut self) {
        self.add(|pool| async move {
            for (key, val) in [
                ("server_name", serde_json::json!("Canis Server")),
                ("registration_policy", serde_json::json!("open")),
                ("terms_url", serde_json::Value::Null),
                ("privacy_url", serde_json::Value::Null),
            ] {
                let _ = sqlx::query(
                    "UPDATE server_config SET value = $1, updated_by = NULL WHERE key = $2",
                )
                .bind(val)
                .bind(key)
                .execute(&pool)
                .await;
            }
        });
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let actions = std::mem::take(&mut self.actions);
        if actions.is_empty() {
            return;
        }

        let pool = self.pool.clone();
        let handle = tokio::runtime::Handle::current();

        // Spawn a blocking thread to run async cleanup.
        // This works regardless of tokio runtime flavor.
        std::thread::spawn(move || {
            handle.block_on(async move {
                for action in actions {
                    action(pool.clone()).await;
                }
            });
        })
        .join()
        .expect("Cleanup thread panicked");
    }
}

// ============================================================================
// Test App
// ============================================================================

/// A test application wrapping the full axum router.
pub struct TestApp {
    pub router: Router,
    pub pool: PgPool,
    pub config: Arc<Config>,
}

impl TestApp {
    /// Create a new test app using shared DB and Redis connections.
    pub async fn new() -> Self {
        let pool = shared_pool().await.clone();
        let redis = shared_redis().await.clone();
        let config = shared_config().await.clone();
        let sfu =
            SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

        let state = AppState::new(
            pool.clone(),
            redis,
            config.clone(),
            None,
            sfu,
            None,
            None,
            None,
        );
        let router = create_router(state);
        let config = Arc::new(config);

        Self {
            router,
            pool,
            config,
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

    /// Create a [`CleanupGuard`] for this app's pool.
    pub fn cleanup_guard(&self) -> CleanupGuard {
        CleanupGuard::new(self.pool.clone())
    }
}

/// Build a [`TestApp`] with fresh DB and Redis resources for one test.
///
/// Prefer this helper for HTTP integration tests that are sensitive to stale
/// runtime-bound connections across `#[tokio::test]` runs.
pub async fn fresh_test_app() -> TestApp {
    let config = shared_config().await.clone();
    let pool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to test DB");
    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("Failed to connect to test Redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SfuServer");

    let state = AppState::new(
        pool.clone(),
        redis,
        config.clone(),
        None,
        sfu,
        None,
        None,
        None,
    );
    let router = create_router(state);

    TestApp {
        router,
        pool,
        config: Arc::new(config),
    }
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
/// let app = TestApp::new().await;
/// let server = spawn_test_server(app.router.clone()).await;
///
/// let client = reqwest::Client::new();
/// let resp = client.get(format!("{}/api/health", server.url)).send().await?;
/// ```
pub async fn spawn_test_server(router: Router) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind test server");
    let addr = listener.local_addr().expect("Failed to get local addr");
    let url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
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

/// Create a test user and return `(user_id, username)`.
pub async fn create_test_user(pool: &PgPool) -> (Uuid, String) {
    const MAX_ATTEMPTS: usize = 6;
    let test_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("httptest_{test_id}");

    for attempt in 1..=MAX_ATTEMPTS {
        match db::create_user(pool, &username, "HTTP Test User", None, "hash").await {
            Ok(user) => return (user.id, username),
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

/// Grant system admin to a user.
pub async fn make_admin(pool: &PgPool, user_id: Uuid) {
    sqlx::query("INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to grant admin");
}

/// Generate an access token for the given user.
pub fn generate_access_token(config: &Config, user_id: Uuid) -> String {
    let pair = jwt::generate_token_pair(
        user_id,
        &config.jwt_private_key,
        config.jwt_access_expiry,
        config.jwt_refresh_expiry,
    )
    .expect("Failed to generate token pair");
    pair.access_token
}

/// Delete a user by ID (cascades to friendships, reports, etc.).
pub async fn delete_user(pool: &PgPool, user_id: Uuid) {
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("Failed to delete test user");
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

/// Create an elevated admin session (valid for 15 minutes).
///
/// Creates a dummy session row first (required FK), then the elevated session.
pub async fn create_elevated_session(pool: &PgPool, user_id: Uuid) {
    let session_id = Uuid::now_v7();
    let token_hash = vc_server::auth::hash_token(&format!("test_elevated_{}", Uuid::new_v4()));
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '1 hour')",
    )
    .bind(session_id)
    .bind(user_id)
    .bind(&token_hash)
    .execute(pool)
    .await
    .expect("Failed to create session for elevation");

    sqlx::query(
        "INSERT INTO elevated_sessions (user_id, session_id, ip_address, elevated_at, expires_at, reason) VALUES ($1, $2, '127.0.0.1'::inet, NOW(), NOW() + INTERVAL '15 minutes', 'test')",
    )
    .bind(user_id)
    .bind(session_id)
    .execute(pool)
    .await
    .expect("Failed to create elevated session");
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
