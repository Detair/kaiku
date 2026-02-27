use std::sync::Arc;

use fred::prelude::*;
use sqlx::PgPool;
use vc_server::api::{AppState, AppStateConfig};
use vc_server::config::Config;
use vc_server::db;
use vc_server::ws::ServerEvent;

// Mock WebSocket connection logic for testing
#[tokio::test]
async fn test_websocket_broadcast_flow() {
    // 1. Setup Test Environment (DB, Redis, AppState)
    // Note: We need a real Redis and Postgres for this integration test
    let config = Config::default_for_test();
    let db_pool: PgPool = db::create_pool(&config.database_url)
        .await
        .expect("Failed to connect to DB");
    let redis = db::create_redis_client(&config.redis_url)
        .await
        .expect("Failed to connect to Redis");

    // Create dummy SFU server (not used for text chat test)
    let sfu = vc_server::voice::SfuServer::new(Arc::new(config.clone()), None)
        .expect("Failed to create SFU");

    let state = AppState::new(AppStateConfig {
        db: db_pool.clone(),
        redis: redis.clone(),
        config: config.clone(),
        s3: None,
        sfu,
        rate_limiter: None,
        email: None,
        oidc_manager: None,
    });

    // 2. Create Test Data with unique identifiers
    let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let user1_name = format!("ws_user1_{test_id}");
    let user2_name = format!("ws_user2_{test_id}");
    let channel_name = format!("ws-channel-{test_id}");

    let _user1 = db::create_user(&db_pool, &user1_name, "WS Test 1", None, "hash")
        .await
        .expect("Create user1 failed");
    let user2 = db::create_user(&db_pool, &user2_name, "WS Test 2", None, "hash")
        .await
        .expect("Create user2 failed");
    let channel = db::create_channel(
        &db_pool,
        db::CreateChannelParams {
            name: &channel_name,
            channel_type: &db::ChannelType::Text,
            category_id: None,
            guild_id: None,
            topic: None,
            icon_url: None,
            user_limit: None,
        },
    )
    .await
    .expect("Create channel failed");

    // 3. Simulate User 1 Subscription (Receiver)
    // We can't easily spawn the full Axum WebSocket handler in a unit test without a running
    // server, but we can test the `handle_pubsub` and `broadcast_to_channel` logic which is the
    // core of the sync robustness.

    // Verify Redis PubSub works
    let subscriber = state.redis.clone_new();
    let _ = subscriber.connect();
    subscriber
        .wait_for_connect()
        .await
        .expect("Redis subscriber connect failed");

    let channel_topic = vc_server::ws::channels::channel_events(channel.id);
    let () = subscriber
        .subscribe(channel_topic.clone())
        .await
        .expect("Subscribe failed");
    let mut message_stream = subscriber.message_rx();

    // 4. Simulate User 2 Sending a Message (Trigger)
    // This calls the logic that would happen in the HTTP handler
    let msg_content = "Hello WebSocket";
    let message = db::create_message(
        &db_pool,
        channel.id,
        user2.id,
        msg_content,
        false,
        None,
        None,
    )
    .await
    .expect("Create message failed");

    // Construct the event that the API handler would broadcast
    // (This mimics what happens in `server/src/chat/messages.rs:create`)
    let response_payload = serde_json::json!({
        "id": message.id,
        "content": message.content,
        "channel_id": message.channel_id,
        "author": {
            "id": user2.id,
            "username": user2.username,
            "display_name": user2.display_name,
            "status": "offline" // Default for test
        },
        "created_at": message.created_at,
        "encrypted": false,
        "attachments": [],
        "reply_to": null,
        "edited_at": null
    });

    let event = ServerEvent::MessageNew {
        channel_id: channel.id,
        message: response_payload,
    };

    // 5. Broadcast the event
    let _: () = vc_server::ws::broadcast_to_channel(&state.redis, channel.id, &event)
        .await
        .expect("Broadcast failed");

    // 6. Verify Reception
    let received = tokio::time::timeout(tokio::time::Duration::from_secs(2), message_stream.recv())
        .await
        .expect("Timed out waiting for Redis message")
        .expect("Stream closed");

    // Verify channel
    assert_eq!(received.channel, channel_topic);

    // Verify payload
    let payload_str = received.value.as_str().expect("Payload not string");
    let received_event: ServerEvent =
        serde_json::from_str(payload_str.as_ref()).expect("Failed to parse event");

    if let ServerEvent::MessageNew {
        channel_id: cid,
        message: msg,
    } = received_event
    {
        assert_eq!(cid, channel.id);
        assert_eq!(msg["content"], msg_content);
        assert_eq!(msg["author"]["username"], user2_name);
    } else {
        panic!("Received wrong event type: {received_event:?}");
    }

    println!("✅ WebSocket robustness test passed: Redis PubSub broadcast verified.");
}

