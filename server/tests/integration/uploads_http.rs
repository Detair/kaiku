//! HTTP Integration Tests for Upload Error Paths
//!
//! S3 is not configured in test environment (`AppState.s3 = None`),
//! so these tests verify error responses only.
//!
//! Run with: `cargo test --test integration uploads_http -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{body_to_json, create_test_user, generate_access_token, TestApp};

// ============================================================================
// Upload Error Paths
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_upload_returns_503_without_s3() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "upload-503-test").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
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
    guard.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_upload_requires_auth() {
    let app = TestApp::new().await;
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
async fn test_get_attachment_not_found() {
    let app = TestApp::new().await;
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
        403,
        "GET nonexistent attachment should return 403"
    );
    let body = body_to_json(resp).await;
    assert_eq!(body["error"], "FORBIDDEN");
    guard.cleanup().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_get_attachment_anti_enumeration_parity() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (outsider_id, _) = create_test_user(&app.pool).await;
    let owner_token = generate_access_token(&app.config, owner_id);
    let outsider_token = generate_access_token(&app.config, outsider_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, owner_id, perms).await;
    let channel_id =
        super::helpers::create_channel(&app.pool, guild_id, "attachment-enum-test").await;
    let message_id =
        super::helpers::insert_message(&app.pool, channel_id, owner_id, "secret file").await;
    super::helpers::insert_attachment(&app.pool, message_id).await;
    let attachment_id: Uuid =
        sqlx::query_scalar("SELECT id FROM file_attachments WHERE message_id = $1")
            .bind(message_id)
            .fetch_one(&app.pool)
            .await
            .expect("Failed to fetch inserted attachment id");

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(owner_id);
    guard.delete_user(outsider_id);

    let owner_req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment_id}"),
    )
    .header("Authorization", format!("Bearer {owner_token}"))
    .body(Body::empty())
    .unwrap();
    let owner_resp = app.oneshot(owner_req).await;
    assert_eq!(
        owner_resp.status(),
        200,
        "Owner should access existing attachment"
    );

    let existing_req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment_id}"),
    )
    .header("Authorization", format!("Bearer {outsider_token}"))
    .body(Body::empty())
    .unwrap();
    let existing_resp = app.oneshot(existing_req).await;
    assert_eq!(
        existing_resp.status(),
        403,
        "Unauthorized user should get 403 for existing attachment"
    );
    let existing_body = body_to_json(existing_resp).await;
    assert_eq!(existing_body["error"], "FORBIDDEN");

    let missing_id = Uuid::now_v7();
    let missing_req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{missing_id}"),
    )
    .header("Authorization", format!("Bearer {outsider_token}"))
    .body(Body::empty())
    .unwrap();
    let missing_resp = app.oneshot(missing_req).await;
    assert_eq!(
        missing_resp.status(),
        403,
        "Unauthorized user should get 403 for nonexistent attachment"
    );
    let missing_body = body_to_json(missing_resp).await;
    assert_eq!(missing_body["error"], "FORBIDDEN");
    guard.cleanup().await;
}
