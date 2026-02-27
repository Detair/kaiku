//! HTTP Integration Tests for Search Endpoints
//!
//! Tests guild and DM message search at the HTTP layer using `tower::ServiceExt::oneshot`.
//! Each test creates its own users and cleans up via `delete_user` (CASCADE).
//!
//! Run with: `cargo test --test integration search_http`

use axum::body::Body;
use axum::http::Method;
use super::helpers::{
    add_guild_member, body_to_json, create_channel, create_dm_channel, create_guild,
    create_guild_with_default_role, create_test_user, delete_dm_channel, delete_guild, delete_user,
    generate_access_token, insert_attachment, insert_encrypted_message, insert_message,
    insert_message_at, TestApp,
};
use uuid::Uuid;
use vc_server::permissions::GuildPermissions;

// ============================================================================
// Local request helpers
// ============================================================================

/// Helper: guild search GET request.
fn guild_search_request(
    guild_id: Uuid,
    query_string: &str,
    token: &str,
) -> axum::http::Request<Body> {
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
// Guild Search — Non-member Forbidden
// ============================================================================

#[tokio::test]
async fn test_guild_search_non_member_forbidden() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (outsider_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, owner_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    insert_message(&app.pool, channel_id, owner_id, "Secret test message").await;

    // Outsider (not a member) tries to search — should get 403
    let token = generate_access_token(&app.config, outsider_id);
    let req = guild_search_request(guild_id, "q=test", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 403);

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, owner_id).await;
    delete_user(&app.pool, outsider_id).await;
}

// ============================================================================
// Guild Search — Nonexistent Guild
// ============================================================================

#[tokio::test]
async fn test_guild_search_nonexistent_guild() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let fake_guild_id = Uuid::now_v7();
    let req = guild_search_request(fake_guild_id, "q=test", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 404);

    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Basic
// ============================================================================

#[tokio::test]
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
async fn test_guild_search_date_filter() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message_at(
        &app.pool,
        channel_id,
        user_id,
        "Old test message",
        "2024-01-15T12:00:00Z",
    )
    .await;
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
async fn test_guild_search_author_filter() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_a).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    add_guild_member(&app.pool, guild_id, user_b).await;
    let token = generate_access_token(&app.config, user_a);

    insert_message(&app.pool, channel_id, user_a, "Test from user A").await;
    insert_message(&app.pool, channel_id, user_b, "Test from user B").await;

    // Filter by user_b
    let req = guild_search_request(guild_id, &format!("q=test&author_id={user_b}"), &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);
    assert_eq!(json["results"][0]["author"]["id"], user_b.to_string());

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Guild Search — Channel Filter
// ============================================================================