/// Helper struct for WebSocket permission tests
struct PermissionTestContext {
    state: AppState,
    db_pool: PgPool,
    guild_id: uuid::Uuid,
    channel: db::Channel,
    owner: db::User,
    user_no_perm: db::User,
    user_with_perm: db::User,
}

impl PermissionTestContext {
    /// Setup test environment with guild, roles, and users
    async fn setup() -> Self {
        use vc_server::permissions::GuildPermissions;

        let config = Config::default_for_test();
        let db_pool: PgPool = db::create_pool(&config.database_url)
            .await
            .expect("Failed to connect to DB");
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to Redis");

        let sfu = vc_server::voice::SfuServer::new(Arc::new(config.clone()), None)
            .expect("Failed to create SFU");

        let state = AppState::new(AppStateConfig {
            db: db_pool.clone(),
            redis: redis.clone(),
            config: config.clone(),
            s3: None,
            sfu,
            rate_limiter: None,
            email: None,
            oidc_manager: None,
        });

        // Create test data with unique identifiers
        let test_id = uuid::Uuid::new_v4().to_string()[..8].to_string();

        // Create guild owner
        let owner = db::create_user(
            &db_pool,
            &format!("owner_{test_id}"),
            "Owner User",
            None,
            "hash",
        )
        .await
        .expect("Create owner failed");

        // Create test user WITHOUT VIEW_CHANNEL permission
        let user_no_perm = db::create_user(
            &db_pool,
            &format!("user_no_perm_{test_id}"),
            "No Perm User",
            None,
            "hash",
        )
        .await
        .expect("Create user_no_perm failed");

        // Create test user WITH VIEW_CHANNEL permission
        let user_with_perm = db::create_user(
            &db_pool,
            &format!("user_with_perm_{test_id}"),
            "With Perm User",
            None,
            "hash",
        )
        .await
        .expect("Create user_with_perm failed");

        // Create guild
        let guild_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
            .bind(guild_id)
            .bind(format!("Test Guild {test_id}"))
            .bind(owner.id)
            .execute(&db_pool)
            .await
            .expect("Create guild failed");

        // Add users as guild members
        for user_id in [owner.id, user_no_perm.id, user_with_perm.id] {
            sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
                .bind(guild_id)
                .bind(user_id)
                .execute(&db_pool)
                .await
                .expect("Add guild member failed");
        }

        // Create @everyone role WITHOUT VIEW_CHANNEL permission
        let everyone_role_id = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
             VALUES ($1, $2, '@everyone', 0, 0, true)",
        )
        .bind(everyone_role_id)
        .bind(guild_id)
        .execute(&db_pool)
        .await
        .expect("Create @everyone role failed");

        // Assign @everyone role to all members
        for user_id in [owner.id, user_no_perm.id, user_with_perm.id] {
            sqlx::query(
                "INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
            )
            .bind(guild_id)
            .bind(user_id)
            .bind(everyone_role_id)
            .execute(&db_pool)
            .await
            .expect("Assign @everyone role failed");
        }

