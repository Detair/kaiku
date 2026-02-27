//! Integration tests for the information pages system.
//!
//! These tests require a running `PostgreSQL` instance.
//! Run with: `cargo test pages --ignored -- --nocapture`

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;
use vc_server::pages::{
    count_pages, hash_content, is_reserved_slug, slug_exists, slugify, CreatePageRequest,
    PageListItem, UpdatePageRequest,
};

/// Helper to create a test database pool.
async fn create_test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/vc_test".into());

    PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Helper to create a unique test slug.
fn test_slug() -> String {
    format!("test-page-{}", &Uuid::new_v4().to_string()[..8])
}

/// Test that content hashing produces consistent results.
#[test]
fn test_hash_content_is_consistent() {
    let content = "# Test Page\n\nThis is test content.";
    let hash1 = hash_content(content);
    let hash2 = hash_content(content);

    assert_eq!(hash1, hash2, "Same content should produce same hash");
    assert_eq!(hash1.len(), 64, "SHA-256 hash should be 64 hex chars");
}

/// Test that different content produces different hashes.
#[test]
fn test_hash_content_differs_for_different_content() {
    let hash1 = hash_content("Content A");
    let hash2 = hash_content("Content B");

    assert_ne!(
        hash1, hash2,
        "Different content should produce different hashes"
    );
}

/// Test slugify produces valid URL-safe slugs.
#[test]
fn test_slugify_basic() {
    assert_eq!(slugify("Hello World"), "hello-world");
    assert_eq!(slugify("  Spaced  Out  "), "spaced-out");
    assert_eq!(slugify("Test!@#$%Page"), "test-page");
    assert_eq!(slugify("UPPERCASE"), "uppercase");
    assert_eq!(slugify("already-slugged"), "already-slugged");
}

/// Test slugify handles edge cases.
#[test]
fn test_slugify_edge_cases() {
    assert_eq!(slugify(""), "");
    assert_eq!(slugify("---"), "");
    assert_eq!(slugify("123"), "123");
    assert_eq!(slugify("a-b-c"), "a-b-c");
    // Unicode letters are stripped to ensure URL-safe ASCII slugs
    assert_eq!(slugify("Über Café"), "ber-caf");
}

/// Test reserved slug detection.
#[test]
fn test_is_reserved_slug() {
    // Reserved slugs
    assert!(is_reserved_slug("admin"), "admin should be reserved");
    assert!(is_reserved_slug("settings"), "settings should be reserved");
    assert!(is_reserved_slug("api"), "api should be reserved");
    assert!(is_reserved_slug("new"), "new should be reserved");

    // Non-reserved slugs
    assert!(
        !is_reserved_slug("my-page"),
        "my-page should not be reserved"
    );
    assert!(
        !is_reserved_slug("welcome"),
        "welcome should not be reserved"
    );
    assert!(!is_reserved_slug("rules"), "rules should not be reserved");
}

/// Test page count for platform pages.
#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_count_platform_pages() {
    let pool = create_test_pool().await;

    let count = count_pages(&pool, None)
        .await
        .expect("Failed to count platform pages");

    assert!(count >= 0, "Count should be non-negative");
    println!("Platform pages count: {count}");
}

/// Test page count for guild pages.
#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_count_guild_pages() {
    let pool = create_test_pool().await;

    // Use a random guild ID (won't exist, should return 0)
    let guild_id = Uuid::new_v4();
    let count = count_pages(&pool, Some(guild_id))
        .await
        .expect("Failed to count guild pages");

    assert_eq!(count, 0, "Non-existent guild should have 0 pages");
    println!("Guild pages count for random guild: {count}");
}

/// Test slug existence check for non-existent slug.
#[tokio::test]
#[ignore] // Requires PostgreSQL
async fn test_slug_not_exists() {
    let pool = create_test_pool().await;
    let slug = test_slug();

    let exists = slug_exists(&pool, None, &slug, None)
        .await
        .expect("Failed to check slug existence");

    assert!(!exists, "Random slug should not exist");
}