#[tokio::test]
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
    let req = guild_search_request(guild_id, &format!("q=test&channel_id={ch_a}"), &token);
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
    assert_eq!(
        json["total"], 1,
        "Only the message with a link should match"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Has File
// ============================================================================

#[tokio::test]
async fn test_guild_search_has_file() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    let msg_with_file =
        insert_message(&app.pool, channel_id, user_id, "Test with attachment").await;
    insert_attachment(&app.pool, msg_with_file).await;
    insert_message(&app.pool, channel_id, user_id, "Test without attachment").await;

    let req = guild_search_request(guild_id, "q=test&has=file", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "Only the message with attachment should match"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Validation Errors (data-driven)
// ============================================================================

#[tokio::test]
async fn test_guild_search_validation() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    let cases = [
        ("q=test&has=invalid", "Invalid has"),
        ("q=test&sort=invalid", "Invalid sort"),
        (
            "q=test&date_from=2025-12-01T00:00:00Z&date_to=2025-01-01T00:00:00Z",
            "Invalid date range (from > to)",
        ),
    ];

    for (qs, label) in cases {
        let req = guild_search_request(guild_id, qs, &token);
        let resp = app.oneshot(req).await;
        assert_eq!(resp.status(), 400, "{label} should return 400");
    }

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// DM Search — Auth
// ============================================================================

#[tokio::test]
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

    delete_dm_channel(&app.pool, dm_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// DM Search — Only Own DMs
// ============================================================================

#[tokio::test]
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
    assert_eq!(
        json["total"], 1,
        "User A should only see their own DM results"
    );
    assert_eq!(json["results"][0]["channel_id"], dm_ab.to_string());

    delete_dm_channel(&app.pool, dm_ab).await;
    delete_dm_channel(&app.pool, dm_bc).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
    delete_user(&app.pool, user_c).await;
}

// ============================================================================
// DM Search — Channel Filter
// ============================================================================

#[tokio::test]
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

    delete_dm_channel(&app.pool, dm_ab).await;
    delete_dm_channel(&app.pool, dm_ac).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
    delete_user(&app.pool, user_c).await;
}

// ============================================================================
// DM Search — Excludes Encrypted
// ============================================================================

#[tokio::test]
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
    assert_eq!(
        json["total"], 1,
        "Encrypted DM should be excluded from search"
    );

    delete_dm_channel(&app.pool, dm_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Guild Search — Headline contains <mark>
// ============================================================================

#[tokio::test]
async fn test_guild_search_headline_contains_mark() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "The cranberry harvest was excellent this year",
    )
    .await;

    let req = guild_search_request(guild_id, "q=cranberry", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);

    let headline = json["results"][0]["headline"].as_str().unwrap();
    assert!(
        headline.contains("<mark>"),
        "headline should contain <mark> tags, got: {headline}"
    );
    assert!(
        headline.contains("</mark>"),
        "headline should contain </mark> tags, got: {headline}"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Rank present
// ============================================================================

#[tokio::test]
async fn test_guild_search_rank_present() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Pomegranate juice is refreshing",
    )
    .await;

    let req = guild_search_request(guild_id, "q=pomegranate", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);

    let rank = json["results"][0]["rank"].as_f64();
    assert!(rank.is_some(), "rank field should be present");
    assert!(
        rank.unwrap() > 0.0,
        "rank should be a positive float, got: {rank:?}"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Sort by relevance
// ============================================================================

#[tokio::test]
async fn test_guild_search_sort_relevance() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, channel_id, user_id, "Guava fruit salad recipe").await;
    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Guava guava guava is my favorite fruit",
    )
    .await;

    let req = guild_search_request(guild_id, "q=guava&sort=relevance", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2);

    // Both results should have rank field
    let results = json["results"].as_array().unwrap();
    for result in results {
        assert!(
            result["rank"].is_f64(),
            "rank should be present with sort=relevance"
        );
    }

    // First result should have higher or equal rank (sorted by relevance descending)
    let rank_0 = results[0]["rank"].as_f64().unwrap();
    let rank_1 = results[1]["rank"].as_f64().unwrap();
    assert!(
        rank_0 >= rank_1,
        "Results should be sorted by rank descending: {rank_0} >= {rank_1}"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Guild Search — Sort by date
// ============================================================================

#[tokio::test]
async fn test_guild_search_sort_date() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message_at(
        &app.pool,
        channel_id,
        user_id,
        "Papaya older message",
        "2024-06-01T12:00:00Z",
    )
    .await;
    insert_message(&app.pool, channel_id, user_id, "Papaya newer message").await;

    let req = guild_search_request(guild_id, "q=papaya&sort=date", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2);

    let results = json["results"].as_array().unwrap();
    let date_0 = results[0]["created_at"].as_str().unwrap();
    let date_1 = results[1]["created_at"].as_str().unwrap();
    assert!(
        date_0 > date_1,
        "sort=date should return newest first: {date_0} > {date_1}"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// TD-08: Search Security Tests — Special Characters / Injection
// ============================================================================

/// Special characters that should not cause errors or SQL injection.
/// `websearch_to_tsquery` uses parameterized queries so special SQL characters
/// are never interpolated into the query string.
#[tokio::test]
async fn test_guild_search_special_characters_no_error() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    // None of these should crash or return 500.
    // websearch_to_tsquery handles special characters gracefully via parameterized queries.
    let cases = [
        // URL-encoded special chars (axum decodes them before the handler sees them)
        ("q=%40%23%24%25%5E%26%2A%28%29", "@#$%^&*()"),
        (
            "q=%27%3B+DROP+TABLE+messages+--",
            "'; DROP TABLE messages --",
        ),
        (
            "q=%3Cscript%3Ealert%281%29%3C%2Fscript%3E",
            "<script>alert(1)</script>",
        ),
        ("q=hello+world", "normal websearch AND"),
    ];

    for (qs, label) in cases {
        let req = guild_search_request(guild_id, qs, &token);
        let resp = app.oneshot(req).await;
        let status = resp.status();
        // Acceptable outcomes: 200 (handled gracefully) or 400 (validation rejected it).
        // A 500 means the handler failed to guard against the input.
        assert!(
            status.is_success() || status == 400,
            "'{label}' returned unexpected status {status} — expected 200 or 400"
        );
    }

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

/// A query longer than 1000 characters must be rejected with 400.
#[tokio::test]
async fn test_guild_search_long_query_rejected() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    // 1001 'a' characters — just over the 1000-char limit.
    let long_query = "a".repeat(1001);
    let qs = format!("q={long_query}");
    let req = guild_search_request(guild_id, &qs, &token);
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        400,
        "query of 1001 chars should be rejected with 400"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

/// A query of exactly 1000 characters must be accepted (boundary condition).
#[tokio::test]
async fn test_guild_search_max_length_query_accepted() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    // Exactly 1000 'a' characters — at the limit, must be accepted.
    let max_query = "a".repeat(1000);
    let qs = format!("q={max_query}");
    let req = guild_search_request(guild_id, &qs, &token);
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        200,
        "query of exactly 1000 chars should be accepted"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

/// Messages containing HTML/XSS payloads are returned verbatim in the `content`
/// field. The server must not alter the stored content; the client is responsible
/// for HTML-escaping on render.
#[tokio::test]
async fn test_guild_search_xss_content_returned_verbatim() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    // Insert a message that contains an XSS payload.
    let xss_content = "check <img src=x onerror=alert(1)> this xssattack";
    insert_message(&app.pool, channel_id, user_id, xss_content).await;

    let req = guild_search_request(guild_id, "q=xssattack", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1, "should find the XSS-containing message");

    // The raw content field must be returned as stored.
    let content = json["results"][0]["content"].as_str().unwrap();
    assert!(
        content.contains("onerror"),
        "raw content should contain the stored XSS payload unmodified"
    );
    // The server must not HTML-escape the content field.
    assert!(
        !content.contains("&lt;"),
        "content field must not be HTML-escaped by the server; clients escape on render"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// TD-08: Search Security Tests — Large Result Sets / Pagination
// ============================================================================

/// Insert 210 matching messages and verify that pagination returns correct
/// non-overlapping pages and that the total count is accurate.
#[tokio::test]
async fn test_guild_search_large_result_set_pagination() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    // Insert 210 messages that all contain the rare word "paginationterm".
    let n: i64 = 210;
    for i in 0..n {
        insert_message(
            &app.pool,
            channel_id,
            user_id,
            &format!("paginationterm message number {i}"),
        )
        .await;
    }

    // Page 1: limit=100, offset=0
    let req = guild_search_request(guild_id, "q=paginationterm&limit=100&offset=0", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json1 = body_to_json(resp).await;
    assert_eq!(
        json1["total"], n,
        "total should reflect all matching messages"
    );
    assert_eq!(
        json1["results"].as_array().unwrap().len(),
        100,
        "first page should have 100 results"
    );

    // Page 2: limit=100, offset=100
    let req = guild_search_request(guild_id, "q=paginationterm&limit=100&offset=100", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json2 = body_to_json(resp).await;
    assert_eq!(json2["total"], n, "total should be consistent across pages");
    assert_eq!(
        json2["results"].as_array().unwrap().len(),
        100,
        "second page should have 100 results"
    );

    // Page 3: limit=100, offset=200 — only 10 remain
    let req = guild_search_request(guild_id, "q=paginationterm&limit=100&offset=200", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);
    let json3 = body_to_json(resp).await;
    assert_eq!(
        json3["results"].as_array().unwrap().len(),
        10,
        "third page should have the remaining 10 results"
    );

    // Verify no overlap between page 1 and page 2 by comparing message IDs.
    let ids1: std::collections::HashSet<String> = json1["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    let ids2: std::collections::HashSet<String> = json2["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        ids1.is_disjoint(&ids2),
        "page 1 and page 2 must not share any message IDs"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// TD-02: Search — Channel Permission Filtering Integration Tests
// ============================================================================

// VIEW_CHANNEL bit = 1 << 24 (matches GuildPermissions::VIEW_CHANNEL in guild.rs)
const VIEW_CHANNEL_BIT: i64 = 1 << 24;

/// Create a guild role with arbitrary permissions.
async fn create_role_with_perms(
    pool: &sqlx::PgPool,
    guild_id: Uuid,
    name: &str,
    permissions: i64,
) -> Uuid {
    let role_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
         VALUES ($1, $2, $3, $4, 500, false)",
    )
    .bind(role_id)
    .bind(guild_id)
    .bind(name)
    .bind(permissions)
    .execute(pool)
    .await
    .expect("Failed to create role");
    role_id
}

/// Assign a role to a guild member.
async fn assign_role_to_member(pool: &sqlx::PgPool, guild_id: Uuid, user_id: Uuid, role_id: Uuid) {
    sqlx::query("INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)")
        .bind(guild_id)
        .bind(user_id)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("Failed to assign role");
}

/// Create a channel permission override.
async fn create_channel_perm_override(
    pool: &sqlx::PgPool,
    channel_id: Uuid,
    role_id: Uuid,
    allow: i64,
    deny: i64,
) {
    let override_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO channel_overrides
         (id, channel_id, role_id, allow_permissions, deny_permissions)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(override_id)
    .bind(channel_id)
    .bind(role_id)
    .bind(allow)
    .bind(deny)
    .execute(pool)
    .await
    .expect("Failed to create channel override");
}

/// Guild search must not return messages from channels the user cannot VIEW.
///
/// Setup:
/// - Guild with @everyone granting `VIEW_CHANNEL` at guild level
/// - `restricted` channel has a deny-VIEW_CHANNEL override for @everyone
/// - Both channels have a matching message
///
/// Expected: search returns only the message from the visible channel.
#[tokio::test]
async fn test_guild_search_excludes_hidden_channels() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, member_id);

    // @everyone has VIEW_CHANNEL.
    let guild_id =
        create_guild_with_default_role(&app.pool, owner_id, GuildPermissions::VIEW_CHANNEL).await;
    add_guild_member(&app.pool, guild_id, member_id).await;

    let visible_ch = create_channel(&app.pool, guild_id, "visible-ch").await;
    let restricted_ch = create_channel(&app.pool, guild_id, "restricted-ch").await;

    // Get the @everyone role ID.
    let (everyone_role_id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default = true")
            .bind(guild_id)
            .fetch_one(&app.pool)
            .await
            .expect("Failed to fetch @everyone role");

    // Deny VIEW_CHANNEL on `restricted_ch` for @everyone.
    create_channel_perm_override(
        &app.pool,
        restricted_ch,
        everyone_role_id,
        0,
        VIEW_CHANNEL_BIT,
    )
    .await;

    // One matching message in each channel.
    insert_message(
        &app.pool,
        visible_ch,
        owner_id,
        "unique_perm_filter_term in visible",
    )
    .await;
    insert_message(
        &app.pool,
        restricted_ch,
        owner_id,
        "unique_perm_filter_term in restricted",
    )
    .await;

    let req = guild_search_request(guild_id, "q=unique_perm_filter_term", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "member must only see the visible channel message; got: {json}"
    );
    assert_eq!(
        json["results"][0]["channel_id"].as_str().unwrap(),
        visible_ch.to_string(),
        "result must be from the visible channel, not the restricted one"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, owner_id).await;
    delete_user(&app.pool, member_id).await;
}

/// Guild owner bypasses channel permission overrides and can find messages in all channels.
#[tokio::test]
async fn test_guild_search_owner_sees_all_channels() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, owner_id);

    // @everyone has NO permissions.
    let guild_id =
        create_guild_with_default_role(&app.pool, owner_id, GuildPermissions::empty()).await;

    let secret_ch = create_channel(&app.pool, guild_id, "owner-only-ch").await;

    // Deny VIEW_CHANNEL on `secret_ch` for @everyone.
    let (everyone_role_id,): (Uuid,) =
        sqlx::query_as("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default = true")
            .bind(guild_id)
            .fetch_one(&app.pool)
            .await
            .expect("Failed to fetch @everyone role");
    create_channel_perm_override(&app.pool, secret_ch, everyone_role_id, 0, VIEW_CHANNEL_BIT).await;

    insert_message(&app.pool, secret_ch, owner_id, "owner_bypass_secret_term").await;

    // Owner searches — must see the message despite the deny override.
    let req = guild_search_request(guild_id, "q=owner_bypass_secret_term", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "guild owner must bypass channel permission overrides"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, owner_id).await;
}

/// A channel-level allow override for `VIEW_CHANNEL` grants search access
/// even when @everyone has no `VIEW_CHANNEL` at guild level.
#[tokio::test]
async fn test_guild_search_channel_allow_override_grants_access() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, member_id);

    // @everyone has NO VIEW_CHANNEL.
    let guild_id =
        create_guild_with_default_role(&app.pool, owner_id, GuildPermissions::empty()).await;
    add_guild_member(&app.pool, guild_id, member_id).await;

    let special_ch = create_channel(&app.pool, guild_id, "special-allow-ch").await;
    let invisible_ch = create_channel(&app.pool, guild_id, "invisible-ch").await;

    // Create a role with a channel-level allow override for VIEW_CHANNEL.
    let special_role = create_role_with_perms(&app.pool, guild_id, "SpecialAccess", 0).await;
    assign_role_to_member(&app.pool, guild_id, member_id, special_role).await;
    create_channel_perm_override(&app.pool, special_ch, special_role, VIEW_CHANNEL_BIT, 0).await;

    insert_message(
        &app.pool,
        special_ch,
        owner_id,
        "allow_override_unique_term",
    )
    .await;
    insert_message(
        &app.pool,
        invisible_ch,
        owner_id,
        "allow_override_unique_term",
    )
    .await;

    let req = guild_search_request(guild_id, "q=allow_override_unique_term", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "member should only see the channel where VIEW_CHANNEL was explicitly allowed"
    );
    assert_eq!(
        json["results"][0]["channel_id"].as_str().unwrap(),
        special_ch.to_string()
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, owner_id).await;
    delete_user(&app.pool, member_id).await;
}

