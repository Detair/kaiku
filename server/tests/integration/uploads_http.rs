//! HTTP Integration Tests for Upload Error Paths
//!
//! S3 is not configured in test environment (`AppState.s3 = None`),
//! so these tests verify error responses only. Send authorization runs
//! before the storage check, so permission tests need no `RustFS`.
//!
//! Run with: `cargo test --test integration uploads_http -- --nocapture`

use axum::body::Body;
use axum::http::Method;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

use super::helpers::{
    add_guild_member, all_features_config, body_to_json, create_channel, create_dm_channel,
    create_guild_with_default_role, create_test_user, fresh_test_app_with_s3_and_config,
    generate_access_token, rustfs_available, TestApp,
};

/// Build an upload request for `channel_id` with a small text file and
/// optional message `content`.
fn upload_request(token: &str, channel_id: Uuid, content: &str) -> axum::http::Request<Body> {
    let boundary = "----TestBoundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"content\"\r\n\r\n{content}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n--{boundary}--\r\n"
    );
    TestApp::request(
        Method::POST,
        &format!("/api/messages/channel/{channel_id}/upload"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header(
        "Content-Type",
        format!("multipart/form-data; boundary={boundary}"),
    )
    .body(Body::from(body))
    .unwrap()
}

/// Create a guild whose `@everyone` role has `perms`, a channel of
/// `channel_type` in it, and a non-owner member. Returns (channel, member token).
async fn guild_member_channel(
    app: &TestApp,
    perms: GuildPermissions,
    channel_type: &str,
) -> (Uuid, String) {
    let (owner, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild_with_default_role(&app.pool, owner, perms).await;
    let channel_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, 'upload-perm', $3::channel_type)",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(channel_type)
    .execute(&app.pool)
    .await
    .expect("create channel");
    let (member, _) = create_test_user(&app.pool).await;
    add_guild_member(&app.pool, guild_id, member).await;
    (channel_id, generate_access_token(&app.config, member))
}

// ============================================================================
// Upload Error Paths
// ============================================================================

#[sqlx::test]
async fn test_upload_returns_503_without_s3(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = super::helpers::create_guild_with_default_role(&app.pool, user_id, perms).await;
    let channel_id = super::helpers::create_channel(&app.pool, guild_id, "upload-503-test").await;

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

#[sqlx::test]
async fn test_upload_requires_auth(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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

#[sqlx::test]
async fn test_get_attachment_not_found(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

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
}

#[sqlx::test]
async fn test_get_attachment_anti_enumeration_parity(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
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
}

// ============================================================================
// Send authorization (shared with text messages)
// ============================================================================

#[sqlx::test]
async fn test_upload_requires_attach_files(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let (channel_id, token) = guild_member_channel(&app, perms, "text").await;

    let resp = app.oneshot(upload_request(&token, channel_id, "")).await;
    assert_eq!(
        resp.status(),
        403,
        "upload without ATTACH_FILES must be 403"
    );
    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "FORBIDDEN");
}

#[sqlx::test]
async fn test_upload_blocked_in_dm(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token_a = generate_access_token(&app.config, user_a);
    let token_b = generate_access_token(&app.config, user_b);
    let channel_id = create_dm_channel(&app.pool, user_a, user_b).await;

    // A (recipient) blocks B (sender)
    let req = TestApp::request(Method::POST, &format!("/api/friends/{user_b}/block"))
        .header("Authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 200);

    let resp = app.oneshot(upload_request(&token_b, channel_id, "")).await;
    assert_eq!(resp.status(), 403);
    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "BLOCKED");

    // The blocking side (sender blocked the recipient) is rejected as well
    let resp = app.oneshot(upload_request(&token_a, channel_id, "")).await;
    assert_eq!(resp.status(), 403, "sender who blocked the recipient");
    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "BLOCKED");
}

#[sqlx::test]
async fn test_upload_to_announcement_requires_send_announcements(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::SEND_MESSAGES
        | GuildPermissions::ATTACH_FILES;
    let (channel_id, token) = guild_member_channel(&app, perms, "announcement").await;

    let resp = app.oneshot(upload_request(&token, channel_id, "")).await;
    assert_eq!(
        resp.status(),
        403,
        "upload to announcement channel without SEND_ANNOUNCEMENTS must be 403"
    );
}

#[sqlx::test]
async fn test_upload_everyone_mention_requires_permission(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let perms = GuildPermissions::VIEW_CHANNEL
        | GuildPermissions::SEND_MESSAGES
        | GuildPermissions::ATTACH_FILES;
    let (channel_id, token) = guild_member_channel(&app, perms, "text").await;

    let resp = app
        .oneshot(upload_request(&token, channel_id, "@everyone"))
        .await;
    assert_eq!(
        resp.status(),
        400,
        "@everyone in an upload without MENTION_EVERYONE must be 400"
    );
}

#[sqlx::test]
async fn test_upload_content_filter_returns_403(pool: PgPool) {
    let app = TestApp::with_pool(pool.clone()).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = create_guild_with_default_role(&app.pool, owner, perms).await;
    let channel_id = create_channel(&app.pool, guild_id, "upload-filter").await;

    let req = TestApp::request(
        Method::POST,
        &format!("/api/guilds/{guild_id}/filters/patterns"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .header("Content-Type", "application/json")
    .body(Body::from(
        serde_json::json!({ "pattern": "forbidden", "is_regex": false }).to_string(),
    ))
    .unwrap();
    assert_eq!(app.oneshot(req).await.status(), 201);

    let resp = app
        .oneshot(upload_request(&token, channel_id, "this is forbidden"))
        .await;
    assert_eq!(resp.status(), 403, "filtered upload text must be 403");
    let json = body_to_json(resp).await;
    assert_eq!(json["error"], "CONTENT_FILTERED");
}

#[sqlx::test]
async fn test_upload_s3_failure_leaves_no_message(pool: PgPool) {
    let app = TestApp::with_pool_and_unreachable_s3(pool.clone()).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = create_guild_with_default_role(&app.pool, owner, perms).await;
    let channel_id = create_channel(&app.pool, guild_id, "upload-s3-fail").await;

    let resp = app
        .oneshot(upload_request(&token, channel_id, "with text"))
        .await;
    assert!(
        resp.status().is_server_error(),
        "upload with failing storage must be 5xx, got {}",
        resp.status()
    );

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE channel_id = $1")
        .bind(channel_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "a failed upload must not leave a message row");
}

// ============================================================================
// Announcement crosspost (RustFS required)
// ============================================================================

/// A file-only upload to an announcement channel crossposts nothing (the
/// crosspost carries text only); a text+file upload still crossposts its text.
#[sqlx::test]
async fn test_file_only_announcement_upload_is_not_crossposted(pool: PgPool) {
    if !rustfs_available().await {
        eprintln!("SKIP: RustFS unavailable");
        return;
    }
    let (app, _bucket) =
        fresh_test_app_with_s3_and_config(pool.clone(), all_features_config()).await;
    let (owner, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner);
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = create_guild_with_default_role(&app.pool, owner, perms).await;
    let (source, follower) = announcement_with_follower(&app.pool, guild_id).await;

    let resp = app.oneshot(upload_request(&token, source, "")).await;
    assert_eq!(resp.status(), 201, "file-only announcement upload");

    // Control: text+file still crossposts its text. Wait for it to land.
    let resp = app.oneshot(upload_request(&token, source, "news")).await;
    assert_eq!(resp.status(), 201, "text+file announcement upload");
    assert_eq!(
        settled_contents(&app.pool, follower).await,
        vec!["news".to_string()],
        "follower must only get the text crosspost"
    );
}

/// Create an announcement channel followed by one text channel.
/// Returns (source, follower).
async fn announcement_with_follower(pool: &PgPool, guild_id: Uuid) -> (Uuid, Uuid) {
    let source = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, 'news', 'announcement')",
    )
    .bind(source)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("create announcement channel");
    let follower = create_channel(pool, guild_id, "follower").await;
    sqlx::query(
        "INSERT INTO announcement_follows (source_channel_id, target_channel_id) VALUES ($1, $2)",
    )
    .bind(source)
    .bind(follower)
    .execute(pool)
    .await
    .expect("create follow");
    (source, follower)
}

/// Wait until `channel` holds at least one message, then a little longer so a
/// stray crosspost spawned earlier also lands; return all contents.
async fn settled_contents(pool: &PgPool, channel: Uuid) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE channel_id = $1")
            .bind(channel)
            .fetch_one(pool)
            .await
            .unwrap();
        if n > 0 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "text crosspost never arrived"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    sqlx::query_scalar("SELECT content FROM messages WHERE channel_id = $1 ORDER BY id")
        .bind(channel)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// The shared dispatch (used by uploads) does not crosspost an announcement
/// without text; one with text still crossposts. No storage needed.
#[sqlx::test]
async fn test_dispatch_skips_crosspost_without_text(pool: PgPool) {
    use std::sync::Arc;

    use vc_server::api::{AppState, AppStateConfig};
    use vc_server::chat::send::{dispatch_new_message, NewMessageDispatch};
    use vc_server::voice::sfu::SfuServer;

    let config = all_features_config();
    let redis = vc_server::db::create_redis_client(&config.redis_url)
        .await
        .expect("connect test Redis");
    let sfu = SfuServer::new(Arc::new(config.clone()), None).expect("SfuServer");
    let state = AppState::new(AppStateConfig {
        db: pool.clone(),
        redis,
        config,
        s3: None,
        sfu,
        rate_limiter: None,
        screen_share_limiter: None,
        email: None,
        oidc_manager: None,
        http_client: reqwest::Client::new(),
    });

    let (owner, _) = create_test_user(&pool).await;
    let perms = GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES;
    let guild_id = create_guild_with_default_role(&pool, owner, perms).await;
    let (source, follower) = announcement_with_follower(&pool, guild_id).await;
    let channel = vc_server::db::find_channel_by_id(&pool, source)
        .await
        .unwrap()
        .expect("announcement channel");

    for content in ["", "news"] {
        dispatch_new_message(
            &state,
            NewMessageDispatch {
                channel: &channel,
                author_id: owner,
                message_id: Uuid::now_v7(),
                content,
                encrypted: false,
                is_crosspost: false,
                message_json: serde_json::json!({}),
            },
        )
        .await;
    }

    assert_eq!(
        settled_contents(&pool, follower).await,
        vec!["news".to_string()],
        "follower must only get the text crosspost"
    );
}