/// Test `CreatePageRequest` validation.
#[test]
fn test_create_page_request_structure() {
    let request = CreatePageRequest {
        title: "Test Page".to_string(),
        slug: Some("test-page".to_string()),
        content: "# Content".to_string(),
        requires_acceptance: Some(false),
        category_id: None,
    };

    assert_eq!(request.title, "Test Page");
    assert_eq!(request.slug, Some("test-page".to_string()));
    assert_eq!(request.requires_acceptance, Some(false));
}

/// Test `UpdatePageRequest` validation.
#[test]
fn test_update_page_request_structure() {
    let request = UpdatePageRequest {
        title: Some("Updated Title".to_string()),
        slug: None,
        content: Some("Updated content".to_string()),
        requires_acceptance: Some(true),
        category_id: None,
    };

    assert_eq!(request.title, Some("Updated Title".to_string()));
    assert!(request.slug.is_none());
    assert_eq!(request.requires_acceptance, Some(true));
}

/// Test `PageListItem` structure.
#[test]
fn test_page_list_item_structure() {
    let now = Utc::now();
    let item = PageListItem {
        id: Uuid::new_v4(),
        guild_id: None,
        title: "Platform Page".to_string(),
        slug: "platform-page".to_string(),
        position: 0,
        requires_acceptance: true,
        category_id: None,
        updated_at: now,
    };

    assert!(
        item.guild_id.is_none(),
        "Platform page should have no guild_id"
    );
    assert!(item.requires_acceptance);
    assert_eq!(item.position, 0);
}

/// Test that content hash is 64 characters (SHA-256 hex).
#[test]
fn test_content_hash_length() {
    let hashes = [
        hash_content(""),
        hash_content("short"),
        hash_content(&"x".repeat(10000)),
    ];

    for hash in hashes {
        assert_eq!(
            hash.len(),
            64,
            "All content hashes should be 64 chars (SHA-256 hex)"
        );
    }
}

/// Test empty content produces valid hash.
#[test]
fn test_empty_content_hash() {
    let hash = hash_content("");
    // SHA-256 of empty string is a known value
    assert_eq!(
        hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "Empty string should produce known SHA-256 hash"
    );
}

/// Test slugify strips unicode characters for URL-safe ASCII slugs.
#[test]
fn test_slugify_unicode() {
    // Unicode characters are stripped for URL-safe slugs (matching frontend behavior)
    assert_eq!(slugify("日本語"), "");
    assert_eq!(slugify("Test 日本語 Page"), "test-page");
    assert_eq!(slugify("Café"), "caf");
}

/// Test slugify doesn't produce leading/trailing dashes.
#[test]
fn test_slugify_no_leading_trailing_dashes() {
    let slug = slugify("  Test Page  ");
    assert!(!slug.starts_with('-'), "Slug should not start with dash");
    assert!(!slug.ends_with('-'), "Slug should not end with dash");
}

/// Test that max pages per scope constant is reasonable.
#[test]
fn test_max_pages_constant() {
    use vc_server::pages::DEFAULT_MAX_PAGES_PER_SCOPE;
    let max = DEFAULT_MAX_PAGES_PER_SCOPE;
    assert!(max >= 10, "Should allow at least 10 pages");
    assert!(max <= 1000, "Should not allow more than 1000 pages");
}

/// Test that max content size constant is reasonable.
#[test]
fn test_max_content_size_constant() {
    use vc_server::pages::MAX_CONTENT_SIZE;
    let max = MAX_CONTENT_SIZE;
    assert!(max >= 1024, "Should allow at least 1KB");
    assert!(max <= 1024 * 1024, "Should not allow more than 1MB");
}

/// Test reserved slugs constant contains expected values.
#[test]
fn test_reserved_slugs_constant() {
    use vc_server::pages::RESERVED_SLUGS;
    assert!(RESERVED_SLUGS.contains(&"admin"));
    assert!(RESERVED_SLUGS.contains(&"settings"));
    assert!(RESERVED_SLUGS.contains(&"api"));
}