/// When the user provides a `channel_id` filter for a channel they cannot view,
/// the search must return 0 results and must not leak information.
#[tokio::test]
async fn test_guild_search_channel_filter_respects_visibility() {
    let app = TestApp::new().await;
    let (owner_id, _) = create_test_user(&app.pool).await;
    let (member_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, member_id);

    // @everyone has NO VIEW_CHANNEL — member cannot see any channel.
    let guild_id =
        create_guild_with_default_role(&app.pool, owner_id, GuildPermissions::empty()).await;
    add_guild_member(&app.pool, guild_id, member_id).await;

    let hidden_ch = create_channel(&app.pool, guild_id, "hidden-filter-ch").await;
    insert_message(
        &app.pool,
        hidden_ch,
        owner_id,
        "channel_filter_visibility_term",
    )
    .await;

    // Member explicitly filters to the hidden channel — must get 0 results.
    let qs = format!("q=channel_filter_visibility_term&channel_id={hidden_ch}");
    let req = guild_search_request(guild_id, &qs, &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 0,
        "filtering to a hidden channel must return 0 results to avoid info leakage"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, owner_id).await;
    delete_user(&app.pool, member_id).await;
}

// ============================================================================
// TD-08: DM Search — Long Query Rejection
// ============================================================================

/// DM search with a query longer than 1000 characters must return 400.
#[tokio::test]
async fn test_dm_search_long_query_rejected() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let dm_id = create_dm_channel(&app.pool, user_a, user_b).await;
    let token = generate_access_token(&app.config, user_a);

    let long_query = "a".repeat(1001);
    let qs = format!("q={long_query}");
    let req = dm_search_request(&qs, &token);
    let resp = app.oneshot(req).await;
    assert_eq!(
        resp.status(),
        400,
        "DM search query of 1001 chars should be rejected with 400"
    );

    delete_dm_channel(&app.pool, dm_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}
