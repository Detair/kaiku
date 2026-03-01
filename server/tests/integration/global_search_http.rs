//! HTTP Integration Tests for Global Search Endpoint (`GET /api/search`)
//!
//! Tests the global search endpoint which searches across all guilds and DMs
//! the authenticated user has access to.
//!
//! Run with: `cargo test --test integration global_search_http -- --test-threads=1`

use axum::body::Body;
use axum::http::Method;

use super::helpers::{
    add_guild_member, body_to_json, create_channel, create_dm_channel, create_guild,
    create_test_user, delete_dm_channel, delete_guild, delete_user, generate_access_token,
    insert_attachment, insert_deleted_message, insert_encrypted_message, insert_message,
    insert_message_at, TestApp,
};

// ============================================================================
// Local helpers
// ============================================================================

/// Helper: global search GET request.
fn global_search_request(query_string: &str, token: &str) -> axum::http::Request<Body> {
    TestApp::request(Method::GET, &format!("/api/search?{query_string}"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

// ============================================================================
// Global Search — Auth
// ============================================================================

#[tokio::test]
async fn test_global_search_requires_auth() {
    let app = TestApp::new().await;

    let req = TestApp::request(Method::GET, "/api/search?q=test")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 401);
}

// ============================================================================
// Global Search — Basic
// ============================================================================

#[tokio::test]
async fn test_global_search_basic() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Hello world testing global search",
    )
    .await;
    insert_message(&app.pool, channel_id, user_id, "Unrelated content").await;

    let req = global_search_request("q=global+search", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);

    let results = json["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);

    let result = &results[0];
    // Verify response shape — all expected string fields present and non-empty
    for field in [
        "id",
        "channel_id",
        "channel_name",
        "content",
        "created_at",
        "headline",
    ] {
        assert!(result[field].is_string(), "{field} should be a string");
        assert!(
            !result[field].as_str().unwrap().is_empty(),
            "{field} should not be empty"
        );
    }
    assert!(result["author"].is_object(), "author should be an object");
    assert!(result["rank"].is_f64(), "rank should be a float");
    assert!(result["source"].is_object(), "source should be an object");

    // Verify headline contains mark tags
    let headline = result["headline"].as_str().unwrap();
    assert!(
        headline.contains("<mark>"),
        "headline should contain <mark> tags, got: {headline}"
    );

    // Verify rank is positive
    let rank = result["rank"].as_f64().unwrap();
    assert!(rank > 0.0, "rank should be positive, got: {rank}");

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Multi Guild
// ============================================================================

#[tokio::test]
async fn test_global_search_multi_guild() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let guild_a = create_guild(&app.pool, user_id).await;
    let ch_a = create_channel(&app.pool, guild_a, "chat-a").await;
    insert_message(&app.pool, ch_a, user_id, "Pineapple discussion in guild A").await;

    let guild_b = create_guild(&app.pool, user_id).await;
    let ch_b = create_channel(&app.pool, guild_b, "chat-b").await;
    insert_message(&app.pool, ch_b, user_id, "Pineapple discussion in guild B").await;

    let req = global_search_request("q=pineapple", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2, "Should find results from both guilds");

    let results = json["results"].as_array().unwrap();
    let guild_ids: Vec<&str> = results
        .iter()
        .filter_map(|r| r["source"]["guild_id"].as_str())
        .collect();
    assert!(
        guild_ids.contains(&guild_a.to_string().as_str()),
        "Should include result from guild A"
    );
    assert!(
        guild_ids.contains(&guild_b.to_string().as_str()),
        "Should include result from guild B"
    );

    delete_guild(&app.pool, guild_a).await;
    delete_guild(&app.pool, guild_b).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Includes DMs
// ============================================================================

#[tokio::test]
async fn test_global_search_includes_dms() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_a);

    let guild_id = create_guild(&app.pool, user_a).await;
    let ch = create_channel(&app.pool, guild_id, "general").await;
    insert_message(&app.pool, ch, user_a, "Mango smoothie in guild").await;

    let dm_id = create_dm_channel(&app.pool, user_a, user_b).await;
    insert_message(&app.pool, dm_id, user_a, "Mango smoothie in DM").await;

    let req = global_search_request("q=mango+smoothie", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2, "Should include both guild and DM results");

    let results = json["results"].as_array().unwrap();
    let source_types: Vec<&str> = results
        .iter()
        .filter_map(|r| r["source"]["type"].as_str())
        .collect();
    assert!(
        source_types.contains(&"guild"),
        "Should have a guild source"
    );
    assert!(source_types.contains(&"dm"), "Should have a dm source");

    delete_dm_channel(&app.pool, dm_id).await;
    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Global Search — Source Guild
// ============================================================================

#[tokio::test]
async fn test_global_search_source_guild() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Kumquat source verification",
    )
    .await;

    let req = global_search_request("q=kumquat", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);

    let source = &json["results"][0]["source"];
    assert_eq!(source["type"], "guild");
    assert_eq!(source["guild_id"], guild_id.to_string());
    assert!(
        source["guild_name"].is_string(),
        "guild_name should be present for guild source"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Source DM
// ============================================================================

#[tokio::test]
async fn test_global_search_source_dm() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_a);

    let dm_id = create_dm_channel(&app.pool, user_a, user_b).await;
    insert_message(&app.pool, dm_id, user_a, "Dragonfruit private chat").await;

    let req = global_search_request("q=dragonfruit", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 1);

    let source = &json["results"][0]["source"];
    assert_eq!(source["type"], "dm");
    assert!(
        source["guild_id"].is_null(),
        "guild_id should be null for DM source"
    );
    assert!(
        source["guild_name"].is_null(),
        "guild_name should be null for DM source"
    );

    delete_dm_channel(&app.pool, dm_id).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Global Search — Excludes Inaccessible
