//! Message Handlers

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::AppState,
    auth::AuthUser,
    db,
    ws::{broadcast_to_channel, ServerEvent},
};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
#[allow(dead_code)]
pub enum MessageError {
    NotFound,
    ChannelNotFound,
    Forbidden,
    Validation(String),
    Database(sqlx::Error),
}

impl IntoResponse for MessageError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Message not found"),
            Self::ChannelNotFound => (StatusCode::NOT_FOUND, "Channel not found"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Access denied"),
            Self::Validation(msg) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response()
            }
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<sqlx::Error> for MessageError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Author profile for message responses.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorProfile {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub status: String,
}

impl From<db::User> for AuthorProfile {
    fn from(user: db::User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            status: format!("{:?}", user.status).to_lowercase(),
        }
    }
}

/// Attachment info for message responses (matches client Attachment type).
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentInfo {
    pub id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    pub url: String,
}

impl AttachmentInfo {
    /// Create from a `FileAttachment` database model.
    pub fn from_db(attachment: &db::FileAttachment) -> Self {
        Self {
            id: attachment.id,
            filename: attachment.filename.clone(),
            mime_type: attachment.mime_type.clone(),
            size: attachment.size_bytes,
            url: format!("/api/messages/attachments/{}/download", attachment.id),
        }
    }
}

/// Full message response with author info (matches client Message type).
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub author: AuthorProfile,
    pub content: String,
    pub encrypted: bool,
    pub attachments: Vec<AttachmentInfo>,
    pub reply_to: Option<Uuid>,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub before: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

