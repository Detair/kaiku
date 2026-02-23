//! Channel Management Handlers

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::ws::{broadcast_to_user, ServerEvent};

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
        let (status, code, message) = match &self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "CHANNEL_NOT_FOUND",
                "Channel not found".to_string(),
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "Access denied".to_string(),
            ),
            Self::Validation(msg) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", msg.clone()),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Database error".to_string(),
            ),
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelResponse {
    pub id: Uuid,
    pub name: String,
    pub channel_type: String,
    pub category_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
    pub position: i32,
    /// Maximum concurrent screen shares (voice channels only).
    pub max_screen_shares: i32,
    pub icon_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
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
            guild_id: ch.guild_id,
            topic: ch.topic,
            icon_url: ch.icon_url.map(|_| format!("/api/dm/{}/icon", ch.id)),
            user_limit: ch.user_limit,
            position: ch.position,
            max_screen_shares: ch.max_screen_shares,
            created_at: ch.created_at,
        }
    }
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateChannelRequest {
    #[validate(length(min = 1, max = 64, message = "Name must be 1-64 characters"))]
    pub name: String,
    pub channel_type: String,
    pub category_id: Option<Uuid>,
    pub guild_id: Option<Uuid>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateChannelRequest {
    #[validate(length(min = 1, max = 64, message = "Name must be 1-64 characters"))]
    pub name: Option<String>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AddMemberRequest {
    pub user_id: Uuid,
    pub role_id: Option<Uuid>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MemberResponse {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

// ============================================================================
// Handlers
// ============================================================================

/// Create a new channel.
/// POST /api/channels
#[utoipa::path(
    post,
    path = "/api/channels",
    tag = "channels",
    request_body = CreateChannelRequest,
    responses(
        (status = 201, body = ChannelResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn create(
    State(state): State<AppState>,
    auth_user: AuthUser,
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

    // For guild channels, verify membership and MANAGE_CHANNELS permission
    if let Some(guild_id) = body.guild_id {
        crate::permissions::require_guild_permission(
            &state.db,
            guild_id,
            auth_user.id,
            crate::permissions::GuildPermissions::MANAGE_CHANNELS,
        )
        .await
        .map_err(|_| ChannelError::Forbidden)?;
    }

    // Validate voice channel user limit
    if channel_type == ChannelType::Voice {
        if let Some(limit) = body.user_limit {
            if !(1..=99).contains(&limit) {
                return Err(ChannelError::Validation(
                    "User limit must be between 1 and 99".to_string(),
                ));
            }
        }
    }

    let channel = db::create_channel(
        &state.db,
        db::CreateChannelParams {
            name: &body.name,
            channel_type: &channel_type,
            category_id: body.category_id,
            guild_id: body.guild_id,
            topic: body.topic.as_deref(),
            icon_url: None,
            user_limit: body.user_limit,
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(channel.into())))
}

/// Get a channel by ID.
/// GET /api/channels/:id
#[utoipa::path(
    get,
    path = "/api/channels/{id}",
    tag = "channels",
    params(("id" = Uuid, Path, description = "Channel ID")),
    responses(
        (status = 200, body = ChannelResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ChannelResponse>, ChannelError> {
    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth.id, id)
        .await
        .map_err(|_| ChannelError::Forbidden)?;

    let channel = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    Ok(Json(channel.into()))
}

/// Update a channel.
/// PATCH /api/channels/:id
#[utoipa::path(
    patch,
    path = "/api/channels/{id}",
    tag = "channels",
    params(("id" = Uuid, Path, description = "Channel ID")),
    request_body = UpdateChannelRequest,
    responses(
        (status = 200, body = ChannelResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn update(
    State(state): State<AppState>,
    auth_user: AuthUser,
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

    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChannelError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChannelError::Forbidden);
    }

    let channel = db::update_channel(
        &state.db,
        id,
        body.name.as_deref(),
        body.topic.as_deref(),
        None, // icon_url
        body.user_limit,
        body.position,
    )
    .await?
    .ok_or(ChannelError::NotFound)?;

    Ok(Json(channel.into()))
}

/// Delete a channel.
/// DELETE /api/channels/:id
#[utoipa::path(
    delete,
    path = "/api/channels/{id}",
    tag = "channels",
    params(("id" = Uuid, Path, description = "Channel ID")),
    responses(
        (status = 204, description = "Channel deleted"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn delete(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ChannelError> {
    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChannelError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChannelError::Forbidden);
    }

    let deleted = db::delete_channel(&state.db, id).await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChannelError::NotFound)
    }
}

/// List members of a channel.
/// GET /api/channels/:id/members
#[utoipa::path(
    get,
    path = "/api/channels/{id}/members",
    tag = "channels",
    params(("id" = Uuid, Path, description = "Channel ID")),
    responses(
        (status = 200, body = Vec<MemberResponse>),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_members(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<MemberResponse>>, ChannelError> {
    // Check channel access (VIEW_CHANNEL permission or DM participant)
    crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChannelError::Forbidden)?;

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
#[utoipa::path(
    post,
    path = "/api/channels/{id}/members",
    tag = "channels",
    params(("id" = Uuid, Path, description = "Channel ID")),
    request_body = AddMemberRequest,
    responses(
        (status = 201, description = "Member added"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn add_member(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<StatusCode, ChannelError> {
    // Check channel access and MANAGE_CHANNELS permission
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChannelError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChannelError::Forbidden);
    }

    // Check user exists
    let _ = db::find_user_by_id(&state.db, body.user_id)
        .await?
        .ok_or(ChannelError::Validation("User not found".to_string()))?;

    db::add_channel_member(&state.db, id, body.user_id, body.role_id).await?;

    Ok(StatusCode::CREATED)
}

/// Remove a member from a channel.
/// DELETE /`api/channels/:id/members/:user_id`
#[utoipa::path(
    delete,
    path = "/api/channels/{id}/members/{user_id}",
    tag = "channels",
    params(
        ("id" = Uuid, Path, description = "Channel ID"),
        ("user_id" = Uuid, Path, description = "User ID"),
    ),
    responses(
        (status = 204, description = "Member removed"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn remove_member(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path((channel_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ChannelError> {
    // Check channel access and MANAGE_CHANNELS permission
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ChannelError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChannelError::Forbidden);
    }

    let removed = db::remove_channel_member(&state.db, channel_id, user_id).await?;

    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChannelError::NotFound)
    }
}

// ============================================================================
// Mark as Read (Guild Channels)
// ============================================================================

/// Request body for marking a guild channel as read.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MarkChannelAsReadRequest {
    pub last_read_message_id: Option<Uuid>,
}

/// Mark a guild channel as read.
/// POST /api/channels/:id/read
#[utoipa::path(
    post,
    path = "/api/channels/{id}/read",
    tag = "channels",
    params(("id" = Uuid, Path, description = "Channel ID")),
    request_body = MarkChannelAsReadRequest,
    responses(
        (status = 200, description = "Marked as read"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn mark_as_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<MarkChannelAsReadRequest>,
) -> Result<Json<()>, ChannelError> {
    // 1. Verify channel exists and is a guild channel (not a DM)
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    let guild_id = channel.guild_id.ok_or(ChannelError::NotFound)?;

    // 2. Verify user is a guild member
    let is_member = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2) as "exists!""#,
        guild_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    if !is_member {
        return Err(ChannelError::Forbidden);
    }

    let now = chrono::Utc::now();

    // 3. UPSERT into channel_read_state
    sqlx::query!(
        r#"INSERT INTO channel_read_state (user_id, channel_id, last_read_at, last_read_message_id)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id, channel_id)
           DO UPDATE SET last_read_at = $3, last_read_message_id = $4"#,
        auth.id,
        channel_id,
        now,
        body.last_read_message_id
    )
    .execute(&state.db)
    .await?;

    // 4. Broadcast ChannelRead event to all user's other sessions
    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth.id,
        &ServerEvent::ChannelRead {
            channel_id,
            last_read_message_id: body.last_read_message_id,
        },
    )
    .await
    {
        tracing::warn!(
            user_id = %auth.id,
            channel_id = %channel_id,
            error = %e,
            "Failed to broadcast ChannelRead event"
        );
    }

    Ok(Json(()))
}