        // Create special role WITH VIEW_CHANNEL permission
        let special_role_id = uuid::Uuid::new_v4();
        let view_channel_bit = GuildPermissions::VIEW_CHANNEL.bits() as i64;
        sqlx::query(
            "INSERT INTO guild_roles (id, guild_id, name, permissions, position, is_default)
             VALUES ($1, $2, 'Special', $3, 1, false)",
        )
        .bind(special_role_id)
        .bind(guild_id)
        .bind(view_channel_bit)
        .execute(&db_pool)
        .await
        .expect("Create special role failed");

        // Assign special role only to user_with_perm
        sqlx::query(
            "INSERT INTO guild_member_roles (guild_id, user_id, role_id) VALUES ($1, $2, $3)",
        )
        .bind(guild_id)
        .bind(user_with_perm.id)
        .bind(special_role_id)
        .execute(&db_pool)
        .await
        .expect("Assign special role failed");

        // Create channel in the guild
        let channel = db::create_channel(
            &db_pool,
            db::CreateChannelParams {
                name: &format!("test-channel-{test_id}"),
                channel_type: &db::ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Create channel failed");

        Self {
            state,
            db_pool,
            guild_id,
            channel,
            owner,
            user_no_perm,
            user_with_perm,
        }
    }

    /// Cleanup test resources
    async fn cleanup(self) {
        // Cleanup in reverse dependency order
        let _ = sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(self.channel.id)
            .execute(&self.db_pool)
            .await;

        let _ = sqlx::query("DELETE FROM guild_member_roles WHERE guild_id = $1")
            .bind(self.guild_id)
            .execute(&self.db_pool)
            .await;

        let _ = sqlx::query("DELETE FROM guild_roles WHERE guild_id = $1")
            .bind(self.guild_id)
            .execute(&self.db_pool)
            .await;

        let _ = sqlx::query("DELETE FROM guild_members WHERE guild_id = $1")
            .bind(self.guild_id)
            .execute(&self.db_pool)
            .await;

        let _ = sqlx::query("DELETE FROM guilds WHERE id = $1")
            .bind(self.guild_id)
            .execute(&self.db_pool)
            .await;

        let _ = sqlx::query("DELETE FROM users WHERE id = ANY($1)")
            .bind([self.owner.id, self.user_no_perm.id, self.user_with_perm.id])
            .execute(&self.db_pool)
            .await;
    }
}

/// Test that WebSocket Subscribe is denied without `VIEW_CHANNEL` permission
#[tokio::test]
async fn test_websocket_subscribe_denied_without_permission() {
    use tokio::sync::mpsc;

    let ctx = PermissionTestContext::setup().await;

    let (tx, mut rx) = mpsc::channel(10);
    let subscribed_channels = Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new()));
    let admin_subscribed = Arc::new(tokio::sync::RwLock::new(false));
    let mut activity_state = vc_server::ws::ActivityState::default();

    let subscribe_event = serde_json::json!({
        "type": "subscribe",
        "channel_id": ctx.channel.id.to_string()
    });

    // Call handle_client_message
    let result = vc_server::ws::handle_client_message(
        &subscribe_event.to_string(),
        ctx.user_no_perm.id,
        &ctx.state,
        &tx,
        &subscribed_channels,
        &admin_subscribed,
        &mut activity_state,
    )
    .await;

    assert!(result.is_ok(), "Handler should not crash");

    // Check that user was NOT added to subscribed channels
    let subscribed = subscribed_channels.read().await;
    assert!(
        !subscribed.contains(&ctx.channel.id),
        "User without VIEW_CHANNEL should NOT be subscribed"
    );

    // Check that an error event was sent (1000ms timeout for CI robustness)
    let event = tokio::time::timeout(tokio::time::Duration::from_millis(1000), rx.recv())
        .await
        .expect("Should receive error event")
        .expect("Channel should not be closed");

    match event {
        ServerEvent::Error { code, message } => {
            assert_eq!(code, "forbidden");
            assert!(message.contains("permission"));
        }
        _ => panic!("Expected Error event, got {event:?}"),
    }

    ctx.cleanup().await;
    println!("✅ WebSocket Subscribe denied without permission test passed.");
}

