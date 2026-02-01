use fred::prelude::*;
use sqlx::PgPool;
use std::sync::Arc;
use vc_server::api::AppState;
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
    let sfu =
        vc_server::voice::SfuServer::new(Arc::new(config.clone()), None).expect("Failed to create SFU");

    let state = AppState::new(
        db_pool.clone(),
        redis.clone(),
        config.clone(),
        None,
        sfu,
        None,
        None,
        None,
    );

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
        &channel_name,
        &db::ChannelType::Text,
        None,
        None,
        None,
        None,
        None,  // user_limit
    )
    .await
    .expect("Create channel failed");

    // 3. Simulate User 1 Subscription (Receiver)
    // We can't easily spawn the full Axum WebSocket handler in a unit test without a running server,
    // but we can test the `handle_pubsub` and `broadcast_to_channel` logic which is the core of the sync robustness.

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
