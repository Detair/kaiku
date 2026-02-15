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
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

// ============================================================================
// Upload Error Paths
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_upload_returns_503_without_s3() {
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = helpers::create_channel(&app.pool, guild_id, "upload-503-test").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { helpers::delete_guild(&pool, guild_id).await });
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
    let app = helpers::fresh_test_app().await;
    let channel_id = Uuid::now_v7();

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
    let app = helpers::fresh_test_app().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let fake_id = Uuid::now_v7();
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
