//! Redis Integration Tests
//!
//! Comprehensive tests for Redis operations including caching, pub/sub, and sessions.

#[cfg(test)]
mod redis_tests {
    use std::time::Duration;

    use fred::prelude::*;
    use tokio::time::sleep;
    use uuid::Uuid;

    /// Helper to create a test Redis client
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

    /// Helper to clean up test keys
    async fn cleanup_key(client: &Client, key: &str) {
        let _ = client.del::<(), _>(key).await;
    }

    // ========================================================================
    // Basic Operations Tests
    // ========================================================================

    #[tokio::test]
    async fn test_redis_connection() {
        let client = create_test_redis().await;

        // Test basic ping
        let pong: String = client.ping(None).await.expect("Ping failed");
        assert_eq!(pong, "PONG");
    }

    #[tokio::test]
    async fn test_set_and_get() {
        let client = create_test_redis().await;
        let key = "test:basic:set_get";

        // Set a value
        client
            .set::<(), _, _>(key, "test_value", None, None, false)
            .await
            .expect("Failed to SET");

        // Get the value
        let value: String = client.get(key).await.expect("Failed to GET");
        assert_eq!(value, "test_value");

        cleanup_key(&client, key).await;
    }

    #[tokio::test]
    async fn test_set_with_expiry() {
        let client = create_test_redis().await;
        let key = "test:expiry:key";

        // Clean up from any previous runs
        cleanup_key(&client, key).await;

        // Set with 2 second expiry
        client
            .set::<(), _, _>(key, "expires_soon", Some(Expiration::EX(2)), None, false)
            .await
            .expect("Failed to SET with expiry");

        // Should exist immediately
        let exists: bool = client.exists(key).await.expect("Failed to check EXISTS");
        assert!(exists);

        // Wait for expiry
        sleep(Duration::from_secs(3)).await;

        // Should not exist after expiry
        let exists: bool = client.exists(key).await.expect("Failed to check EXISTS");
        assert!(!exists);
    }

    #[tokio::test]
    async fn test_increment_operations() {
        let client = create_test_redis().await;
        let key = "test:counter:incr";

        // Initial increment
        let count: i64 = client.incr(key).await.expect("Failed to INCR");
        assert_eq!(count, 1);

        // Increment again
        let count: i64 = client.incr(key).await.expect("Failed to INCR");
        assert_eq!(count, 2);

        // Increment by amount
        let count: i64 = client.incr_by(key, 5).await.expect("Failed to INCRBY");
        assert_eq!(count, 7);

        cleanup_key(&client, key).await;
    }

    #[tokio::test]
    async fn test_delete_key() {
        let client = create_test_redis().await;
        let key = "test:delete:key";

        // Set a value
        client
            .set::<(), _, _>(key, "to_delete", None, None, false)
            .await
            .expect("Failed to SET");

        // Delete it
        let deleted: i64 = client.del(key).await.expect("Failed to DEL");
        assert_eq!(deleted, 1);

        // Should not exist
        let exists: bool = client.exists(key).await.expect("Failed to check EXISTS");
        assert!(!exists);
    }

    // ========================================================================
    // Hash Operations Tests (for structured data)
    // ========================================================================

    #[tokio::test]
    async fn test_hash_operations() {
        let client = create_test_redis().await;
        let key = "test:hash:user_data";

        // Set hash fields
        client
            .hset::<(), _, _>(key, ("username", "testuser"))
            .await
            .expect("Failed to HSET username");
        client
            .hset::<(), _, _>(key, ("email", "test@example.com"))
            .await
            .expect("Failed to HSET email");
        client
            .hset::<(), _, _>(key, ("status", "online"))
            .await
            .expect("Failed to HSET status");

        // Get single field
        let username: String = client.hget(key, "username").await.expect("Failed to HGET");
        assert_eq!(username, "testuser");

        // Get all fields
        let all_fields: std::collections::HashMap<String, String> =
            client.hgetall(key).await.expect("Failed to HGETALL");
        assert_eq!(all_fields.len(), 3);
        assert_eq!(all_fields.get("username"), Some(&"testuser".to_string()));
        assert_eq!(
            all_fields.get("email"),
            Some(&"test@example.com".to_string())
        );
        assert_eq!(all_fields.get("status"), Some(&"online".to_string()));

        // Check if field exists
        let exists: bool = client
            .hexists(key, "username")
            .await
            .expect("Failed to HEXISTS");
        assert!(exists);

        // Delete a field
        let deleted: i64 = client.hdel(key, "status").await.expect("Failed to HDEL");
        assert_eq!(deleted, 1);

        // Verify field is gone
        let all_fields: std::collections::HashMap<String, String> =
            client.hgetall(key).await.expect("Failed to HGETALL");
        assert_eq!(all_fields.len(), 2);
        assert!(!all_fields.contains_key("status"));

        cleanup_key(&client, key).await;
    }