// ============================================================================

#[tokio::test]
async fn test_global_search_excludes_inaccessible() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;

    let guild_a = create_guild(&app.pool, user_a).await;
    let ch_a = create_channel(&app.pool, guild_a, "secret").await;
    insert_message(&app.pool, ch_a, user_a, "Starfruit secret message").await;

    let guild_b = create_guild(&app.pool, user_b).await;
    let ch_b = create_channel(&app.pool, guild_b, "public").await;
    insert_message(&app.pool, ch_b, user_b, "Starfruit public message").await;

    let token_b = generate_access_token(&app.config, user_b);
    let req = global_search_request("q=starfruit", &token_b);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "user_b should only see messages from their own guild"
    );
    assert_eq!(
        json["results"][0]["source"]["guild_id"],
        guild_b.to_string()
    );

    delete_guild(&app.pool, guild_a).await;
    delete_guild(&app.pool, guild_b).await;
    delete_user(&app.pool, user_a).await;
    delete_user(&app.pool, user_b).await;
}

// ============================================================================
// Global Search — Excludes Deleted Messages
// ============================================================================

#[tokio::test]
async fn test_global_search_excludes_deleted() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, channel_id, user_id, "Visible rambutan message").await;
    insert_deleted_message(&app.pool, channel_id, user_id, "Deleted rambutan message").await;

    let req = global_search_request("q=rambutan", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "Soft-deleted message should be excluded from global search"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Excludes Encrypted Messages
// ============================================================================

#[tokio::test]
async fn test_global_search_excludes_encrypted() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, channel_id, user_id, "Visible persimmon message").await;
    insert_encrypted_message(
        &app.pool,
        channel_id,
        user_id,
        "Encrypted persimmon message",
    )
    .await;

    let req = global_search_request("q=persimmon", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "Encrypted message should be excluded from global search"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Date Filter
// ============================================================================

#[tokio::test]
async fn test_global_search_date_filter() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message_at(
        &app.pool,
        channel_id,
        user_id,
        "Old tamarind message",
        "2024-01-15T12:00:00Z",
    )
    .await;
    insert_message(&app.pool, channel_id, user_id, "Recent tamarind message").await;

    let req = global_search_request("q=tamarind&date_from=2025-01-01T00:00:00Z", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 1,
        "Only the recent message should match with date_from filter"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Author Filter
// ============================================================================

#[tokio::test]
async fn test_global_search_author_filter() {
    let app = TestApp::new().await;
    let (user_a, _) = create_test_user(&app.pool).await;
    let (user_b, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_a).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;

    add_guild_member(&app.pool, guild_id, user_b).await;
    let token = generate_access_token(&app.config, user_a);

    insert_message(&app.pool, channel_id, user_a, "Jackfruit from user A").await;
    insert_message(&app.pool, channel_id, user_b, "Jackfruit from user B").await;

    let req = global_search_request(&format!("q=jackfruit&author_id={user_b}"), &token);
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
// Global Search — Has Link
// ============================================================================

#[tokio::test]
async fn test_global_search_has_link() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Durian info at https://example.com/durian",
    )
    .await;
    insert_message(&app.pool, channel_id, user_id, "Durian without link").await;

    let req = global_search_request("q=durian&has=link", &token);
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
// Global Search — Has File
// ============================================================================

#[tokio::test]
async fn test_global_search_has_file() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    let msg_with_file =
        insert_message(&app.pool, channel_id, user_id, "Breadfruit with attachment").await;
    insert_attachment(&app.pool, msg_with_file).await;
    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Breadfruit without attachment",
    )
    .await;

    let req = global_search_request("q=breadfruit&has=file", &token);
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
// Global Search — Sort by Relevance
// ============================================================================

#[tokio::test]
async fn test_global_search_sort_relevance() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message(&app.pool, channel_id, user_id, "Coconut fruit salad recipe").await;
    insert_message(
        &app.pool,
        channel_id,
        user_id,
        "Coconut coconut coconut is my favorite fruit",
    )
    .await;

    let req = global_search_request("q=coconut&sort=relevance", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 2);

    let results = json["results"].as_array().unwrap();
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
// Global Search — Sort by Date
// ============================================================================

#[tokio::test]
async fn test_global_search_sort_date() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    insert_message_at(
        &app.pool,
        channel_id,
        user_id,
        "Plantain older message",
        "2024-06-01T12:00:00Z",
    )
    .await;
    insert_message(&app.pool, channel_id, user_id, "Plantain newer message").await;

    let req = global_search_request("q=plantain&sort=date", &token);
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
// Global Search — Validation (data-driven)
// ============================================================================

#[tokio::test]
async fn test_global_search_validation() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let token = generate_access_token(&app.config, user_id);

    let cases = [
        ("q=", "Empty q"),
        ("q=a", "Single-char q"),
        ("q=test&sort=invalid", "Invalid sort"),
        ("q=test&has=invalid", "Invalid has"),
        (
            "q=test&date_from=2025-12-01T00:00:00Z&date_to=2025-01-01T00:00:00Z",
            "Invalid date range (from > to)",
        ),
    ];

    for (qs, label) in cases {
        let req = global_search_request(qs, &token);
        let resp = app.oneshot(req).await;
        assert_eq!(resp.status(), 400, "{label} should return 400");
    }

    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Pagination (with offset verification)
// ============================================================================

#[tokio::test]
async fn test_global_search_pagination() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    for i in 1..=5 {
        insert_message(
            &app.pool,
            channel_id,
            user_id,
            &format!("Lychee pagination message number {i}"),
        )
        .await;
    }

    // Page 1: limit=2, offset=0
    let req = global_search_request("q=lychee&limit=2&offset=0", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(
        json["total"], 5,
        "total should reflect all matching results"
    );
    assert_eq!(json["limit"], 2);
    assert_eq!(json["offset"], 0);
    let page1_results = json["results"].as_array().unwrap();
    assert_eq!(page1_results.len(), 2, "Should return only 2 results");
    let page1_ids: Vec<&str> = page1_results
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();

    // Page 2: limit=2, offset=2 — results must be different from page 1
    let req = global_search_request("q=lychee&limit=2&offset=2", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 5);
    assert_eq!(json["offset"], 2);
    let page2_results = json["results"].as_array().unwrap();
    assert_eq!(
        page2_results.len(),
        2,
        "Should return 2 results at offset 2"
    );
    let page2_ids: Vec<&str> = page2_results
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert!(
        page1_ids.iter().all(|id| !page2_ids.contains(id)),
        "Page 2 results should be different from page 1"
    );

    // Beyond total
    let req = global_search_request("q=lychee&limit=2&offset=10", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["total"], 5);
    assert_eq!(
        json["results"].as_array().unwrap().len(),
        0,
        "Should return empty results when offset exceeds total"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}

// ============================================================================
// Global Search — Limit Clamping
// ============================================================================

#[tokio::test]
async fn test_global_search_limit_clamping() {
    let app = TestApp::new().await;
    let (user_id, _) = create_test_user(&app.pool).await;
    let guild_id = create_guild(&app.pool, user_id).await;
    let channel_id = create_channel(&app.pool, guild_id, "general").await;
    let token = generate_access_token(&app.config, user_id);

    for i in 1..=3 {
        insert_message(
            &app.pool,
            channel_id,
            user_id,
            &format!("Soursop clamping test {i}"),
        )
        .await;
    }

    // limit=200 should be clamped to 100 (returns all 3 since 3 < 100)
    let req = global_search_request("q=soursop&limit=200", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["limit"], 100, "limit=200 should be clamped to 100");
    assert_eq!(json["results"].as_array().unwrap().len(), 3);

    // limit=0 should be clamped to 1
    let req = global_search_request("q=soursop&limit=0", &token);
    let resp = app.oneshot(req).await;
    assert_eq!(resp.status(), 200);

    let json = body_to_json(resp).await;
    assert_eq!(json["limit"], 1, "limit=0 should be clamped to 1");
    assert_eq!(
        json["results"].as_array().unwrap().len(),
        1,
        "Clamped limit=1 should return 1 result"
    );

    delete_guild(&app.pool, guild_id).await;
    delete_user(&app.pool, user_id).await;
}
