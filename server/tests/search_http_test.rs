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
    add_guild_member, body_to_json, create_channel, create_dm_channel, create_guild,
    create_test_user, delete_dm_channel, delete_guild, delete_user, generate_access_token,
    insert_attachment, insert_encrypted_message, insert_message, insert_message_at, TestApp,
};
use serial_test::serial;
use uuid::Uuid;

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
#[serial]
async fn test_guild_search_requires_auth() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_non_member_forbidden() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_nonexistent_guild() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_basic() {
    let app = helpers::fresh_test_app().await;
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
    let app = helpers::fresh_test_app().await;
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
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_author_filter() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_channel_filter() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_has_link() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_has_file() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_validation() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_dm_search_requires_auth() {
    let app = helpers::fresh_test_app().await;

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
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_dm_search_only_own_dms() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_dm_search_channel_filter() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_dm_search_excludes_encrypted() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_headline_contains_mark() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_rank_present() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_sort_relevance() {
    let app = helpers::fresh_test_app().await;
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
#[serial]
async fn test_guild_search_sort_date() {
    let app = helpers::fresh_test_app().await;
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
