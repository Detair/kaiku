//! Reusable test helpers for HTTP integration tests.
//!
//! Provides `TestApp` for building and sending requests through the full axum router,
//! plus utilities for user creation, admin grants, and JWT generation.

use axum::{
    body::Body,
    http::{self, Method, Request, Response},
    Router,
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use vc_server::{
    api::{create_router, AppState},
    auth::jwt,
    config::Config,
    db,
    voice::sfu::SfuServer,
};

/// A test application wrapping the full axum router.
pub struct TestApp {
    pub router: Router,
    pub pool: PgPool,
    pub config: Arc<Config>,
}

impl TestApp {
    /// Create a new test app with real DB and Redis connections.
    pub async fn new() -> Self {
        let config = Config::default_for_test();
        let pool = db::create_pool(&config.database_url)
            .await
            .expect("Failed to connect to test DB");
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to test Redis");
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
}

/// Create a test user and return `(user_id, username)`.
pub async fn create_test_user(pool: &PgPool) -> (Uuid, String) {
    let test_id = Uuid::new_v4().to_string()[..8].to_string();
    let username = format!("httptest_{test_id}");
    let user = db::create_user(pool, &username, "HTTP Test User", None, "hash")
        .await
        .expect("Failed to create test user");
    (user.id, username)
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
