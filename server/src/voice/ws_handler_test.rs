//! Tests for voice WebSocket handlers.

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use fred::prelude::*;
    use sqlx::{PgPool, Row};
    use tokio::sync::mpsc;
    use uuid::Uuid;

    use crate::config::Config;
    use crate::ratelimit::{LimitConfig, RateLimitConfig, RateLimiter, RateLimits};
    use crate::voice::leave::leave_voice_on_socket_close;
    use crate::voice::sfu::StalePeer;
    use crate::voice::{error, sfu, ws_handler};
    use crate::ws::{ClientEvent, OutboundMsg, ServerEvent};

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
        let _redis = create_test_redis().await;

        // Create channel for server events
        let (tx, mut rx) = mpsc::channel::<OutboundMsg>(10);

        // Join voice channel
        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx,
            None,
        )
        .await?;

        // In the dual-PC model, the server does NOT send an initial offer.
        // The first message should be VoiceRoomState.

        // Verify VoiceRoomState includes username
        let msg = rx.recv().await.expect("Should receive VoiceRoomState");
        let OutboundMsg::Event(event) = msg else {
            panic!("Expected Event")
        };
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

        // Create guild with voice permissions
        let guild_id = create_test_guild_with_voice_permissions(&pool, user_id).await?;

        // Create test channel
        let channel_id = create_test_channel(&pool, "Rate Limit Test", guild_id).await?;

        // Create Redis client and build a rate limiter with voice_join limit of 1 per 60s.
        // This ensures the second VoiceJoin in the same window is blocked.
        let redis = create_test_redis().await;
        let rl_config = RateLimitConfig {
            enabled: true,
            redis_key_prefix: format!("test:vj:{}", uuid::Uuid::new_v4()),
            fail_open: false,
            trust_proxy: false,
            allowlist: HashSet::new(),
            limits: RateLimits {
                voice_join: LimitConfig {
                    requests: 1,
                    window_secs: 60,
                },
                ..RateLimits::default()
            },
        };
        let mut rate_limiter = RateLimiter::new(redis, rl_config);
        rate_limiter
            .init()
            .await
            .expect("Failed to init rate limiter");

        // Create SFU server with the rate limiter wired in.
        let config = Arc::new(Config::default_for_test());
        let sfu = Arc::new(sfu::SfuServer::new(config, Some(rate_limiter))?);

        // Create channel for server events
        let (tx, _rx) = mpsc::channel::<OutboundMsg>(10);

        // First join should succeed (1/1 voice_join budget consumed)
        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx,
            None,
        )
        .await?;

        // Immediate second join should fail with rate limit error (budget exhausted)
        let result = ws_handler::handle_voice_event(
            &sfu,
            &pool,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx,
            None,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            error::VoiceError::RateLimited(_) => {
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
        let _redis = create_test_redis().await;

        // Create channels for server events
        let (tx1, _rx1) = mpsc::channel::<OutboundMsg>(10);
        let (tx2, _rx2) = mpsc::channel::<OutboundMsg>(10);

        // Both users should be able to join
        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            user1_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx1,
            None,
        )
        .await?;

        ws_handler::handle_voice_event(
            &sfu,
            &pool,
            user2_id,
            ClientEvent::VoiceJoin { channel_id },
            &tx2,
            None,
        )
        .await?;

        // Verify both are in the room
        let room = sfu.get_or_create_room(channel_id).await;
        let participants = room.get_participant_info().await;
        assert_eq!(participants.len(), 2);

        Ok(())
    }

    /// Two users in one voice channel, each with its own socket queue.
    struct TwoInRoom {
        sfu: Arc<sfu::SfuServer>,
        channel_id: Uuid,
        guild_id: Uuid,
        u1: Uuid,
        u2: Uuid,
        tx1: mpsc::Sender<OutboundMsg>,
        tx2: mpsc::Sender<OutboundMsg>,
        _rx1: mpsc::Receiver<OutboundMsg>,
        rx2: mpsc::Receiver<OutboundMsg>,
    }

    async fn join(
        sfu: &Arc<sfu::SfuServer>,
        pool: &PgPool,
        user_id: Uuid,
        channel_id: Uuid,
        tx: &mpsc::Sender<OutboundMsg>,
    ) -> Result<(), error::VoiceError> {
        ws_handler::handle_voice_event(
            sfu,
            pool,
            user_id,
            ClientEvent::VoiceJoin { channel_id },
            tx,
            None,
        )
        .await
    }

    async fn two_in_room(pool: &PgPool) -> Result<TwoInRoom, Box<dyn std::error::Error>> {
        let u1 = create_test_user(pool, "ghost1", "Ghost One").await?;
        let u2 = create_test_user(pool, "ghost2", "Ghost Two").await?;
        let guild_id = create_test_guild_with_voice_permissions(pool, u1).await?;
        add_user_to_guild(pool, guild_id, u2).await?;
        let channel_id = create_test_channel(pool, "Ghost Test", guild_id).await?;
        let sfu = Arc::new(sfu::SfuServer::new(
            Arc::new(Config::default_for_test()),
            None,
        )?);
        let (tx1, rx1) = mpsc::channel::<OutboundMsg>(32);
        let (tx2, rx2) = mpsc::channel::<OutboundMsg>(32);
        join(&sfu, pool, u1, channel_id, &tx1).await?;
        join(&sfu, pool, u2, channel_id, &tx2).await?;
        Ok(TwoInRoom {
            sfu,
            channel_id,
            guild_id,
            u1,
            u2,
            tx1,
            tx2,
            _rx1: rx1,
            rx2,
        })
    }

    fn drain(rx: &mut mpsc::Receiver<OutboundMsg>) -> Vec<ServerEvent> {
        let mut events = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            if let OutboundMsg::Event(e) = msg {
                events.push(e);
            }
        }
        events
    }

    async fn peer_present(sfu: &sfu::SfuServer, channel_id: Uuid, user_id: Uuid) -> bool {
        match sfu.get_room(channel_id).await {
            Some(room) => room.get_peer(user_id).await.is_some(),
            None => false,
        }
    }

    #[sqlx::test]
    async fn test_socket_close_removes_peer_and_notifies_room(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut t = two_in_room(&pool).await?;
        drain(&mut t.rx2);

        leave_voice_on_socket_close(&t.sfu, &pool, t.u1, &t.tx1, None).await;

        assert!(!peer_present(&t.sfu, t.channel_id, t.u1).await);
        assert!(peer_present(&t.sfu, t.channel_id, t.u2).await);
        let events = drain(&mut t.rx2);
        assert!(
            events.iter().any(|e| matches!(
                e,
                ServerEvent::VoiceUserLeft { user_id, channel_id }
                    if *user_id == t.u1 && *channel_id == t.channel_id
            )),
            "remaining peer must be told the user left, got {events:?}"
        );
        Ok(())
    }

    #[sqlx::test]
    async fn test_socket_close_only_removes_peer_owned_by_that_socket(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let t = two_in_room(&pool).await?;
        let (tx_other, _rx_other) = mpsc::channel::<OutboundMsg>(4);

        // Another socket of the same user closing must not drop the call.
        leave_voice_on_socket_close(&t.sfu, &pool, t.u1, &tx_other, None).await;
        // A different user's socket queue never matches.
        leave_voice_on_socket_close(&t.sfu, &pool, t.u1, &t.tx2, None).await;

        assert!(peer_present(&t.sfu, t.channel_id, t.u1).await);
        assert!(peer_present(&t.sfu, t.channel_id, t.u2).await);
        Ok(())
    }

    #[sqlx::test]
    async fn test_leave_succeeds_after_access_revoked(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let t = two_in_room(&pool).await?;
        sqlx::query("DELETE FROM guild_member_roles WHERE guild_id = $1 AND user_id = $2")
            .bind(t.guild_id)
            .bind(t.u2)
            .execute(&pool)
            .await?;
        sqlx::query("DELETE FROM guild_members WHERE guild_id = $1 AND user_id = $2")
            .bind(t.guild_id)
            .bind(t.u2)
            .execute(&pool)
            .await?;

        ws_handler::handle_voice_event(
            &t.sfu,
            &pool,
            t.u2,
            ClientEvent::VoiceLeave {
                channel_id: t.channel_id,
            },
            &t.tx2,
            None,
        )
        .await?;

        assert!(!peer_present(&t.sfu, t.channel_id, t.u2).await);
        Ok(())
    }

    async fn wait_until_gone(sfu: &sfu::SfuServer, channel_id: Uuid, user_id: Uuid) -> bool {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            if !peer_present(sfu, channel_id, user_id).await {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        false
    }

    #[sqlx::test]
    async fn test_reaper_removes_matching_session(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let t = two_in_room(&pool).await?;
        let reaper = t.sfu.spawn_stale_peer_reaper(pool.clone(), None);
        let room = t.sfu.get_room(t.channel_id).await.expect("room exists");
        let session_id = room.get_peer(t.u1).await.expect("peer").session_id;

        t.sfu.stale_peer_sender().send(StalePeer {
            user_id: t.u1,
            channel_id: t.channel_id,
            session_id,
        })?;

        assert!(
            wait_until_gone(&t.sfu, t.channel_id, t.u1).await,
            "reaper must remove the stale peer within 2s"
        );
        assert!(peer_present(&t.sfu, t.channel_id, t.u2).await);
        reaper.abort();
        Ok(())
    }

    #[sqlx::test]
    async fn test_reaper_ignores_rejoined_session(
        pool: PgPool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let t = two_in_room(&pool).await?;
        let reaper = t.sfu.spawn_stale_peer_reaper(pool.clone(), None);
        let room = t.sfu.get_room(t.channel_id).await.expect("room exists");
        let old_session = room.get_peer(t.u1).await.expect("peer").session_id;

        // u1 re-joins (e.g. after a network switch): a new session replaces the old one.
        join(&t.sfu, &pool, t.u1, t.channel_id, &t.tx1).await?;
        let new_session = room.get_peer(t.u1).await.expect("peer").session_id;
        assert_ne!(old_session, new_session);

        t.sfu.stale_peer_sender().send(StalePeer {
            user_id: t.u1,
            channel_id: t.channel_id,
            session_id: old_session,
        })?;
        // Then a matching item for u2, so we know the reaper processed the queue.
        let u2_session = room.get_peer(t.u2).await.expect("peer").session_id;
        t.sfu.stale_peer_sender().send(StalePeer {
            user_id: t.u2,
            channel_id: t.channel_id,
            session_id: u2_session,
        })?;
        assert!(wait_until_gone(&t.sfu, t.channel_id, t.u2).await);

        let current = room.get_peer(t.u1).await.expect("rejoined peer must stay");
        assert_eq!(current.session_id, new_session);
        reaper.abort();
        Ok(())
    }
}
