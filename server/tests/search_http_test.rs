//! HTTP Integration Tests for Search Endpoints
//!
//! Tests guild and DM message search at the HTTP layer using `tower::ServiceExt::oneshot`.
//! Each test creates its own users and cleans up via `delete_user` (CASCADE).
//!
//! Run with: `cargo test --test search_http_test`

mod helpers;

use axum::body::Body;
use axum::http::Method;
use helpers::{
    body_to_json, create_dm_channel, create_test_user, delete_user, generate_access_token, TestApp,
};
use serial_test::serial;
use uuid::Uuid;

// ============================================================================
// Local test helpers
// ============================================================================

/// Create a guild with the given owner and return its ID.
async fn create_guild(pool: &sqlx::PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::now_v7();
    let name = format!("SearchTestGuild_{}", &guild_id.to_string()[..8]);

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

/// Create a text channel in a guild and return its ID.
async fn create_channel(pool: &sqlx::PgPool, guild_id: Uuid, name: &str) -> Uuid {
    let channel_id = Uuid::now_v7();

    sqlx::query("INSERT INTO channels (id, guild_id, name, channel_type) VALUES ($1, $2, $3, 'text')")
        .bind(channel_id)
        .bind(guild_id)
        .bind(name)
        .execute(pool)
        .await
        .expect("Failed to create channel");

    channel_id
}

/// Insert a message and return its ID.
async fn insert_message(
    pool: &sqlx::PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
) -> Uuid {
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
async fn insert_encrypted_message(
    pool: &sqlx::PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
) -> Uuid {
    let msg_id = Uuid::now_v7();

    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, encrypted) VALUES ($1, $2, $3, $4, true)",
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
async fn insert_attachment(pool: &sqlx::PgPool, message_id: Uuid) {
    sqlx::query(
        "INSERT INTO file_attachments (message_id, filename, mime_type, size_bytes, s3_key) VALUES ($1, 'test.png', 'image/png', 1024, 'uploads/test.png')",
    )
    .bind(message_id)
    .execute(pool)
    .await
    .expect("Failed to insert attachment");
}

/// Delete a guild (cascades channels, messages, members).
async fn delete_guild(pool: &sqlx::PgPool, guild_id: Uuid) {
    // Delete channels (cascades messages and attachments)
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

/// Helper: guild search GET request.
fn guild_search_request(guild_id: Uuid, query_string: &str, token: &str) -> axum::http::Request<Body> {
    TestApp::request(
        Method::GET,
        &format!("/api/guilds/{guild_id}/search?{query_string}"),
    )
    .header("Authorization", format!("Bearer {token}"))
    .body(Body::empty())
    .unwrap()
}

/// Helper: DM search GET request.
fn dm_search_request(query_string: &str, token: &str) -> axum::http::Request<Body> {
    TestApp::request(Method::GET, &format!("/api/dm/search?{query_string}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

// ============================================================================
// Guild Search — Auth
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_requires_auth() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;

    let req = TestApp::request(
        Method::GET,
        &format!("/api/guilds/{guild_id}/search?q=test"),
    )
    .body(Body::empty())
    .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Basic
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_basic() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, channel_id, user_id, "Hello world testing search").await;
    insert_message(&app.pool, channel_id, user_id, "This is a test message").await;
    insert_message(&app.pool, channel_id, user_id, "Unrelated content here").await;

    let req = guild_search_request(guild_id, "q=test", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2);
    assert_eq!(json["results"].as_array().unwrap().len(), 2);

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Excludes Encrypted
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_excludes_encrypted() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, channel_id, user_id, "Visible test message").await;
    insert_encrypted_message(&app.pool, channel_id, user_id, "Encrypted test message").await;

    let req = guild_search_request(guild_id, "q=test", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "Encrypted message should be excluded");

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Date Filter
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_date_filter() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    // Insert a message with an old timestamp
    let old_msg = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO messages (id, channel_id, user_id, content, created_at) VALUES ($1, $2, $3, $4, '2024-01-15T12:00:00Z')",
    )
    .bind(old_msg)
    .bind(channel_id)
    .bind(user_id)
    .bind("Old test message")
    .execute(&app.pool)
    .await
    .expect("insert old message");

    // Insert a recent message
    insert_message(&app.pool, channel_id, user_id, "Recent test message").await;

    // Search with date_from after old message
    let req = guild_search_request(guild_id, "q=test&date_from=2025-01-01T00:00:00Z", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "Only the recent message should match");

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Author Filter
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_author_filter() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_a).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    // Add user_b as guild member
    sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
        .bind(guild_id)
        .bind(user_b)
        .execute(&app.pool)
        .await
        .expect("add member");

    let token = generate_access_token(&app.config, user_a);

    insert_message(&app.pool, channel_id, user_a, "Test from user A").await;
    insert_message(&app.pool, channel_id, user_b, "Test from user B").await;

    // Filter by user_b
    let req = guild_search_request(
        guild_id,
        &format!("q=test&author_id={user_b}"),
        &token,
    );
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);
    assert_eq!(
        json["results"][0]["author"]["id"],
        user_b.to_string()
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Guild Search — Channel Filter
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_channel_filter() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let ch_a = create_channel(&app.pool, guild_id, "channel-a").await;
    let ch_b = create_channel(&app.pool, guild_id, "channel-b").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, ch_a, user_id, "Test in channel A").await;
    insert_message(&app.pool, ch_b, user_id, "Test in channel B").await;

    // Filter to channel_a only
    let req = guild_search_request(
        guild_id,
        &format!("q=test&channel_id={ch_a}"),
        &token,
    );
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"][0]["channel_id"], ch_a.to_string());

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Has Link
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_has_link() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Check this test link https://example.com",
    )
    .await;
    insert_message(&app.pool, channel_id, user_id, "Test without link").await;

    let req = guild_search_request(guild_id, "q=test&has=link", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "Only the message with a link should match");

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Has File
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_has_file() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    let msg_with_file = insert_message(&app.pool, channel_id, user_id, "Test with attachment").await;
    insert_attachment(&app.pool, msg_with_file).await;
    insert_message(&app.pool, channel_id, user_id, "Test without attachment").await;

    let req = guild_search_request(guild_id, "q=test&has=file", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "Only the message with attachment should match");

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Validation Errors
// ============================================================================

