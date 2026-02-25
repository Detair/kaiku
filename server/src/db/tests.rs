//! Database Integration Tests
//!
//! Comprehensive tests for `PostgreSQL` operations.

#[cfg(test)]
mod postgres_tests {
    use sqlx::PgPool;

    use super::super::*;

    // ========================================================================
    // User Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_create_and_find_user(pool: PgPool) {
        let username = "testuser";
        let display_name = "Test User";
        let email = Some("test@example.com");
        let password_hash = "hashed_password_123";

        // Create user
        let user = create_user(&pool, username, display_name, email, password_hash)
            .await
            .expect("Failed to create user");

        assert_eq!(user.username, username);
        assert_eq!(user.display_name, display_name);
        assert_eq!(user.email.as_deref(), Some("test@example.com"));
        assert_eq!(user.auth_method, AuthMethod::Local);
        assert_eq!(user.status, UserStatus::Offline);

        // Find by ID
        let found = find_user_by_id(&pool, user.id)
            .await
            .expect("Query failed")
            .expect("User not found");
        assert_eq!(found.id, user.id);
        assert_eq!(found.username, username);

        // Find by username
        let found = find_user_by_username(&pool, username)
            .await
            .expect("Query failed")
            .expect("User not found");
        assert_eq!(found.username, username);

