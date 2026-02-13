//! HTTP Integration Tests for Upload Error Paths
//!
//! S3 is not configured in test environment (`AppState.s3 = None`),
//! so these tests verify error responses only.
//!
//! Run with: `cargo test --test uploads_http_test -- --nocapture`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{create_test_user, generate_access_token, TestApp};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

// ============================================================================
// Permission bits (from server/src/permissions/guild.rs)
// ============================================================================

const VIEW_CHANNEL: i64 = 1 << 24;
const SEND_MESSAGES: i64 = 1 << 0;

// ============================================================================
// Test Helpers
// ============================================================================

async fn create_guild_with_owner(pool: &PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::new_v4();
    sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind("Upload Test Guild")
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to create test guild");

    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(owner_id)
        .execute(pool)
        .await
        .expect("Failed to add owner as guild member");

    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default) VALUES ($1, $2, '@everyone', $3, 0, true)",
    )
    .bind(Uuid::new_v4())
    .bind(guild_id)
    .bind(VIEW_CHANNEL | SEND_MESSAGES)
    .execute(pool)
    .await
    .expect("Failed to create @everyone role");

    guild_id
}

async fn create_channel(pool: &PgPool, guild_id: Uuid, name: &str) -> Uuid {
    let channel_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO channels (id, name, channel_type, guild_id, position, max_screen_shares)
         VALUES ($1, $2, 'text', $3, 0, 5)",
    )
    .bind(channel_id)
    .bind(name)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("Failed to create test channel");
    channel_id
}

// ============================================================================
// Upload Error Paths
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_upload_returns_503_without_s3() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let guild_id = create_guild_with_owner(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "upload-503-test").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move {
        let _ = sqlx::query("DELETE FROM guilds WHERE id = $1")
            .bind(guild_id)
            .execute(&pool)
            .await;
    });
    guard.delete_user(user_id);

    // Build a minimal multipart body
    let boundary = "----TestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );

    let req = TestApp::request(
        Method::POST,
        &format!("/api/messages/channel/{channel_id}/upload"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header(
        "Content-Type",
        format!("multipart/form-data; boundary={boundary}"),
    )
    .body(Body::from(body))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        503,
        "Upload without S3 should return 503 Service Unavailable"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_upload_requires_auth() {
    let app = TestApp::new().await;
    let channel_id = Uuid::new_v4();

    let boundary = "----TestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );

    let req = TestApp::request(
        Method::POST,
        &format!("/api/messages/channel/{channel_id}/upload"),
    )
    .header(
        "Content-Type",
        format!("multipart/form-data; boundary={boundary}"),
    )
    .body(Body::from(body))
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401, "Upload without auth should return 401");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_get_attachment_not_found() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let fake_id = Uuid::new_v4();
    let req = TestApp::request(Method::GET, &format!("/api/messages/attachments/{fake_id}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        404,
        "GET nonexistent attachment should return 404"
    );
}