#[tokio::test]
#[serial]
async fn test_guild_search_invalid_date_range() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    let req = guild_search_request(
        guild_id,
        "q=test&date_from=2025-12-01T00:00:00Z&date_to=2025-01-01T00:00:00Z",
        &token,
    );
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

#[tokio::test]
#[serial]
async fn test_guild_search_invalid_has() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    let req = guild_search_request(guild_id, "q=test&has=invalid", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 400);

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// DM Search — Auth
// ============================================================================

#[tokio::test]
#[serial]
async fn test_dm_search_requires_auth() {
    let app = TestApp::new().await;

    let req = TestApp::request(Method::GET, "/api/dm/search?q=test")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);
}

// ============================================================================
// DM Search — Basic
// ============================================================================

#[tokio::test]
#[serial]
async fn test_dm_search_basic() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let dm_id = create_dm_channel(&app.pool, user_a, user_b).await;
    let token = generate_access_token(&app.config, user_a);

    insert_message(&app.pool, dm_id, user_a, "Hello test in DM").await;
    insert_message(&app.pool, dm_id, user_b, "Another test reply").await;
    insert_message(&app.pool, dm_id, user_a, "Unrelated content").await;

    let req = dm_search_request("q=test", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2);
    assert_eq!(json["results"].as_array().unwrap().len(), 2);

    // Cleanup: delete DM channel (cascades messages)
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(dm_id)
        .execute(&app.pool)
        .await
        .ok();
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// DM Search — Only Own DMs
// ============================================================================

#[tokio::test]
#[serial]
async fn test_dm_search_only_own_dms() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let (user_c, _) = create_test_user(&app.pool).await;

    // DM between A and B
    let dm_ab = create_dm_channel(&app.pool, user_a, user_b).await;
    insert_message(&app.pool, dm_ab, user_a, "Test private message AB").await;

    // DM between B and C (user_a has no access)
    let dm_bc = create_dm_channel(&app.pool, user_b, user_c).await;
    insert_message(&app.pool, dm_bc, user_b, "Test private message BC").await;

    // User A searches — should only see their own DMs
    let token_a = generate_access_token(&app.config, user_a);
    let req = dm_search_request("q=test", &token_a);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "User A should only see their own DM results");
    assert_eq!(json["results"][0]["channel_id"], dm_ab.to_string());

    // Cleanup
    sqlx::query("DELETE FROM channels WHERE id = ANY($1)")
        .bind(&vec![dm_ab, dm_bc])
        .execute(&app.pool)
        .await
        .ok();
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
    delete_user(&app.pool, user_c).await;
}

// ============================================================================
// DM Search — Channel Filter
// ============================================================================

#[tokio::test]
#[serial]
async fn test_dm_search_channel_filter() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let (user_c, _) = create_test_user(&app.pool).await;

    let dm_ab = create_dm_channel(&app.pool, user_a, user_b).await;
    let dm_ac = create_dm_channel(&app.pool, user_a, user_c).await;

    insert_message(&app.pool, dm_ab, user_a, "Test in DM with B").await;
    insert_message(&app.pool, dm_ac, user_a, "Test in DM with C").await;

    let token = generate_access_token(&app.config, user_a);

    // Filter to dm_ab only
    let req = dm_search_request(&format!("q=test&channel_id={dm_ab}"), &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"][0]["channel_id"], dm_ab.to_string());

    // Cleanup
    sqlx::query("DELETE FROM channels WHERE id = ANY($1)")
        .bind(&vec![dm_ab, dm_ac])
        .execute(&app.pool)
        .await
        .ok();
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
    delete_user(&app.pool, user_c).await;
}

// ============================================================================
// DM Search — Excludes Encrypted
// ============================================================================

#[tokio::test]
#[serial]
async fn test_dm_search_excludes_encrypted() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let dm_id = create_dm_channel(&app.pool, user_a, user_b).await;
    let token = generate_access_token(&app.config, user_a);

    insert_message(&app.pool, dm_id, user_a, "Visible test DM").await;
    insert_encrypted_message(&app.pool, dm_id, user_a, "Encrypted test DM").await;

    let req = dm_search_request("q=test", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "Encrypted DM should be excluded from search");

    // Cleanup
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(dm_id)
        .execute(&app.pool)
        .await
        .ok();
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}
