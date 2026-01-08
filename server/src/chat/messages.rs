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
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg })))
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

/// Full message response with author info (matches client Message type).
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub author: AuthorProfile,
    pub content: String,
    pub encrypted: bool,
    pub attachments: Vec<()>, // TODO: implement attachments
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

fn default_limit() -> i64 {
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
/// GET /api/messages/channel/:channel_id
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

    // Build response with author info
    let mut response = Vec::with_capacity(messages.len());
    for msg in messages {
        let author = db::find_user_by_id(&state.db, msg.user_id)
            .await?
            .map(|u| AuthorProfile {
                id: u.id,
                username: u.username,
                display_name: u.display_name,
                avatar_url: u.avatar_url,
                status: format!("{:?}", u.status).to_lowercase(),
            })
            .unwrap_or_else(|| AuthorProfile {
                id: msg.user_id,
                username: "deleted".to_string(),
                display_name: "Deleted User".to_string(),
                avatar_url: None,
                status: "offline".to_string(),
            });

        response.push(MessageResponse {
            id: msg.id,
            channel_id: msg.channel_id,
            author,
            content: msg.content,
            encrypted: msg.encrypted,
            attachments: vec![],
            reply_to: msg.reply_to,
            edited_at: msg.edited_at,
            created_at: msg.created_at,
        });
    }

    Ok(Json(response))
}

/// Create a new message.
/// POST /api/messages/channel/:channel_id
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
            return Err(MessageError::Validation("Reply target not found".to_string()));
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
        .map(|u| AuthorProfile {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            avatar_url: u.avatar_url,
            status: format!("{:?}", u.status).to_lowercase(),
        })
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
        .map(|u| AuthorProfile {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            avatar_url: u.avatar_url,
            status: format!("{:?}", u.status).to_lowercase(),
        })
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
        author,
        content: message.content.clone(),
        encrypted: message.encrypted,
        attachments: vec![],
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
            edited_at: message.edited_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
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
