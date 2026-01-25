//! Integration tests for the message search system.
//!
//! These tests require a running PostgreSQL instance with the schema applied.
//! Run with: `cargo test search --ignored -- --nocapture`

use sqlx::PgPool;
use uuid::Uuid;

/// Helper to create a test database pool.
async fn create_test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/vc_test".into());

    PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Helper to create a test user and return their ID.
async fn create_test_user(pool: &PgPool) -> Uuid {
    let user_id = Uuid::new_v4();
    let username = format!("testuser_{}", &user_id.to_string()[..8]);

    sqlx::query(
        r#"
        INSERT INTO users (id, username, display_name, password_hash, email)
        VALUES ($1, $2, $3, 'fake_hash', $4)
        "#,
    )
    .bind(user_id)
    .bind(&username)
    .bind(&username)
    .bind(format!("{}@test.com", username))
    .execute(pool)
    .await
    .expect("Failed to create test user");

    user_id
}

/// Helper to create a test guild and return its ID.
async fn create_test_guild(pool: &PgPool, owner_id: Uuid) -> Uuid {
    let guild_id = Uuid::new_v4();
    let guild_name = format!("Test Guild {}", &guild_id.to_string()[..8]);

    sqlx::query(
        r#"
        INSERT INTO guilds (id, name, owner_id)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(guild_id)
    .bind(&guild_name)
    .bind(owner_id)
    .execute(pool)
    .await
    .expect("Failed to create test guild");

    // Add owner as guild member
    sqlx::query(
        r#"
        INSERT INTO guild_members (guild_id, user_id)
        VALUES ($1, $2)
        "#,
    )
    .bind(guild_id)
    .bind(owner_id)
    .execute(pool)
    .await
    .expect("Failed to add guild member");

    guild_id
}

/// Helper to create a test channel and return its ID.
async fn create_test_channel(pool: &PgPool, guild_id: Uuid, name: &str) -> Uuid {
    let channel_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO channels (id, guild_id, name, channel_type)
        VALUES ($1, $2, $3, 'text')
        "#,
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(name)
    .execute(pool)
    .await
    .expect("Failed to create test channel");

    channel_id
}

/// Helper to create a test message and return its ID.
async fn create_test_message(
    pool: &PgPool,
    channel_id: Uuid,
    user_id: Uuid,
    content: &str,
) -> Uuid {
    let message_id = Uuid::new_v4();

    sqlx::query(
        r#"
        INSERT INTO messages (id, channel_id, user_id, content)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(message_id)
    .bind(channel_id)
    .bind(user_id)
    .bind(content)
    .execute(pool)
    .await
    .expect("Failed to create test message");

    message_id
}

/// Helper to cleanup test data.
async fn cleanup_test_data(pool: &PgPool, user_id: Uuid, guild_id: Uuid) {
    // Delete messages first (FK constraint)
    sqlx::query("DELETE FROM messages WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();

    // Delete channels
    sqlx::query("DELETE FROM channels WHERE guild_id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .ok();

    // Delete guild members
    sqlx::query("DELETE FROM guild_members WHERE guild_id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .ok();

    // Delete guild
    sqlx::query("DELETE FROM guilds WHERE id = $1")
        .bind(guild_id)
        .execute(pool)
        .await
        .ok();

    // Delete user
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
}

// ============================================================================
// Search Query Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires database"]
async fn test_search_messages_basic() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general").await;

    // Create messages with different content
    create_test_message(&pool, channel_id, user_id, "Hello world").await;
    create_test_message(&pool, channel_id, user_id, "Testing search functionality").await;
    create_test_message(&pool, channel_id, user_id, "Another test message").await;

    // Search for "test"
    let results: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT m.id, m.content
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ORDER BY m.created_at DESC
        "#,
    )
    .bind(guild_id)
    .bind("test")
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(results.len(), 2, "Expected 2 results for 'test'");

    cleanup_test_data(&pool, user_id, guild_id).await;
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_search_messages_no_results() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general").await;

    // Create messages that won't match
    create_test_message(&pool, channel_id, user_id, "Hello world").await;

    // Search for something that doesn't exist
    let results: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT m.id
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        "#,
    )
    .bind(guild_id)
    .bind("nonexistent")
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(results.len(), 0, "Expected no results for 'nonexistent'");

    cleanup_test_data(&pool, user_id, guild_id).await;
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_search_messages_pagination() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general").await;

    // Create 10 test messages
    for i in 0..10 {
        create_test_message(
            &pool,
            channel_id,
            user_id,
            &format!("Test message number {}", i),
        )
        .await;
    }

    // Get first page (limit 5)
    let page1: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT m.id
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ORDER BY m.created_at DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(guild_id)
    .bind("test")
    .bind(5_i64)
    .bind(0_i64)
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(page1.len(), 5, "Expected 5 results on first page");

    // Get second page
    let page2: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT m.id
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        ORDER BY m.created_at DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(guild_id)
    .bind("test")
    .bind(5_i64)
    .bind(5_i64)
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(page2.len(), 5, "Expected 5 results on second page");

    // Verify no overlap between pages
    let page1_ids: Vec<Uuid> = page1.iter().map(|(id,)| *id).collect();
    let page2_ids: Vec<Uuid> = page2.iter().map(|(id,)| *id).collect();
    for id in &page2_ids {
        assert!(
            !page1_ids.contains(id),
            "Pages should not have overlapping results"
        );
    }

    cleanup_test_data(&pool, user_id, guild_id).await;
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_search_messages_count() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general").await;

    // Create messages
    for i in 0..15 {
        create_test_message(
            &pool,
            channel_id,
            user_id,
            &format!("Test message {}", i),
        )
        .await;
    }
    // Create some non-matching messages
    create_test_message(&pool, channel_id, user_id, "Hello world").await;
    create_test_message(&pool, channel_id, user_id, "Goodbye").await;

    // Count matching messages
    let count: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        "#,
    )
    .bind(guild_id)
    .bind("test")
    .fetch_one(&pool)
    .await
    .expect("Count query failed");

    assert_eq!(count.0, 15, "Expected 15 matching messages");

    cleanup_test_data(&pool, user_id, guild_id).await;
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_search_excludes_deleted_messages() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general").await;

    // Create a message
    let msg_id = create_test_message(&pool, channel_id, user_id, "Test message to delete").await;

    // Soft-delete the message
    sqlx::query("UPDATE messages SET deleted_at = NOW() WHERE id = $1")
        .bind(msg_id)
        .execute(&pool)
        .await
        .expect("Failed to soft-delete message");

    // Create another non-deleted message
    create_test_message(&pool, channel_id, user_id, "Test message visible").await;

    // Search should only find the non-deleted message
    let results: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT m.id
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        "#,
    )
    .bind(guild_id)
    .bind("test")
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(results.len(), 1, "Expected 1 result (deleted message excluded)");
    assert_ne!(results[0].0, msg_id, "Deleted message should not be in results");

    cleanup_test_data(&pool, user_id, guild_id).await;
}

#[tokio::test]
#[ignore = "requires database"]
async fn test_search_websearch_syntax() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general").await;

    // Create test messages
    create_test_message(&pool, channel_id, user_id, "The quick brown fox").await;
    create_test_message(&pool, channel_id, user_id, "A lazy brown dog").await;
    create_test_message(&pool, channel_id, user_id, "The quick red fox").await;

    // Search with AND (implicit in websearch_to_tsquery)
    let and_results: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT m.id
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        "#,
    )
    .bind(guild_id)
    .bind("quick brown")
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(and_results.len(), 1, "Expected 1 result for 'quick brown'");

    // Search with OR
    let or_results: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT m.id
        FROM messages m
        JOIN channels c ON m.channel_id = c.id
        WHERE c.guild_id = $1
          AND m.deleted_at IS NULL
          AND m.content_search @@ websearch_to_tsquery('english', $2)
        "#,
    )
    .bind(guild_id)
    .bind("quick OR lazy")
    .fetch_all(&pool)
    .await
    .expect("Search query failed");

    assert_eq!(or_results.len(), 3, "Expected 3 results for 'quick OR lazy'");

    cleanup_test_data(&pool, user_id, guild_id).await;
}