/// Test that WebSocket Subscribe is allowed with `VIEW_CHANNEL` permission
#[tokio::test]
async fn test_websocket_subscribe_allowed_with_permission() {
    use tokio::sync::mpsc;

    let ctx = PermissionTestContext::setup().await;

    let (tx, mut rx) = mpsc::channel(10);
    let subscribed_channels = Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new()));
    let admin_subscribed = Arc::new(tokio::sync::RwLock::new(false));
    let mut activity_state = vc_server::ws::ActivityState::default();

    let subscribe_event = serde_json::json!({
        "type": "subscribe",
        "channel_id": ctx.channel.id.to_string()
    });

    let result = vc_server::ws::handle_client_message(
        &subscribe_event.to_string(),
        ctx.user_with_perm.id,
        &ctx.state,
        &tx,
        &subscribed_channels,
        &admin_subscribed,
        &mut activity_state,
    )
    .await;

    assert!(result.is_ok(), "Handler should succeed");

    // Check that user WAS added to subscribed channels
    let subscribed = subscribed_channels.read().await;
    assert!(
        subscribed.contains(&ctx.channel.id),
        "User with VIEW_CHANNEL should be subscribed"
    );

    // Check that a success event was sent (1000ms timeout for CI robustness)
    let event = tokio::time::timeout(tokio::time::Duration::from_millis(1000), rx.recv())
        .await
        .expect("Should receive subscribed event")
        .expect("Channel should not be closed");

    match event {
        ServerEvent::Subscribed { channel_id } => {
            assert_eq!(channel_id, ctx.channel.id);
        }
        _ => panic!("Expected Subscribed event, got {event:?}"),
    }

    ctx.cleanup().await;
    println!("✅ WebSocket Subscribe allowed with permission test passed.");
}

/// Test that guild owner can subscribe (owner bypass)
#[tokio::test]
async fn test_websocket_subscribe_owner_bypass() {
    use tokio::sync::mpsc;

    let ctx = PermissionTestContext::setup().await;

    let (tx, mut rx) = mpsc::channel(10);
    let subscribed_channels = Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new()));
    let admin_subscribed = Arc::new(tokio::sync::RwLock::new(false));
    let mut activity_state = vc_server::ws::ActivityState::default();

    let subscribe_event = serde_json::json!({
        "type": "subscribe",
        "channel_id": ctx.channel.id.to_string()
    });

    let result = vc_server::ws::handle_client_message(
        &subscribe_event.to_string(),
        ctx.owner.id,
        &ctx.state,
        &tx,
        &subscribed_channels,
        &admin_subscribed,
        &mut activity_state,
    )
    .await;

    assert!(result.is_ok(), "Handler should succeed for owner");

    // Check that owner WAS added to subscribed channels
    let subscribed = subscribed_channels.read().await;
    assert!(
        subscribed.contains(&ctx.channel.id),
        "Guild owner should be able to subscribe"
    );

    // Check that a success event was sent (1000ms timeout for CI robustness)
    let event = tokio::time::timeout(tokio::time::Duration::from_millis(1000), rx.recv())
        .await
        .expect("Should receive subscribed event")
        .expect("Channel should not be closed");

    match event {
        ServerEvent::Subscribed { channel_id } => {
            assert_eq!(channel_id, ctx.channel.id);
        }
        _ => panic!("Expected Subscribed event, got {event:?}"),
    }

    ctx.cleanup().await;
    println!("✅ WebSocket Subscribe owner bypass test passed.");
}