    // ========================================================================
    // List Operations Tests (for queues/stacks)
    // ========================================================================

    #[tokio::test]
    async fn test_list_operations() {
        let client = create_test_redis().await;
        let key = "test:list:messages";

        // Push items to list
        client
            .rpush::<(), _, _>(key, "message1")
            .await
            .expect("Failed to RPUSH");
        client
            .rpush::<(), _, _>(key, "message2")
            .await
            .expect("Failed to RPUSH");
        client
            .rpush::<(), _, _>(key, "message3")
            .await
            .expect("Failed to RPUSH");

        // Get list length
        let len: i64 = client.llen(key).await.expect("Failed to LLEN");
        assert_eq!(len, 3);

        // Get all items
        let items: Vec<String> = client.lrange(key, 0, -1).await.expect("Failed to LRANGE");
        assert_eq!(items, vec!["message1", "message2", "message3"]);

        // Pop from left
        let popped: String = client.lpop(key, None).await.expect("Failed to LPOP");
        assert_eq!(popped, "message1");

        // Verify length decreased
        let len: i64 = client.llen(key).await.expect("Failed to LLEN");
        assert_eq!(len, 2);

        cleanup_key(&client, key).await;
    }

    // ========================================================================
    // Set Operations Tests (for unique collections)
    // ========================================================================

    #[tokio::test]
    async fn test_set_operations() {
        let client = create_test_redis().await;
        let key = "test:set:active_users";

        // Add members to set
        let added: i64 = client.sadd(key, "user1").await.expect("Failed to SADD");
        assert_eq!(added, 1);

        let _: i64 = client.sadd(key, "user2").await.expect("Failed to SADD");
        let _: i64 = client.sadd(key, "user3").await.expect("Failed to SADD");

        // Try to add duplicate (should not increase count)
        let added: i64 = client.sadd(key, "user1").await.expect("Failed to SADD");
        assert_eq!(added, 0);

        // Get set size
        let size: i64 = client.scard(key).await.expect("Failed to SCARD");
        assert_eq!(size, 3);

        // Check membership
        let is_member: bool = client
            .sismember(key, "user2")
            .await
            .expect("Failed to SISMEMBER");
        assert!(is_member);

        let is_member: bool = client
            .sismember(key, "user99")
            .await
            .expect("Failed to SISMEMBER");
        assert!(!is_member);

        // Get all members
        let members: Vec<String> = client.smembers(key).await.expect("Failed to SMEMBERS");
        assert_eq!(members.len(), 3);
        assert!(members.contains(&"user1".to_string()));
        assert!(members.contains(&"user2".to_string()));
        assert!(members.contains(&"user3".to_string()));

        // Remove a member
        let removed: i64 = client.srem(key, "user2").await.expect("Failed to SREM");
        assert_eq!(removed, 1);

        // Verify removal
        let size: i64 = client.scard(key).await.expect("Failed to SCARD");
        assert_eq!(size, 2);

        cleanup_key(&client, key).await;
    }

    // ========================================================================
    // Pub/Sub Tests (for real-time messaging)
    // ========================================================================

    #[tokio::test]
    async fn test_pubsub_basic() {
        let publisher = create_test_redis().await;
        let subscriber = publisher.clone_new();
        subscriber.connect();
        subscriber
            .wait_for_connect()
            .await
            .expect("Failed to connect subscriber");

        let channel = "test:pubsub:basic";
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);

        // Subscribe to channel
        let mut pubsub_stream = subscriber.message_rx();
        subscriber
            .subscribe(channel)
            .await
            .expect("Failed to SUBSCRIBE");

        // Spawn task to receive messages
        let channel_name = channel.to_string();
        tokio::spawn(async move {
            while let Ok(message) = pubsub_stream.recv().await {
                if message.channel == channel_name {
                    if let Ok(value) = String::from_utf8(message.value.as_bytes().unwrap().to_vec())
                    {
                        let _ = tx.send(value).await;
                    }
                }
            }
        });

