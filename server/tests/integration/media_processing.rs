//! Integration Tests for Media Processing
//!
//! Tests the full upload → process → download pipeline using a local `RustFS`
//! instance for S3-compatible storage.
//!
//! Requires: `podman compose -f docker-compose.dev.yml --profile storage up -d`
//!
//! Run with: `cargo test --test integration media_processing -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use http_body_util::BodyExt;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{create_test_user, generate_access_token, TestApp};

/// Create a minimal PNG image in memory (10x10 solid color).
fn create_test_png(width: u32, height: u32) -> Vec<u8> {
    use image::{DynamicImage, ImageFormat};
    let img = DynamicImage::new_rgba8(width, height);
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Build a multipart body for the upload-with-message endpoint.
fn build_upload_multipart(
    filename: &str,
    content_type: &str,
    file_data: &[u8],
    message_content: &str,
) -> (String, Vec<u8>) {
    let boundary = "----TestBoundary12345";
    let mut body = Vec::new();

    // Content field
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\n{message_content}\r\n"
        )
        .as_bytes(),
    );

    // File field
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file_data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    (boundary.to_string(), body)
}

// ============================================================================
// Auth & Error Path Tests (no S3 required)
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_variant_download_requires_auth() {
    let app = TestApp::new().await;
    let attachment_id = Uuid::now_v7();

    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{attachment_id}/download?variant=thumbnail"),
    )
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    // Without S3 configured, endpoint returns 503; with S3 it would return 403
    assert!(
        resp.status() == 403 || resp.status() == 503,
        "Variant download without auth should return 403 or 503 (no S3), got {}",
        resp.status()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_variant_download_not_found() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let mut guard = app.cleanup_guard();
    guard.delete_user(user_id);

    let fake_id = Uuid::now_v7();
    let req = TestApp::request(
        Method::GET,
        &format!("/api/messages/attachments/{fake_id}/download?variant=thumbnail"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    // Without S3 configured, endpoint returns 503; with S3 it would return 403 or 404
    assert!(
        resp.status() == 403 || resp.status() == 404 || resp.status() == 503,
        "Variant download for non-existent attachment should return 403, 404, or 503 (no S3), got {}",
        resp.status()
    );
}

// ============================================================================
// Full Pipeline Tests (S3 required)
// ============================================================================

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_image_upload_generates_metadata() {
    let (app, _bucket) = super::helpers::fresh_test_app_with_s3().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "media-test").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Create a 500x400 PNG (large enough for thumbnail, no medium)
    let png_data = create_test_png(500, 400);
    let (boundary, body) =
        build_upload_multipart("photo.png", "image/png", &png_data, "check this out");

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
    assert_eq!(resp.status(), 201, "Image upload should return 201");

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    // Verify media metadata is present in the attachment
    let attachment = &json["attachments"][0];
    assert_eq!(attachment["width"], 500, "Should have width");
    assert_eq!(attachment["height"], 400, "Should have height");
    assert!(
        attachment["blurhash"].is_string(),
        "Should have blurhash string"
    );
    assert!(
        !attachment["blurhash"].as_str().unwrap().is_empty(),
        "Blurhash should not be empty"
    );
    assert!(
        attachment["thumbnail_url"].is_string(),
        "Should have thumbnail_url for 500px image"
    );
    // 500px is smaller than medium threshold (1024px), so no medium variant
    assert!(
        attachment["medium_url"].is_null(),
        "Should not have medium_url for 500px image"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_non_image_upload_no_metadata() {
    let (app, _bucket) = super::helpers::fresh_test_app_with_s3().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "media-test-txt").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let (boundary, body) =
        build_upload_multipart("readme.txt", "text/plain", b"hello world", "a text file");

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
    assert_eq!(resp.status(), 201, "Text upload should return 201");

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

    let attachment = &json["attachments"][0];
    assert!(
        attachment["blurhash"].is_null(),
        "Text file should not have blurhash"
    );
    assert!(
        attachment["width"].is_null(),
        "Text file should not have width"
    );
    assert!(
        attachment["thumbnail_url"].is_null(),
        "Text file should not have thumbnail_url"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_variant_download_returns_webp() {
    let (app, _bucket) = super::helpers::fresh_test_app_with_s3().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "media-test-dl").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Upload a 500x400 PNG (generates thumbnail)
    let png_data = create_test_png(500, 400);
    let (boundary, body) =
        build_upload_multipart("photo.png", "image/png", &png_data, "download test");

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
    assert_eq!(resp.status(), 201);

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let attachment_id = json["attachments"][0]["id"].as_str().unwrap();

    // Download the thumbnail variant
    let req = TestApp::request(
        Method::GET,
        &format!(
            "/api/messages/attachments/{attachment_id}/download?variant=thumbnail&token={token}"
        ),
    )
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Thumbnail download should return 200");
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "image/webp",
        "Thumbnail should be served as WebP"
    );

    let thumb_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(
        !thumb_bytes.is_empty(),
        "Thumbnail data should not be empty"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_variant_fallback_to_original() {
    let (app, _bucket) = super::helpers::fresh_test_app_with_s3().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "media-test-fb").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    // Upload a tiny 50x50 PNG (too small for any variants)
    let png_data = create_test_png(50, 50);
    let (boundary, body) = build_upload_multipart("tiny.png", "image/png", &png_data, "tiny image");

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
    assert_eq!(resp.status(), 201);

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let attachment_id = json["attachments"][0]["id"].as_str().unwrap();

    // No thumbnail was generated, so variant=thumbnail should fall back to original
    assert!(
        json["attachments"][0]["thumbnail_url"].is_null(),
        "50px image should not have thumbnail_url"
    );

    // Requesting variant=thumbnail on an attachment without one falls back to original PNG
    let req = TestApp::request(
        Method::GET,
        &format!(
            "/api/messages/attachments/{attachment_id}/download?variant=thumbnail&token={token}"
        ),
    )
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200, "Fallback download should return 200");
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "image/png",
        "Fallback should serve original PNG content type"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_invalid_variant_returns_validation_error() {
    let (app, _bucket) = super::helpers::fresh_test_app_with_s3().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id =
        super::helpers::create_channel(&app.pool, guild_id, "media-test-invalid-variant").await;

    let mut guard = app.cleanup_guard();
    guard.add(move |pool| async move { super::helpers::delete_guild(&pool, guild_id).await });
    guard.delete_user(user_id);

    let png_data = create_test_png(500, 400);
    let (boundary, body) =
        build_upload_multipart("photo.png", "image/png", &png_data, "variant validation");

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
    assert_eq!(resp.status(), 201);

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let attachment_id = json["attachments"][0]["id"].as_str().unwrap();

    let req = TestApp::request(
        Method::GET,
        &format!(
            "/api/messages/attachments/{attachment_id}/download?variant=original&token={token}"
        ),
    )
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(error["error"], "VALIDATION_ERROR");
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("Invalid variant"),
        "Unexpected validation message: {}",
        error
    );
}
