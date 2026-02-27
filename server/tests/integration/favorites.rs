//! Integration tests for the favorites system.
//!
//! These tests require a running `PostgreSQL` instance with the schema applied.
//! Run with: `cargo test favorites --ignored -- --nocapture`

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
        r"
        INSERT INTO users (id, username, display_name, password_hash, email)
        VALUES ($1, $2, $3, 'fake_hash', $4)
        ",
    )
    .bind(user_id)
    .bind(&username)
    .bind(&username)
    .bind(format!("{username}@test.com"))
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
        r"
        INSERT INTO guilds (id, name, owner_id)
        VALUES ($1, $2, $3)
        ",
    )
    .bind(guild_id)
    .bind(&guild_name)
    .bind(owner_id)
    .execute(pool)
    .await
    .expect("Failed to create test guild");

    // Add owner as guild member
    sqlx::query(
        r"
        INSERT INTO guild_members (guild_id, user_id)
        VALUES ($1, $2)
        ",
    )
    .bind(guild_id)
    .bind(owner_id)
    .execute(pool)
    .await
    .expect("Failed to add guild member");

    guild_id
}

/// Helper to create a test channel and return its ID.
async fn create_test_channel(
    pool: &PgPool,
    guild_id: Uuid,
    name: &str,
    channel_type: &str,
) -> Uuid {
    let channel_id = Uuid::new_v4();

    sqlx::query(
        r"
        INSERT INTO channels (id, guild_id, name, channel_type)
        VALUES ($1, $2, $3, $4)
        ",
    )
    .bind(channel_id)
    .bind(guild_id)
    .bind(name)
    .bind(channel_type)
    .execute(pool)
    .await
    .expect("Failed to create test channel");

    channel_id
}

/// Helper to add a favorite.
async fn add_favorite(pool: &PgPool, user_id: Uuid, guild_id: Uuid, channel_id: Uuid) {
    // First insert guild entry
    sqlx::query(
        r"
        INSERT INTO user_favorite_guilds (user_id, guild_id, position)
        SELECT $1, $2, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_guilds WHERE user_id = $1), 0)
        ON CONFLICT (user_id, guild_id) DO NOTHING
        ",
    )
    .bind(user_id)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("Failed to insert favorite guild");

    // Then insert channel entry
    sqlx::query(
        r"
        INSERT INTO user_favorite_channels (user_id, channel_id, guild_id, position)
        VALUES ($1, $2, $3, COALESCE((SELECT MAX(position) + 1 FROM user_favorite_channels WHERE user_id = $1 AND guild_id = $3), 0))
        ",
    )
    .bind(user_id)
    .bind(channel_id)
    .bind(guild_id)
    .execute(pool)
    .await
    .expect("Failed to insert favorite channel");
}

/// Helper to count favorites for a user.
async fn count_favorites(pool: &PgPool, user_id: Uuid) -> i64 {
    let result: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_favorite_channels WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to count favorites");
    result.0
}

/// Helper to count favorite guilds for a user.
async fn count_favorite_guilds(pool: &PgPool, user_id: Uuid) -> i64 {
    let result: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_favorite_guilds WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("Failed to count favorite guilds");
    result.0
}

