//! Tests for voice WebSocket handlers.

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::voice::{error, sfu, ws_handler};
    use crate::ws::{ClientEvent, ServerEvent};
    use fred::prelude::*;
    use sqlx::{PgPool, Row};
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    /// Helper to create a test Redis client.
    async fn create_test_redis() -> RedisClient {
        let config = RedisConfig::from_url("redis://localhost:6379").unwrap();
        let client = RedisClient::new(config, None, None, None);
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

    /// Helper to create test channel in database.
    async fn create_test_channel(pool: &PgPool, name: &str) -> Result<Uuid, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO channels (name, channel_type, position) VALUES ($1, 'voice', 0) RETURNING id"
        )
        .bind(name)
        .fetch_one(pool)
        .await?;

        row.try_get("id")
    }

    #[sqlx::test]
    async fn test_voice_join_includes_username(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create test user
        let user_id = create_test_user(&pool, "testuser", "Test User").await?;

        // Create test channel
        let channel_id = create_test_channel(&pool, "Test Voice Channel").await?;

        // Create SFU server
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config)?);

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
    async fn test_rate_limiting_blocks_rapid_joins(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create test user
        let user_id = create_test_user(&pool, "ratelimituser", "Rate Limit User").await?;

        // Create test channel
        let channel_id = create_test_channel(&pool, "Rate Limit Test").await?;

        // Create SFU server
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config)?);

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

        // Create test channel
        let channel_id = create_test_channel(&pool, "Multi User Test").await?;

        // Create SFU server
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config)?);

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