        // Give subscriber time to fully subscribe
        sleep(Duration::from_millis(100)).await;

        // Publish a message
        let _: i64 = publisher
            .publish(channel, "Hello, World!")
            .await
            .expect("Failed to PUBLISH");

        // Receive the message
        let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Timeout waiting for message")
            .expect("Channel closed");
        assert_eq!(received, "Hello, World!");

        // Cleanup
        subscriber
            .unsubscribe(channel)
            .await
            .expect("Failed to UNSUBSCRIBE");
    }

    #[tokio::test]
    async fn test_pubsub_multiple_subscribers() {
        let publisher = create_test_redis().await;
        let subscriber1 = publisher.clone_new();
        let subscriber2 = publisher.clone_new();

        subscriber1.connect();
        subscriber2.connect();
        subscriber1
            .wait_for_connect()
            .await
            .expect("Failed to connect subscriber1");
        subscriber2
            .wait_for_connect()
            .await
            .expect("Failed to connect subscriber2");

        let channel = "test:pubsub:multiple";
        let (tx1, mut rx1) = tokio::sync::mpsc::channel::<String>(10);
        let (tx2, mut rx2) = tokio::sync::mpsc::channel::<String>(10);

        // Subscribe both
        let mut stream1 = subscriber1.message_rx();
        let mut stream2 = subscriber2.message_rx();
        subscriber1
            .subscribe(channel)
            .await
            .expect("Failed to SUBSCRIBE");
        subscriber2
            .subscribe(channel)
            .await
            .expect("Failed to SUBSCRIBE");

        // Spawn receive tasks
        let ch1 = channel.to_string();
        tokio::spawn(async move {
            while let Ok(msg) = stream1.recv().await {
                if msg.channel == ch1 {
                    if let Ok(value) = String::from_utf8(msg.value.as_bytes().unwrap().to_vec()) {
                        let _ = tx1.send(value).await;
                    }
                }
            }
        });

        let ch2 = channel.to_string();
        tokio::spawn(async move {
            while let Ok(msg) = stream2.recv().await {
                if msg.channel == ch2 {
                    if let Ok(value) = String::from_utf8(msg.value.as_bytes().unwrap().to_vec()) {
                        let _ = tx2.send(value).await;
                    }
                }
            }
        });

        sleep(Duration::from_millis(100)).await;

        // Publish message
        let _: i64 = publisher
            .publish(channel, "Broadcast message")
            .await
            .expect("Failed to PUBLISH");

        // Both subscribers should receive it
        let msg1 = tokio::time::timeout(Duration::from_secs(2), rx1.recv())
            .await
            .expect("Timeout on subscriber 1")
            .expect("Channel 1 closed");
        let msg2 = tokio::time::timeout(Duration::from_secs(2), rx2.recv())
            .await
            .expect("Timeout on subscriber 2")
            .expect("Channel 2 closed");

        assert_eq!(msg1, "Broadcast message");
        assert_eq!(msg2, "Broadcast message");

        // Cleanup
        subscriber1
            .unsubscribe(channel)
            .await
            .expect("Failed to UNSUBSCRIBE");
        subscriber2
            .unsubscribe(channel)
            .await
            .expect("Failed to UNSUBSCRIBE");
    }

    // ========================================================================
    // Session/Cache Pattern Tests
    // ========================================================================

    #[tokio::test]
    async fn test_session_cache_pattern() {
        let client = create_test_redis().await;
        let session_id = Uuid::new_v4();
        let key = format!("session:{session_id}");

        // Store session data as hash
        client
            .hset::<(), _, _>(&key, ("user_id", "12345"))
            .await
            .expect("Failed to HSET user_id");
        client
            .hset::<(), _, _>(&key, ("username", "testuser"))
            .await
            .expect("Failed to HSET username");
        client
            .hset::<(), _, _>(&key, ("ip", "192.168.1.1"))
            .await
            .expect("Failed to HSET ip");

        // Set session expiry (15 minutes)
        let _: i64 = client
            .expire(&key, 900, None)
            .await
            .expect("Failed to EXPIRE");

        // Retrieve session
        let user_id: String = client
            .hget(&key, "user_id")
            .await
            .expect("Failed to HGET user_id");
        assert_eq!(user_id, "12345");

        // Check TTL
        let ttl: i64 = client.ttl(&key).await.expect("Failed to TTL");
        assert!(ttl > 0 && ttl <= 900);

        cleanup_key(&client, &key).await;
    }

    #[tokio::test]
    async fn test_rate_limiting_pattern() {
        let client = create_test_redis().await;
        let user_id = Uuid::new_v4();
        let key = format!("rate_limit:user:{user_id}");

        // Simulate rate limiting (max 5 requests per 10 seconds)
        for i in 1..=5 {
            let count: i64 = client.incr(&key).await.expect("Failed to INCR");
            assert_eq!(count, i);

            // Set expiry on first request
            if i == 1 {
                let _: i64 = client
                    .expire(&key, 10, None)
                    .await
                    .expect("Failed to EXPIRE");
            }
        }

        // 6th request should see count at 6
        let count: i64 = client.incr(&key).await.expect("Failed to INCR");
        assert_eq!(count, 6);

        // In real app, would check if count > 5 and reject request

        cleanup_key(&client, &key).await;
    }

    #[tokio::test]
    async fn test_active_users_tracking() {
        let client = create_test_redis().await;
        let channel_id = Uuid::new_v4();
        let key = format!("voice:channel:{channel_id}:users");

        let user1 = Uuid::new_v4().to_string();
        let user2 = Uuid::new_v4().to_string();
        let user3 = Uuid::new_v4().to_string();

        // Add active users to channel
        let _: i64 = client.sadd(&key, &user1).await.expect("Failed to SADD");
        let _: i64 = client.sadd(&key, &user2).await.expect("Failed to SADD");
        let _: i64 = client.sadd(&key, &user3).await.expect("Failed to SADD");

        // Get active user count
        let count: i64 = client.scard(&key).await.expect("Failed to SCARD");
        assert_eq!(count, 3);

        // Remove a user
        let _: i64 = client.srem(&key, &user2).await.expect("Failed to SREM");

        // Verify count decreased
        let count: i64 = client.scard(&key).await.expect("Failed to SCARD");
        assert_eq!(count, 2);

        // Check if specific user is active
        let is_active: bool = client
            .sismember(&key, &user1)
            .await
            .expect("Failed to SISMEMBER");
        assert!(is_active);

        let is_active: bool = client
            .sismember(&key, &user2)
            .await
            .expect("Failed to SISMEMBER");
        assert!(!is_active);

        cleanup_key(&client, &key).await;
    }

    #[tokio::test]
    async fn test_message_queue_pattern() {
        let client = create_test_redis().await;
        let queue_key = "test:queue:events";

        // Producer: Add events to queue
        let _: i64 = client
            .rpush(queue_key, "event:user_joined:123")
            .await
            .expect("Failed to RPUSH");
        let _: i64 = client
            .rpush(queue_key, "event:message_sent:456")
            .await
            .expect("Failed to RPUSH");
        let _: i64 = client
            .rpush(queue_key, "event:user_left:789")
            .await
            .expect("Failed to RPUSH");

        // Consumer: Process events from queue
        let event1: String = client.lpop(queue_key, None).await.expect("Failed to LPOP");
        assert_eq!(event1, "event:user_joined:123");

        let event2: String = client.lpop(queue_key, None).await.expect("Failed to LPOP");
        assert_eq!(event2, "event:message_sent:456");

        // Check remaining queue size
        let len: i64 = client.llen(queue_key).await.expect("Failed to LLEN");
        assert_eq!(len, 1);

        cleanup_key(&client, queue_key).await;
    }

    #[tokio::test]
    async fn test_cached_user_data() {
        let client = create_test_redis().await;
        let user_id = Uuid::new_v4();
        let cache_key = format!("cache:user:{user_id}");

        // Simulate caching user data (would normally come from DB)
        let user_data = serde_json::json!({
            "id": user_id.to_string(),
            "username": "cached_user",
            "display_name": "Cached User",
            "status": "online"
        });

        // Cache the data with 5 minute TTL
        client
            .set::<(), _, _>(
                &cache_key,
                user_data.to_string(),
                Some(Expiration::EX(300)),
                None,
                false,
            )
            .await
            .expect("Failed to cache user data");

        // Retrieve from cache
        let cached: String = client
            .get(&cache_key)
            .await
            .expect("Failed to GET cached data");
        let parsed: serde_json::Value =
            serde_json::from_str(&cached).expect("Failed to parse JSON");

        assert_eq!(parsed["username"], "cached_user");
        assert_eq!(parsed["status"], "online");

        cleanup_key(&client, &cache_key).await;
    }
}
