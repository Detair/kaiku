//! Tests for voice WebSocket handlers.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fred::prelude::*;
    use sqlx::{PgPool, Row};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use crate::config::Config;
    use crate::voice::{error, sfu, ws_handler};
    use crate::ws::{ClientEvent, ServerEvent};

    /// Helper to create a test Redis client.
    async fn create_test_redis() -> Client {
        let config = fred::types::config::Config::from_url("redis://localhost:6379").unwrap();
        let client = Client::new(config, None, None, None);
        client.connect();
        client
            .wait_for_connect()
            .await
            .expect("Failed to connect to Redis");
        client
    }

    /// Helper to create test user in database.
    async fn create_test_user(
        pool: &PgPool,
        username: &str,
        display_name: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$test$test"; // Dummy hash

        let row = sqlx::query(
            "INSERT INTO users (username, display_name, password_hash) VALUES ($1, $2, $3) RETURNING id"
        )
        .bind(username)
        .bind(display_name)
        .bind(password_hash)
        .fetch_one(pool)
        .await?;

        row.try_get("id")
    }

    /// Helper to create test channel in database with guild permissions.
    async fn create_test_channel(
        pool: &PgPool,
        name: &str,
        guild_id: Uuid,
    ) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO channels (name, channel_type, position, guild_id) VALUES ($1, 'voice', 0, $2) RETURNING id"
        )
        .bind(name)
        .bind(guild_id)
        .fetch_one(pool)
        .await?;

        row.try_get("id")
    }

    /// Helper to create a guild with proper permissions for voice testing
    async fn create_test_guild_with_voice_permissions(
        pool: &PgPool,
        owner_id: Uuid,
    ) -> Result<Uuid, sqlx::Error> {
        // VIEW_CHANNEL = 1 << 24, VOICE_CONNECT = 1 << 5
        let permissions = (1i64 << 24) | (1i64 << 5);

        // Create guild
        let guild_id = Uuid::new_v4();
        sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
            .bind(guild_id)
            .bind("Test Voice Guild")
            .bind(owner_id)
            .execute(pool)
            .await?;

        // Add owner as member
        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(guild_id)
            .bind(owner_id)
            .execute(pool)
            .await?;

        // Create @everyone role with VIEW_CHANNEL + VOICE_CONNECT
        let everyone_role_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
             VALUES ($1, $2, '@everyone', $3, 0, true)",
        )
        .bind(everyone_role_id)
        .bind(guild_id)
        .bind(permissions)
        .execute(pool)
        .await?;

        // Assign @everyone role to owner
        sqlx::query(
            "INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
        )
        .bind(guild_id)
        .bind(owner_id)
        .bind(everyone_role_id)
        .execute(pool)
        .await?;

        Ok(guild_id)
    }

    /// Helper to add a user to an existing guild
    async fn add_user_to_guild(
        pool: &PgPool,
        guild_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        // Add as guild member
        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(guild_id)
            .bind(user_id)
            .execute(pool)
            .await?;

        // Get @everyone role
        let everyone_role: (Uuid,) =
            sqlx::query_as("SELECT id FROM guild_roles WHERE guild_id = $1 AND is_default = true")
                .bind(guild_id)
                .fetch_one(pool)
                .await?;

        // Assign @everyone role
        sqlx::query(
            "INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
        )
        .bind(guild_id)
        .bind(user_id)
        .bind(everyone_role.0)
        .execute(pool)
        .await?;

        Ok(())
    }

    #[sqlx::test]
    async fn test_voice_join_includes_username(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create test user
        let user_id = create_test_user(&pool, "testuser", "Test User").await?;

        // Create guild with voice permissions
        let guild_id = create_test_guild_with_voice_permissions(&pool, user_id).await?;

        // Create test channel
        let channel_id = create_test_channel(&pool, "Test Voice Channel", guild_id).await?;

        // Create SFU server
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config, None)?);

        // Create Redis client
        let redis = create_test_redis().await;

        // Create channel for server events
        let (tx, mut rx) = mpsc::channel::<ServerEvent>(10);

        // Join voice channel
        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            &redis,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx,
        )
        .await?;

        // Verify VoiceOffer was sent
        let event = rx.recv().await.expect("Should receive VoiceOffer");
        assert!(matches!(event, ServerEvent::VoiceOffer { .. }));

        // Verify VoiceRoomState includes username
        let event = rx.recv().await.expect("Should receive VoiceRoomState");
        match event {
            ServerEvent::VoiceRoomState { participants, .. } => {
                assert_eq!(participants.len(), 1);
                assert_eq!(participants[0].username, Some("testuser".to_string()));
                assert_eq!(participants[0].display_name, Some("Test User".to_string()));
            }
            _ => panic!("Expected VoiceRoomState event"),
        }

        Ok(())
    }

    #[sqlx::test]
    #[ignore = "Requires Redis-backed rate limiter configuration"]
    async fn test_rate_limiting_blocks_rapid_joins(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create test user
        let user_id = create_test_user(&pool, "ratelimituser", "Rate Limit User").await?;

        // Create guild with voice permissions
        let guild_id = create_test_guild_with_voice_permissions(&pool, user_id).await?;

        // Create test channel
        let channel_id = create_test_channel(&pool, "Rate Limit Test", guild_id).await?;

        // Create SFU server
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config, None)?);

        // Create Redis client
        let redis = create_test_redis().await;

        // Create channel for server events
        let (tx, _rx) = mpsc::channel::<ServerEvent>(10);

        // First join should succeed
        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            &redis,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx,
        )
        .await?;

        // Immediate second join should fail with rate limit error
        let result = ws_handler::handle_voice_event(
            &sfu,
            &pool,
            &redis,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            error::VoiceError::RateLimited => {
                // Expected
            }
            other => panic!("Expected RateLimited error, got: {other:?}"),
        }

        Ok(())
    }

    #[sqlx::test]
    async fn test_different_users_can_join_simultaneously(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create two test users
        let user1_id = create_test_user(&pool, "user1", "User One").await?;
        let user2_id = create_test_user(&pool, "user2", "User Two").await?;

        // Create guild with voice permissions (user1 as owner)
        let guild_id = create_test_guild_with_voice_permissions(&pool, user1_id).await?;

        // Add user2 to the guild
        add_user_to_guild(&pool, guild_id, user2_id).await?;

        // Create test channel
        let channel_id = create_test_channel(&pool, "Multi User Test", guild_id).await?;

        // Create SFU server
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config, None)?);

        // Create Redis client
        let redis = create_test_redis().await;

        // Create channels for server events
        let (tx1, _rx1) = mpsc::channel::<ServerEvent>(10);
        let (tx2, _rx2) = mpsc::channel::<ServerEvent>(10);

        // Both users should be able to join
        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            &redis,
            user1_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx1,
        )
        .await?;

        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            &redis,
            user2_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx2,
        )
        .await?;

        // Verify both are in the room
        let room = sfu.get_or_create_room(channel_id).await;
        let participants = room.get_participant_info().await;
        assert_eq!(participants.len(), 2);

        Ok(())
    }
}