        // Find by email
        let found = find_user_by_email(&pool, "test@example.com")
            .await
            .expect("Query failed")
            .expect("User not found");
        assert_eq!(found.email.as_deref(), Some("test@example.com"));
    }

    #[sqlx::test]
    async fn test_username_uniqueness(pool: PgPool) {
        let username = "duplicate_user";
        let password_hash = "hash123";

        // Create first user
        create_user(&pool, username, "User One", None, password_hash)
            .await
            .expect("Failed to create first user");

        // Try to create duplicate
        let result = create_user(&pool, username, "User Two", None, password_hash).await;
        assert!(result.is_err(), "Should fail on duplicate username");
    }

    #[sqlx::test]
    async fn test_username_exists_check(pool: PgPool) {
        let username = "existcheck";
        let password_hash = "hash456";

        // Should not exist initially
        let exists = username_exists(&pool, username)
            .await
            .expect("Query failed");
        assert!(!exists);

        // Create user
        create_user(&pool, username, "Display", None, password_hash)
            .await
            .expect("Failed to create user");

        // Should exist now
        let exists = username_exists(&pool, username)
            .await
            .expect("Query failed");
        assert!(exists);
    }

    #[sqlx::test]
    async fn test_email_exists_check(pool: PgPool) {
        let email = "unique@example.com";
        let password_hash = "hash789";

        // Should not exist initially
        let exists = email_exists(&pool, email).await.expect("Query failed");
        assert!(!exists);

        // Create user with email
        create_user(
            &pool,
            "userwithemail",
            "Display",
            Some(email),
            password_hash,
        )
        .await
        .expect("Failed to create user");

        // Should exist now
        let exists = email_exists(&pool, email).await.expect("Query failed");
        assert!(exists);
    }

    #[sqlx::test]
    async fn test_update_mfa_secret(pool: PgPool) {
        // Create user
        let user = create_user(&pool, "mfauser", "MFA User", None, "hash123")
            .await
            .expect("Failed to create user");

        // Set MFA secret
        set_mfa_secret(&pool, user.id, Some("secret_mfa_key"))
            .await
            .expect("Failed to set MFA secret");

        // Verify MFA secret was set
        let updated = find_user_by_id(&pool, user.id)
            .await
            .expect("Query failed")
            .expect("User not found");
        assert_eq!(updated.mfa_secret.as_deref(), Some("secret_mfa_key"));

        // Clear MFA secret
        set_mfa_secret(&pool, user.id, None)
            .await
            .expect("Failed to clear MFA secret");

        let cleared = find_user_by_id(&pool, user.id)
            .await
            .expect("Query failed")
            .expect("User not found");
        assert!(cleared.mfa_secret.is_none());
    }

    // ========================================================================
    // Session Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_session_lifecycle(pool: PgPool) {
        use chrono::{Duration, Utc};

        // Create user
        let user = create_user(&pool, "sessionuser", "Session User", None, "hash")
            .await
            .expect("Failed to create user");

        let token_hash = "token_hash_abc123";
        let expires_at = Utc::now() + Duration::hours(1);

        // Create session
        let session = create_session(
            &pool,
            user.id,
            token_hash,
            expires_at,
            Some("192.168.1.1"),
            Some("Mozilla/5.0"),
        )
        .await
        .expect("Failed to create session");

        assert_eq!(session.user_id, user.id);
        assert_eq!(session.token_hash, token_hash);
        assert_eq!(session.ip_address.as_deref(), Some("192.168.1.1"));
        assert_eq!(session.user_agent.as_deref(), Some("Mozilla/5.0"));

        // Find session by token hash
        let found = find_session_by_token_hash(&pool, token_hash)
            .await
            .expect("Query failed")
            .expect("Session not found");
        assert_eq!(found.id, session.id);

        // Delete session
        delete_session(&pool, session.id)
            .await
            .expect("Failed to delete session");

        // Should not find deleted session
        let not_found = find_session_by_token_hash(&pool, token_hash)
            .await
            .expect("Query failed");
        assert!(not_found.is_none());
    }

    #[sqlx::test]
    async fn test_delete_all_user_sessions(pool: PgPool) {
        use chrono::{Duration, Utc};

        // Create user
        let user = create_user(&pool, "multisession", "Multi Session", None, "hash")
            .await
            .expect("Failed to create user");

        let expires_at = Utc::now() + Duration::hours(1);

        // Create multiple sessions
        create_session(&pool, user.id, "token1", expires_at, None, None)
            .await
            .expect("Failed to create session 1");
        create_session(&pool, user.id, "token2", expires_at, None, None)
            .await
            .expect("Failed to create session 2");
        create_session(&pool, user.id, "token3", expires_at, None, None)
            .await
            .expect("Failed to create session 3");

        // Delete all sessions for user
        let deleted_count = delete_all_user_sessions(&pool, user.id)
            .await
            .expect("Failed to delete sessions");
        assert_eq!(deleted_count, 3);

        // Verify all sessions are gone
        assert!(find_session_by_token_hash(&pool, "token1")
            .await
            .unwrap()
            .is_none());
        assert!(find_session_by_token_hash(&pool, "token2")
            .await
            .unwrap()
            .is_none());
        assert!(find_session_by_token_hash(&pool, "token3")
            .await
            .unwrap()
            .is_none());
    }

    #[sqlx::test]
    async fn test_cleanup_expired_sessions(pool: PgPool) {
        use chrono::{Duration, Utc};

        // Create user
        let user = create_user(&pool, "expireuser", "Expire User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create expired session (created 2 hours ago, expired 1 hour ago)
        // Direct SQL insert to bypass constraint by setting both created_at and expires_at
        let created_time = Utc::now() - Duration::hours(2);
        let expired_time = Utc::now() - Duration::hours(1);
        sqlx::query(
            "INSERT INTO sessions (user_id, token_hash, expires_at, created_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(user.id)
        .bind("expired_token")
        .bind(expired_time)
        .bind(created_time)
        .execute(&pool)
        .await
        .expect("Failed to create expired session");

        // Create valid session (1 hour future)
        let valid_time = Utc::now() + Duration::hours(1);
        create_session(&pool, user.id, "valid_token", valid_time, None, None)
            .await
            .expect("Failed to create valid session");

        // Cleanup expired sessions
        let cleaned = cleanup_expired_sessions(&pool)
            .await
            .expect("Failed to cleanup sessions");
        assert_eq!(cleaned, 1);

        // Expired should be gone
        assert!(find_session_by_token_hash(&pool, "expired_token")
            .await
            .unwrap()
            .is_none());

        // Valid should still exist
        assert!(find_session_by_token_hash(&pool, "valid_token")
            .await
            .unwrap()
            .is_some());
    }

    // ========================================================================
    // Channel Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_create_channels(pool: PgPool) {
        // Create text channel
        let text_channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "general",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: Some("General discussion"),
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create text channel");

        assert_eq!(text_channel.name, "general");
        assert_eq!(text_channel.channel_type, ChannelType::Text);
        assert_eq!(text_channel.topic.as_deref(), Some("General discussion"));

        // Create voice channel
        let voice_channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "voice-lobby",
                channel_type: &ChannelType::Voice,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: Some(10),
            },
        )
        .await
        .expect("Failed to create voice channel");

        assert_eq!(voice_channel.name, "voice-lobby");
        assert_eq!(voice_channel.channel_type, ChannelType::Voice);
        assert_eq!(voice_channel.user_limit, Some(10));

        // Verify channels can be found by ID
        let found_text = find_channel_by_id(&pool, text_channel.id)
            .await
            .expect("Failed to find text channel")
            .expect("Text channel not found");
        assert_eq!(found_text.name, "general");

        let found_voice = find_channel_by_id(&pool, voice_channel.id)
            .await
            .expect("Failed to find voice channel")
            .expect("Voice channel not found");
        assert_eq!(found_voice.name, "voice-lobby");
    }

    #[sqlx::test]
    async fn test_find_channel_by_id(pool: PgPool) {
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "test-channel",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let found = find_channel_by_id(&pool, channel.id)
            .await
            .expect("Query failed")
            .expect("Channel not found");
        assert_eq!(found.id, channel.id);
        assert_eq!(found.name, "test-channel");
    }

    #[sqlx::test]
    async fn test_update_channel(pool: PgPool) {
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "old-name",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        // Update channel
        let updated = update_channel(
            &pool,
            channel.id,
            Some("new-name"),
            Some("New topic"),
            None,
            None,
            None, // position
        )
        .await
        .expect("Failed to update channel")
        .expect("Channel not found");

        assert_eq!(updated.name, "new-name");
        assert_eq!(updated.topic.as_deref(), Some("New topic"));
    }

    #[sqlx::test]
    async fn test_delete_channel(pool: PgPool) {
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "to-delete",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let deleted = delete_channel(&pool, channel.id)
            .await
            .expect("Failed to delete channel");
        assert!(deleted);

        // Should not find deleted channel
        let not_found = find_channel_by_id(&pool, channel.id)
            .await
            .expect("Query failed");
        assert!(not_found.is_none());
    }

    // ========================================================================
    // Channel Member Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_channel_members(pool: PgPool) {
        // Create channel and user
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "member-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "memberuser", "Member User", None, "hash")
            .await
            .expect("Failed to create user");

        // Add member
        let member = add_channel_member(&pool, channel.id, user.id, None)
            .await
            .expect("Failed to add member");

        assert_eq!(member.channel_id, channel.id);
        assert_eq!(member.user_id, user.id);

        // Check membership
        let is_member = is_channel_member(&pool, channel.id, user.id)
            .await
            .expect("Query failed");
        assert!(is_member);

        // List members
        let members = list_channel_members(&pool, channel.id)
            .await
            .expect("Failed to list members");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].user_id, user.id);

        // Remove member
        let removed = remove_channel_member(&pool, channel.id, user.id)
            .await
            .expect("Failed to remove member");
        assert!(removed);

        // Check membership again
        let is_member = is_channel_member(&pool, channel.id, user.id)
            .await
            .expect("Query failed");
        assert!(!is_member);
    }

    #[sqlx::test]
    async fn test_list_channel_members_with_users(pool: PgPool) {
        // Create channel
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "user-list-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        // Create multiple users
        let user1 = create_user(&pool, "user1", "User One", None, "hash")
            .await
            .expect("Failed to create user1");
        let user2 = create_user(&pool, "user2", "User Two", None, "hash")
            .await
            .expect("Failed to create user2");

        // Add both users to channel
        add_channel_member(&pool, channel.id, user1.id, None)
            .await
            .expect("Failed to add user1");
        add_channel_member(&pool, channel.id, user2.id, None)
            .await
            .expect("Failed to add user2");

        // List members with user details
        let members = list_channel_members_with_users(&pool, channel.id)
            .await
            .expect("Failed to list members with users");

        assert_eq!(members.len(), 2);
        assert!(members.iter().any(|u| u.id == user1.id));
        assert!(members.iter().any(|u| u.id == user2.id));
    }

    // ========================================================================
    // Message Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_message_lifecycle(pool: PgPool) {
        // Create channel and user
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "msg-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "msguser", "Message User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create message
        let message = create_message(
            &pool,
            channel.id,
            user.id,
            "Hello, World!",
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create message");

        assert_eq!(message.content, "Hello, World!");
        assert_eq!(message.channel_id, channel.id);
        assert_eq!(message.user_id, Some(user.id));
        assert!(!message.encrypted);

        // Find message by ID
        let found = find_message_by_id(&pool, message.id)
            .await
            .expect("Query failed")
            .expect("Message not found");
        assert_eq!(found.id, message.id);

        // Update message
        let updated = update_message(&pool, message.id, user.id, "Updated message")
            .await
            .expect("Failed to update message")
            .expect("Message not found");
        assert_eq!(updated.content, "Updated message");
        assert!(updated.edited_at.is_some());

        // Delete message
        let deleted = delete_message(&pool, message.id, user.id)
            .await
            .expect("Failed to delete message");
        assert!(deleted);

        // Should not find deleted message
        let not_found = find_message_by_id(&pool, message.id)
            .await
            .expect("Query failed");
        assert!(not_found.is_none());
    }

    #[sqlx::test]
    async fn test_list_messages_pagination(pool: PgPool) {
        // Create channel and user
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "pagination-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "paginuser", "Pagination User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create multiple messages
        for i in 1..=5 {
            create_message(
                &pool,
                channel.id,
                user.id,
                &format!("Message {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("Failed to create message");
        }

        // List all messages
        let all_messages = list_messages(&pool, channel.id, None, 100)
            .await
            .expect("Failed to list messages");
        assert_eq!(all_messages.len(), 5);

        // List with limit
        let limited = list_messages(&pool, channel.id, None, 3)
            .await
            .expect("Failed to list messages");
        assert_eq!(limited.len(), 3);

        // List with pagination (before first message)
        let paginated = list_messages(&pool, channel.id, Some(limited[0].id), 2)
            .await
            .expect("Failed to list messages");
        assert!(paginated.len() <= 2);
    }

    #[sqlx::test]
    async fn test_reply_to_message(pool: PgPool) {
        // Create channel and user
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "reply-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "replyuser", "Reply User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create original message
        let original = create_message(
            &pool,
            channel.id,
            user.id,
            "Original message",
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create original message");

        // Create reply
        let reply = create_message(
            &pool,
            channel.id,
            user.id,
            "This is a reply",
            false,
            None,
            Some(original.id),
        )
        .await
        .expect("Failed to create reply");

        assert_eq!(reply.reply_to, Some(original.id));
    }

    #[sqlx::test]
    async fn test_admin_delete_message(pool: PgPool) {
        // Create channel and users
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "admin-del-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "regularuser", "Regular User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create message
        let message = create_message(&pool, channel.id, user.id, "Delete me", false, None, None)
            .await
            .expect("Failed to create message");

        // Admin delete (no user ID check)
        let deleted = admin_delete_message(&pool, message.id)
            .await
            .expect("Failed to admin delete message");
        assert!(deleted);

        // Should not find deleted message
        let not_found = find_message_by_id(&pool, message.id)
            .await
            .expect("Query failed");
        assert!(not_found.is_none());
    }

    // ========================================================================
    // File Attachment Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_file_attachments(pool: PgPool) {
        // Create channel, user, and message
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "attachment-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "attachuser", "Attachment User", None, "hash")
            .await
            .expect("Failed to create user");

        let message = create_message(
            &pool,
            channel.id,
            user.id,
            "Message with attachment",
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create message");

        // Create file attachment
        let attachment = create_file_attachment(
            &pool,
            message.id,
            "document.pdf",
            "application/pdf",
            1024000,
            "uploads/abc123/document.pdf",
            None,
            None,
            None,
            None,
            None,
            "skipped",
        )
        .await
        .expect("Failed to create attachment");

        assert_eq!(attachment.filename, "document.pdf");
        assert_eq!(attachment.mime_type, "application/pdf");
        assert_eq!(attachment.size_bytes, 1024000);

        // Find attachment by ID
        let found = find_file_attachment_by_id(&pool, attachment.id)
            .await
            .expect("Query failed")
            .expect("Attachment not found");
        assert_eq!(found.id, attachment.id);

        // List attachments for message
        let attachments = list_file_attachments_by_message(&pool, message.id)
            .await
            .expect("Failed to list attachments");
        assert_eq!(attachments.len(), 1);

        // Delete attachment
        let deleted = delete_file_attachment(&pool, attachment.id)
            .await
            .expect("Failed to delete attachment")
            .expect("Attachment not found");
        assert_eq!(deleted.id, attachment.id);
    }

    #[sqlx::test]
    async fn test_delete_attachments_by_message(pool: PgPool) {
        // Create channel, user, and message
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "multi-attach-test",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("Failed to create channel");

        let user = create_user(&pool, "multiattachuser", "Multi Attach User", None, "hash")
            .await
            .expect("Failed to create user");

        let message = create_message(
            &pool,
            channel.id,
            user.id,
            "Multiple attachments",
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create message");

        // Create multiple attachments
        create_file_attachment(&pool, message.id, "file1.txt", "text/plain", 100, "path1", None, None, None, None, None, "skipped")
            .await
            .expect("Failed to create attachment 1");
        create_file_attachment(&pool, message.id, "file2.txt", "text/plain", 200, "path2", None, None, None, None, None, "skipped")
            .await
            .expect("Failed to create attachment 2");

        // Delete all attachments for message
        let deleted = delete_file_attachments_by_message(&pool, message.id)
            .await
            .expect("Failed to delete attachments");
        assert_eq!(deleted.len(), 2);

        // Verify all deleted
        let remaining = list_file_attachments_by_message(&pool, message.id)
            .await
            .expect("Failed to list attachments");
        assert_eq!(remaining.len(), 0);
    }

    // ========================================================================
    // Unread Aggregation Tests
    // ========================================================================

    #[sqlx::test]
    async fn test_get_unread_aggregate_empty(pool: PgPool) {
        // Create a user with no guild memberships or DMs
        let user = create_user(&pool, "lonely", "Lonely User", None, "hash")
            .await
            .expect("create user");

        let result = get_unread_aggregate(&pool, user.id)
            .await
            .expect("get_unread_aggregate");

        assert!(result.guilds.is_empty());
        assert!(result.dms.is_empty());
        assert_eq!(result.total, 0);
    }

    #[sqlx::test]
    async fn test_get_unread_aggregate_guild_unreads(pool: PgPool) {
        // Create owner and guild
        let owner = create_user(&pool, "guildowner", "Guild Owner", None, "hash")
            .await
            .expect("create owner");

        let guild_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
            .bind(guild_id)
            .bind("Test Guild")
            .bind(owner.id)
            .execute(&pool)
            .await
            .expect("create guild");

        // Add owner as guild member
        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(guild_id)
            .bind(owner.id)
            .execute(&pool)
            .await
            .expect("join guild");

        // Create a text channel in the guild
        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "general",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("create channel");

        // Create another user who posts messages
        let sender = create_user(&pool, "sender", "Sender", None, "hash")
            .await
            .expect("create sender");

        // Post 3 messages from sender
        for i in 1..=3 {
            create_message(
                &pool,
                channel.id,
                sender.id,
                &format!("Message {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("create message");
        }

        // Owner has never read the channel, so all 3 should be unread
        let result = get_unread_aggregate(&pool, owner.id)
            .await
            .expect("get_unread_aggregate");

        assert_eq!(result.guilds.len(), 1);
        assert_eq!(result.guilds[0].guild_id, guild_id);
        assert_eq!(result.guilds[0].guild_name, "Test Guild");
        assert_eq!(result.guilds[0].channels.len(), 1);
        assert_eq!(result.guilds[0].channels[0].channel_id, channel.id);
        assert_eq!(result.guilds[0].channels[0].unread_count, 3);
        assert_eq!(result.guilds[0].total_unread, 3);
        assert_eq!(result.total, 3);
        assert!(result.dms.is_empty());
    }

    #[sqlx::test]
    async fn test_get_unread_aggregate_no_unreads_after_read(pool: PgPool) {
        // Create owner and guild
        let owner = create_user(&pool, "reader", "Reader", None, "hash")
            .await
            .expect("create owner");

        let guild_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO guilds (id, name, owner_id) VALUES ($1, $2, $3)")
            .bind(guild_id)
            .bind("Read Guild")
            .bind(owner.id)
            .execute(&pool)
            .await
            .expect("create guild");

        sqlx::query("INSERT INTO guild_members (guild_id, user_id) VALUES ($1, $2)")
            .bind(guild_id)
            .bind(owner.id)
            .execute(&pool)
            .await
            .expect("join guild");

        let channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "updates",
                channel_type: &ChannelType::Text,
                category_id: None,
                guild_id: Some(guild_id),
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("create channel");

        // Another user sends messages
        let sender = create_user(&pool, "sender2", "Sender 2", None, "hash")
            .await
            .expect("create sender");

        for i in 1..=2 {
            create_message(
                &pool,
                channel.id,
                sender.id,
                &format!("Update {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("create message");
        }

        // Mark channel as read by inserting a channel_read_state with current timestamp
        sqlx::query(
            "INSERT INTO channel_read_state (user_id, channel_id, last_read_at) VALUES ($1, $2, NOW())",
        )
        .bind(owner.id)
        .bind(channel.id)
        .execute(&pool)
        .await
        .expect("mark as read");

        // After marking as read, no unreads should appear
        let result = get_unread_aggregate(&pool, owner.id)
            .await
            .expect("get_unread_aggregate");

        assert!(result.guilds.is_empty());
        assert_eq!(result.total, 0);
    }

    #[sqlx::test]
    async fn test_get_unread_aggregate_dm_unreads(pool: PgPool) {
        // Create two users
        let user_a = create_user(&pool, "usera", "User A", None, "hash")
            .await
            .expect("create user_a");
        let user_b = create_user(&pool, "userb", "User B", None, "hash")
            .await
            .expect("create user_b");

        // Create a DM channel
        let dm_channel = create_channel(
            &pool,
            CreateChannelParams {
                name: "dm-channel",
                channel_type: &ChannelType::Dm,
                category_id: None,
                guild_id: None,
                topic: None,
                icon_url: None,
                user_limit: None,
            },
        )
        .await
        .expect("create dm channel");

        // Add both users as DM participants
        sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)")
            .bind(dm_channel.id)
            .bind(user_a.id)
            .execute(&pool)
            .await
            .expect("add participant a");

        sqlx::query("INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)")
            .bind(dm_channel.id)
            .bind(user_b.id)
            .execute(&pool)
            .await
            .expect("add participant b");

        // User B sends 2 messages to the DM
        for i in 1..=2 {
            create_message(
                &pool,
                dm_channel.id,
                user_b.id,
                &format!("DM from B {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("create dm message from B");
        }

        // User A sends 1 message (should NOT count as unread for user A)
        create_message(
            &pool,
            dm_channel.id,
            user_a.id,
            "DM from A",
            false,
            None,
            None,
        )
        .await
        .expect("create dm message from A");

        // Check unreads for user A: should see 2 unread (only B's messages)
        let result = get_unread_aggregate(&pool, user_a.id)
            .await
            .expect("get_unread_aggregate");

        assert!(result.guilds.is_empty());
        assert_eq!(result.dms.len(), 1);
        assert_eq!(result.dms[0].channel_id, dm_channel.id);
        assert_eq!(result.dms[0].unread_count, 2);
        assert_eq!(result.total, 2);
    }
}