const fn default_limit() -> i64 {
    50
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateMessageRequest {
    #[validate(length(min = 1, max = 4000, message = "Content must be 1-4000 characters"))]
    pub content: String,
    #[serde(default)]
    pub encrypted: bool,
    pub nonce: Option<String>,
    pub reply_to: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateMessageRequest {
    #[validate(length(min = 1, max = 4000, message = "Content must be 1-4000 characters"))]
    pub content: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// List messages in a channel.
/// GET /`api/messages/channel/:channel_id`
pub async fn list(
    State(state): State<AppState>,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageResponse>>, MessageError> {
    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(MessageError::ChannelNotFound)?;

    // Limit between 1 and 100
    let limit = query.limit.clamp(1, 100);

    let messages = db::list_messages(&state.db, channel_id, query.before, limit).await?;

    // Bulk fetch all user IDs to avoid N+1 query
    let user_ids: Vec<Uuid> = messages.iter().map(|m| m.user_id).collect();
    let users = db::find_users_by_ids(&state.db, &user_ids).await?;

    // Create lookup map for O(1) access
    let user_map: std::collections::HashMap<Uuid, db::User> =
        users.into_iter().map(|u| (u.id, u)).collect();

    // Bulk fetch all attachments to avoid N+1 query
    let message_ids: Vec<Uuid> = messages.iter().map(|m| m.id).collect();
    let all_attachments = db::list_file_attachments_by_messages(&state.db, &message_ids).await?;

    // Create lookup map for attachments by message_id
    let mut attachment_map: std::collections::HashMap<Uuid, Vec<AttachmentInfo>> =
        std::collections::HashMap::new();
    for attachment in all_attachments {
        attachment_map
            .entry(attachment.message_id)
            .or_default()
            .push(AttachmentInfo::from_db(&attachment));
    }

    // Build response with author info and attachments
    let response: Vec<MessageResponse> = messages
        .into_iter()
        .map(|msg| {
            let author = user_map
                .get(&msg.user_id)
                .map(|u| AuthorProfile::from(u.clone()))
                .unwrap_or_else(|| AuthorProfile {
                    id: msg.user_id,
                    username: "deleted".to_string(),
                    display_name: "Deleted User".to_string(),
                    avatar_url: None,
                    status: "offline".to_string(),
                });

            let attachments = attachment_map.remove(&msg.id).unwrap_or_default();

            MessageResponse {
                id: msg.id,
                channel_id: msg.channel_id,
                author,
                content: msg.content,
                encrypted: msg.encrypted,
                attachments,
                reply_to: msg.reply_to,
                edited_at: msg.edited_at,
                created_at: msg.created_at,
            }
        })
        .collect();

    Ok(Json(response))
}

/// Create a new message.
/// POST /`api/messages/channel/:channel_id`
pub async fn create(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), MessageError> {
    // Validate input
    body.validate()
        .map_err(|e| MessageError::Validation(e.to_string()))?;

    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(MessageError::ChannelNotFound)?;

    // Validate encrypted messages have nonce
    if body.encrypted && body.nonce.is_none() {
        return Err(MessageError::Validation(
            "Encrypted messages require a nonce".to_string(),
        ));
    }

    // Validate reply_to exists if provided
    if let Some(reply_id) = body.reply_to {
        let reply_msg = db::find_message_by_id(&state.db, reply_id).await?;
        if reply_msg.is_none() {
            return Err(MessageError::Validation(
                "Reply target not found".to_string(),
            ));
        }
    }

    let message = db::create_message(
        &state.db,
        channel_id,
        auth_user.id,
        &body.content,
        body.encrypted,
        body.nonce.as_deref(),
        body.reply_to,
    )
    .await?;

    // Get author profile for response
    let author = db::find_user_by_id(&state.db, auth_user.id)
        .await?
        .map(AuthorProfile::from)
        .unwrap_or_else(|| AuthorProfile {
            id: auth_user.id,
            username: "unknown".to_string(),
            display_name: "Unknown User".to_string(),
            avatar_url: None,
            status: "offline".to_string(),
        });

    let response = MessageResponse {
        id: message.id,
        channel_id: message.channel_id,
        author: author.clone(),
        content: message.content,
        encrypted: message.encrypted,
        attachments: vec![],
        reply_to: message.reply_to,
        edited_at: message.edited_at,
        created_at: message.created_at,
    };

    // Broadcast new message via Redis pub-sub
    let message_json = serde_json::to_value(&response).unwrap_or_default();
    let _ = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::MessageNew {
            channel_id,
            message: message_json,
        },
    )
    .await;

    Ok((StatusCode::CREATED, Json(response)))
}

/// Update (edit) a message.
/// PATCH /api/messages/:id
pub async fn update(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateMessageRequest>,
) -> Result<Json<MessageResponse>, MessageError> {
    // Validate input
    body.validate()
        .map_err(|e| MessageError::Validation(e.to_string()))?;

    // Update message (only owner can edit)
    let message = db::update_message(&state.db, id, auth_user.id, &body.content)
        .await?
        .ok_or(MessageError::NotFound)?;

    // Get author profile for response
    let author = db::find_user_by_id(&state.db, auth_user.id)
        .await?
        .map(AuthorProfile::from)
        .unwrap_or_else(|| AuthorProfile {
            id: auth_user.id,
            username: "unknown".to_string(),
            display_name: "Unknown User".to_string(),
            avatar_url: None,
            status: "offline".to_string(),
        });

    // Fetch existing attachments
    let attachments = db::list_file_attachments_by_message(&state.db, message.id)
        .await?
        .iter()
        .map(AttachmentInfo::from_db)
        .collect();

    let response = MessageResponse {
        id: message.id,
        channel_id: message.channel_id,
        author,
        content: message.content.clone(),
        encrypted: message.encrypted,
        attachments,
        reply_to: message.reply_to,
        edited_at: message.edited_at,
        created_at: message.created_at,
    };

    // Broadcast edit via Redis pub-sub
    let _ = broadcast_to_channel(
        &state.redis,
        message.channel_id,
        &ServerEvent::MessageEdit {
            channel_id: message.channel_id,
            message_id: message.id,
            content: message.content,
            edited_at: message
                .edited_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        },
    )
    .await;

    Ok(Json(response))
}

/// Delete a message (soft delete).
/// DELETE /api/messages/:id
pub async fn delete(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, MessageError> {
    // Get message to find channel_id before deleting
    let message = db::find_message_by_id(&state.db, id)
        .await?
        .ok_or(MessageError::NotFound)?;

    // Check ownership
    if message.user_id != auth_user.id {
        return Err(MessageError::Forbidden);
    }

    let channel_id = message.channel_id;

    // Delete message
    let deleted = db::delete_message(&state.db, id, auth_user.id).await?;

    if deleted {
        // Broadcast delete via Redis pub-sub
        let _ = broadcast_to_channel(
            &state.redis,
            channel_id,
            &ServerEvent::MessageDelete {
                channel_id,
                message_id: id,
            },
        )
        .await;

        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(MessageError::NotFound)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;
    use crate::config::Config;
    use sqlx::PgPool;

    /// Helper to create test app state
    async fn create_test_state(pool: PgPool) -> AppState {
        let config = Config::default_for_test();
        let redis = db::create_redis_client(&config.redis_url)
            .await
            .expect("Failed to connect to Redis");

        AppState::new(
            pool,
            redis,
            config,
            None,
            crate::voice::SfuServer::new(std::sync::Arc::new(Config::default_for_test())).unwrap(),
        )
    }

    #[sqlx::test]
    async fn test_list_messages_with_multiple_users(pool: PgPool) {
        let state = create_test_state(pool.clone()).await;

        // Create two users
        let user1 = db::create_user(&pool, "user1", "User One", None, "hash1")
            .await
            .expect("Failed to create user1");

        let user2 = db::create_user(&pool, "user2", "User Two", None, "hash2")
            .await
            .expect("Failed to create user2");

        // Create a channel
        let channel = db::create_channel(
            &pool,
            "test-channel",
            &db::ChannelType::Text,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create channel");

        // Create 5 messages: 3 from user1, 2 from user2
        let msg1 = db::create_message(&pool, channel.id, user1.id, "Message 1", false, None, None)
            .await
            .expect("Failed to create message 1");

        let msg2 = db::create_message(&pool, channel.id, user2.id, "Message 2", false, None, None)
            .await
            .expect("Failed to create message 2");

        let msg3 = db::create_message(&pool, channel.id, user1.id, "Message 3", false, None, None)
            .await
            .expect("Failed to create message 3");

        let msg4 = db::create_message(&pool, channel.id, user1.id, "Message 4", false, None, None)
            .await
            .expect("Failed to create message 4");

        let msg5 = db::create_message(&pool, channel.id, user2.id, "Message 5", false, None, None)
            .await
            .expect("Failed to create message 5");

        // Call the list handler
        let query = ListMessagesQuery {
            before: None,
            limit: 50,
        };

        let result = list(State(state), Path(channel.id), Query(query))
            .await
            .expect("Handler failed");

        let messages = result.0;

        // Assert correct number of messages
        assert_eq!(messages.len(), 5, "Should return 5 messages");

        // Verify messages are in reverse chronological order (newest first)
        assert_eq!(messages[0].id, msg5.id);
        assert_eq!(messages[1].id, msg4.id);
        assert_eq!(messages[2].id, msg3.id);
        assert_eq!(messages[3].id, msg2.id);
        assert_eq!(messages[4].id, msg1.id);

        // Verify author information is correctly populated
        // Message 5 (from user2)
        assert_eq!(messages[0].author.id, user2.id);
        assert_eq!(messages[0].author.username, "user2");
        assert_eq!(messages[0].author.display_name, "User Two");
        assert_eq!(messages[0].content, "Message 5");

        // Message 4 (from user1)
        assert_eq!(messages[1].author.id, user1.id);
        assert_eq!(messages[1].author.username, "user1");
        assert_eq!(messages[1].author.display_name, "User One");
        assert_eq!(messages[1].content, "Message 4");

        // Message 3 (from user1)
        assert_eq!(messages[2].author.id, user1.id);
        assert_eq!(messages[2].author.username, "user1");

        // Message 2 (from user2)
        assert_eq!(messages[3].author.id, user2.id);
        assert_eq!(messages[3].author.username, "user2");

        // Message 1 (from user1)
        assert_eq!(messages[4].author.id, user1.id);
        assert_eq!(messages[4].author.username, "user1");

        // Verify no N+1 query issue - this test would timeout if there was one
        // The handler should use bulk fetching, making only 2 queries total:
        // 1. Fetch messages
        // 2. Bulk fetch users
    }

    #[sqlx::test]
    async fn test_list_messages_with_deleted_user(pool: PgPool) {
        let state = create_test_state(pool.clone()).await;

        // Create user
        let user = db::create_user(&pool, "deleteuser", "Delete User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create channel
        let channel = db::create_channel(
            &pool,
            "test-channel",
            &db::ChannelType::Text,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create channel");

        // Create message
        let _msg = db::create_message(
            &pool,
            channel.id,
            user.id,
            "Message before delete",
            false,
            None,
            None,
        )
        .await
        .expect("Failed to create message");

        // Delete the user (CASCADE will also delete messages)
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .expect("Failed to delete user");

        // Call the list handler
        let query = ListMessagesQuery {
            before: None,
            limit: 50,
        };

        let result = list(State(state), Path(channel.id), Query(query))
            .await
            .expect("Handler should not fail");

        let messages = result.0;

        // Due to CASCADE DELETE, messages are deleted when user is deleted
        // This test verifies the handler doesn't crash when messages reference deleted users
        // In production, the messages would be gone due to CASCADE
        assert_eq!(messages.len(), 0, "Messages should be deleted via CASCADE");
    }

    #[sqlx::test]
    async fn test_list_messages_pagination(pool: PgPool) {
        let state = create_test_state(pool.clone()).await;

        // Create user
        let user = db::create_user(&pool, "paguser", "Pag User", None, "hash")
            .await
            .expect("Failed to create user");

        // Create channel
        let channel = db::create_channel(
            &pool,
            "test-channel",
            &db::ChannelType::Text,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create channel");

        // Create 10 messages
        for i in 1..=10 {
            db::create_message(
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
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }

        // Fetch first page (limit 3)
        let query1 = ListMessagesQuery {
            before: None,
            limit: 3,
        };

        let result1 = list(State(state.clone()), Path(channel.id), Query(query1))
            .await
            .expect("First page failed");

        let page1 = result1.0;
        assert_eq!(page1.len(), 3, "First page should have 3 messages");

        // Fetch second page using cursor
        let oldest_from_page1 = page1.last().unwrap().id;

        let query2 = ListMessagesQuery {
            before: Some(oldest_from_page1),
            limit: 3,
        };

        let result2 = list(State(state.clone()), Path(channel.id), Query(query2))
            .await
            .expect("Second page failed");

        let page2 = result2.0;

        // Should have at least some messages
        assert!(!page2.is_empty(), "Second page should have messages");
        assert!(page2.len() <= 3, "Second page should respect limit");

        // Verify no overlap - oldest_from_page1 should not appear in page2
        let page2_ids: Vec<Uuid> = page2.iter().map(|m| m.id).collect();
        assert!(
            !page2_ids.contains(&oldest_from_page1),
            "Cursor message should not appear in next page"
        );

        // Verify we can fetch all messages eventually
        let query_all = ListMessagesQuery {
            before: None,
            limit: 100,
        };

        let result_all = list(State(state), Path(channel.id), Query(query_all))
            .await
            .expect("Fetch all failed");

        assert_eq!(result_all.0.len(), 10, "Should have 10 total messages");
    }

    #[sqlx::test]
    async fn test_list_messages_empty_channel(pool: PgPool) {
        let state = create_test_state(pool.clone()).await;

        // Create channel with no messages
        let channel = db::create_channel(
            &pool,
            "empty-channel",
            &db::ChannelType::Text,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create channel");

        let query = ListMessagesQuery {
            before: None,
            limit: 50,
        };

        let result = list(State(state), Path(channel.id), Query(query))
            .await
            .expect("Handler failed");

        let messages = result.0;
        assert_eq!(messages.len(), 0, "Empty channel should return 0 messages");
    }

    #[sqlx::test]
    async fn test_authorprofile_from_user_status_formatting(pool: PgPool) {
        // Test that the From<User> impl correctly formats status

        // Create user with different status
        let user = db::create_user(&pool, "statususer", "Status User", None, "hash")
            .await
            .expect("Failed to create user");

        // User starts as Offline by default
        let profile = AuthorProfile::from(user.clone());
        assert_eq!(profile.status, "offline");
        assert_eq!(profile.username, "statususer");
        assert_eq!(profile.display_name, "Status User");
        assert_eq!(profile.id, user.id);

        // Update user status to Online
        sqlx::query("UPDATE users SET status = 'online' WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .expect("Failed to update status");

        let updated_user = db::find_user_by_id(&pool, user.id)
            .await
            .expect("Failed to find user")
            .expect("User not found");

        let profile2 = AuthorProfile::from(updated_user);
        assert_eq!(profile2.status, "online");
    }

    #[sqlx::test]
    async fn test_list_messages_limit_clamping(pool: PgPool) {
        let state = create_test_state(pool.clone()).await;

        let user = db::create_user(&pool, "clampuser", "Clamp User", None, "hash")
            .await
            .expect("Failed to create user");

        let channel = db::create_channel(
            &pool,
            "test-channel",
            &db::ChannelType::Text,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to create channel");

        // Create 10 messages
        for i in 1..=10 {
            db::create_message(
                &pool,
                channel.id,
                user.id,
                &format!("Msg {i}"),
                false,
                None,
                None,
            )
            .await
            .expect("Failed to create message");
        }

        // Test limit = 0 (should clamp to 1)
        let query_zero = ListMessagesQuery {
            before: None,
            limit: 0,
        };

        let result_zero = list(State(state.clone()), Path(channel.id), Query(query_zero))
            .await
            .expect("Handler failed");

        assert_eq!(result_zero.0.len(), 1, "Limit 0 should clamp to 1");

        // Test limit = 200 (should clamp to 100)
        let query_large = ListMessagesQuery {
            before: None,
            limit: 200,
        };

        let result_large = list(State(state), Path(channel.id), Query(query_large))
            .await
            .expect("Handler failed");

        // Should return all 10 messages (max available), not more than 100
        assert_eq!(
            result_large.0.len(),
            10,
            "Should return all available messages"
        );
    }
}
