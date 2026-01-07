//! Channel Management Handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::AppState,
    auth::AuthUser,
    db::{self, ChannelType},
};

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug)]
pub enum ChannelError {
    NotFound,
    Forbidden,
    Validation(String),
    Database(sqlx::Error),
}

impl IntoResponse for ChannelError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Channel not found"),
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

impl From<sqlx::Error> for ChannelError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(err)
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ChannelResponse {
    pub id: Uuid,
    pub name: String,
    pub channel_type: String,
    pub category_id: Option<Uuid>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
    pub position: i32,
}

impl From<db::Channel> for ChannelResponse {
    fn from(ch: db::Channel) -> Self {
        Self {
            id: ch.id,
            name: ch.name,
            channel_type: match ch.channel_type {
                ChannelType::Text => "text".to_string(),
                ChannelType::Voice => "voice".to_string(),
                ChannelType::Dm => "dm".to_string(),
            },
            category_id: ch.category_id,
            topic: ch.topic,
            user_limit: ch.user_limit,
            position: ch.position,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChannelRequest {
    #[validate(length(min = 1, max = 64, message = "Name must be 1-64 characters"))]
    pub name: String,
    pub channel_type: String,
    pub category_id: Option<Uuid>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateChannelRequest {
    #[validate(length(min = 1, max = 64, message = "Name must be 1-64 characters"))]
    pub name: Option<String>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub user_id: Uuid,
    pub role_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MemberResponse {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

/// List all channels.
/// GET /api/channels
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<ChannelResponse>>, ChannelError> {
    let channels = db::list_channels(&state.db).await?;
    let response: Vec<ChannelResponse> = channels.into_iter().map(Into::into).collect();
    Ok(Json(response))
}

/// Create a new channel.
/// POST /api/channels
pub async fn create(
    State(state): State<AppState>,
    _auth_user: AuthUser,
    Json(body): Json<CreateChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), ChannelError> {
    // Validate input
    body.validate()
        .map_err(|e| ChannelError::Validation(e.to_string()))?;

    // Parse channel type
    let channel_type = match body.channel_type.to_lowercase().as_str() {
        "text" => ChannelType::Text,
        "voice" => ChannelType::Voice,
        "dm" => ChannelType::Dm,
        _ => return Err(ChannelError::Validation("Invalid channel type".to_string())),
    };

    // Validate voice channel user limit
    if channel_type == ChannelType::Voice {
        if let Some(limit) = body.user_limit {
            if limit < 1 || limit > 99 {
                return Err(ChannelError::Validation(
                    "User limit must be between 1 and 99".to_string(),
                ));
            }
        }
    }

    let channel = db::create_channel(
        &state.db,
        &body.name,
        &channel_type,
        body.category_id,
        body.topic.as_deref(),
        body.user_limit,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(channel.into())))
}

/// Get a channel by ID.
/// GET /api/channels/:id
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ChannelResponse>, ChannelError> {
    let channel = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    Ok(Json(channel.into()))
}

/// Update a channel.
/// PATCH /api/channels/:id
pub async fn update(
    State(state): State<AppState>,
    _auth_user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateChannelRequest>,
) -> Result<Json<ChannelResponse>, ChannelError> {
    // Validate input
    body.validate()
        .map_err(|e| ChannelError::Validation(e.to_string()))?;

    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    let channel = db::update_channel(
        &state.db,
        id,
        body.name.as_deref(),
        body.topic.as_deref(),
        body.user_limit,
        body.position,
    )
    .await?
    .ok_or(ChannelError::NotFound)?;

    Ok(Json(channel.into()))
}

/// Delete a channel.
/// DELETE /api/channels/:id
pub async fn delete(
    State(state): State<AppState>,
    _auth_user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ChannelError> {
    let deleted = db::delete_channel(&state.db, id).await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChannelError::NotFound)
    }
}

/// List members of a channel.
/// GET /api/channels/:id/members
pub async fn list_members(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<MemberResponse>>, ChannelError> {
    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    let users = db::list_channel_members_with_users(&state.db, id).await?;

    let response: Vec<MemberResponse> = users
        .into_iter()
        .map(|u| MemberResponse {
            user_id: u.id,
            username: u.username,
            display_name: u.display_name,
            avatar_url: u.avatar_url,
        })
        .collect();

    Ok(Json(response))
}

/// Add a member to a channel.
/// POST /api/channels/:id/members
pub async fn add_member(
    State(state): State<AppState>,
    _auth_user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<StatusCode, ChannelError> {
    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    // Check user exists
    let _ = db::find_user_by_id(&state.db, body.user_id)
        .await?
        .ok_or(ChannelError::Validation("User not found".to_string()))?;

    db::add_channel_member(&state.db, id, body.user_id, body.role_id).await?;

    Ok(StatusCode::CREATED)
}

/// Remove a member from a channel.
/// DELETE /api/channels/:id/members/:user_id
pub async fn remove_member(
    State(state): State<AppState>,
    _auth_user: AuthUser,
    Path((channel_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ChannelError> {
    let removed = db::remove_channel_member(&state.db, channel_id, user_id).await?;

    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChannelError::NotFound)
    }
}