/// Helper to clean up test data.
async fn cleanup_test_data(pool: &PgPool, user_id: Uuid) {
    // Delete favorites first (cascade should handle this, but be explicit)
    sqlx::query("DELETE FROM user_favorite_channels WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM user_favorite_guilds WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
    // Delete user's guild memberships
    sqlx::query("DELETE FROM guild_members WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .ok();
    // Delete guilds owned by user (channels cascade)
    sqlx::query("DELETE FROM guilds WHERE owner_id = $1")
        .bind(user_id)
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

/// Test adding a favorite channel.
#[tokio::test]
#[ignore]
async fn test_add_favorite_channel() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general", "text").await;

    add_favorite(&pool, user_id, guild_id, channel_id).await;

    let count = count_favorites(&pool, user_id).await;
    assert_eq!(count, 1, "Should have 1 favorite");

    let guild_count = count_favorite_guilds(&pool, user_id).await;
    assert_eq!(guild_count, 1, "Should have 1 favorite guild");

    cleanup_test_data(&pool, user_id).await;
}

/// Test removing a favorite triggers guild cleanup.
#[tokio::test]
#[ignore]
async fn test_remove_favorite_triggers_guild_cleanup() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general", "text").await;

    // Add favorite
    add_favorite(&pool, user_id, guild_id, channel_id).await;
    assert_eq!(count_favorite_guilds(&pool, user_id).await, 1);

    // Remove favorite channel
    sqlx::query("DELETE FROM user_favorite_channels WHERE user_id = $1 AND channel_id = $2")
        .bind(user_id)
        .bind(channel_id)
        .execute(&pool)
        .await
        .expect("Failed to remove favorite");

    // Guild should be auto-cleaned by trigger
    let guild_count = count_favorite_guilds(&pool, user_id).await;
    assert_eq!(guild_count, 0, "Guild should be removed by cleanup trigger");

    cleanup_test_data(&pool, user_id).await;
}

/// Test that multiple channels in same guild share guild entry.
#[tokio::test]
#[ignore]
async fn test_multiple_channels_same_guild() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel1_id = create_test_channel(&pool, guild_id, "general", "text").await;
    let channel2_id = create_test_channel(&pool, guild_id, "voice", "voice").await;

    add_favorite(&pool, user_id, guild_id, channel1_id).await;
    add_favorite(&pool, user_id, guild_id, channel2_id).await;

    assert_eq!(
        count_favorites(&pool, user_id).await,
        2,
        "Should have 2 favorites"
    );
    assert_eq!(
        count_favorite_guilds(&pool, user_id).await,
        1,
        "Should have only 1 guild entry"
    );

    cleanup_test_data(&pool, user_id).await;
}

/// Test user cascade delete removes favorites.
#[tokio::test]
#[ignore]
async fn test_user_delete_cascades_to_favorites() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general", "text").await;

    add_favorite(&pool, user_id, guild_id, channel_id).await;
    assert_eq!(count_favorites(&pool, user_id).await, 1);

    // Delete memberships and guilds first (to avoid FK constraints)
    sqlx::query("DELETE FROM guild_members WHERE user_id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM guilds WHERE owner_id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .ok();

    // Delete user (favorites should cascade)
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("Failed to delete user");

    // Verify favorites are gone
    let count = count_favorites(&pool, user_id).await;
    assert_eq!(count, 0, "Favorites should be deleted with user");
}

/// Test channel delete cascades to favorites.
#[tokio::test]
#[ignore]
async fn test_channel_delete_cascades_to_favorites() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel_id = create_test_channel(&pool, guild_id, "general", "text").await;

    add_favorite(&pool, user_id, guild_id, channel_id).await;
    assert_eq!(count_favorites(&pool, user_id).await, 1);

    // Delete channel
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel_id)
        .execute(&pool)
        .await
        .expect("Failed to delete channel");

    // Favorite should be gone
    let count = count_favorites(&pool, user_id).await;
    assert_eq!(count, 0, "Favorite should be deleted with channel");

    // Guild entry should also be gone (trigger)
    let guild_count = count_favorite_guilds(&pool, user_id).await;
    assert_eq!(guild_count, 0, "Guild entry should be removed by trigger");

    cleanup_test_data(&pool, user_id).await;
}

/// Test position ordering for channels.
#[tokio::test]
#[ignore]
async fn test_channel_position_ordering() {
    let pool = create_test_pool().await;
    let user_id = create_test_user(&pool).await;
    let guild_id = create_test_guild(&pool, user_id).await;
    let channel1_id = create_test_channel(&pool, guild_id, "first", "text").await;
    let channel2_id = create_test_channel(&pool, guild_id, "second", "text").await;
    let channel3_id = create_test_channel(&pool, guild_id, "third", "text").await;

    add_favorite(&pool, user_id, guild_id, channel1_id).await;
    add_favorite(&pool, user_id, guild_id, channel2_id).await;
    add_favorite(&pool, user_id, guild_id, channel3_id).await;

    // Check positions are sequential
    let positions: Vec<(i32,)> = sqlx::query_as(
        "SELECT position FROM user_favorite_channels WHERE user_id = $1 ORDER BY position",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .expect("Failed to fetch positions");

    assert_eq!(positions.len(), 3);
    assert_eq!(positions[0].0, 0);
    assert_eq!(positions[1].0, 1);
    assert_eq!(positions[2].0, 2);

    cleanup_test_data(&pool, user_id).await;
}
