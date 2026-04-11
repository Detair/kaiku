//! Channel Management Handlers

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;
use validator::Validate;

use super::types::{
    AddMemberRequest, ChannelResponse, CreateChannelRequest, MarkChannelAsReadRequest,
    MemberResponse, UpdateChannelRequest,
};
use super::{queries, ChatError};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, ChannelType};
use crate::ws::{broadcast_to_user, ServerEvent};

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
) -> Result<(StatusCode, Json<ChannelResponse>), ChatError> {
    // Validate input
    body.validate()
        .map_err(|e| ChatError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Parse channel type
    let channel_type = match body.channel_type.to_lowercase().as_str() {
        "text" => ChannelType::Text,
        "voice" => ChannelType::Voice,
        "dm" => ChannelType::Dm,
        _ => return Err(ChatError::Validation("Invalid channel type".to_string())),
    };

    // Validate voice channel user limit
    if channel_type == ChannelType::Voice {
        if let Some(limit) = body.user_limit {
            if !(1..=99).contains(&limit) {
                return Err(ChatError::Validation(
                    "User limit must be between 1 and 99".to_string(),
                ));
            }
        }
    }

    // For guild channels, use advisory lock to prevent TOCTOU race on channel limits.
    let channel = if let Some(guild_id) = body.guild_id {
        crate::permissions::require_guild_permission(
            &state.db,
            guild_id,
            auth_user.id,
            crate::permissions::GuildPermissions::MANAGE_CHANNELS,
        )
        .await
        .map_err(|_| ChatError::Forbidden)?;

        let mut tx = state.db.begin().await?;

        // Advisory lock seed 55 = channel_create (see db/mod.rs registry)
        queries::lock_channel_create(&mut tx, guild_id).await?;

        let channel_count = queries::count_channels_in_guild(&mut tx, guild_id).await?;

        if channel_count >= state.config.max_channels_per_guild {
            return Err(ChatError::LimitExceeded(format!(
                "Maximum number of channels per guild reached ({})",
                state.config.max_channels_per_guild
            )));
        }

        // Validate channel type against category type restriction
        if let Some(cat_id) = body.category_id {
            if let Some(cat_type) = queries::find_channel_category_type(&mut tx, cat_id).await? {
                let channel_type_str = match &channel_type {
                    db::ChannelType::Text => "text",
                    db::ChannelType::Voice => "voice",
                    db::ChannelType::Dm => "dm",
                };
                if (cat_type == "text" && channel_type_str != "text")
                    || (cat_type == "voice" && channel_type_str != "voice")
                {
                    return Err(ChatError::Validation(format!(
                        "This category only allows {cat_type} channels"
                    )));
                }
            }
        }

        let position = queries::next_channel_position(&mut tx, body.category_id).await?;

        let channel = queries::insert_channel(
            &mut tx,
            queries::InsertChannelParams {
                name: &body.name,
                channel_type: &channel_type,
                category_id: body.category_id,
                guild_id: body.guild_id,
                topic: body.topic.as_deref(),
                icon_url: None,
                user_limit: body.user_limit,
                position,
            },
        )
        .await?;

        tx.commit().await?;
        channel
    } else {
        db::create_channel(
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
        .await?
    };

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
) -> Result<Json<ChannelResponse>, ChatError> {
    // Check if user has VIEW_CHANNEL permission
    crate::permissions::require_channel_access(&state.db, auth.id, id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    let channel = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

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
) -> Result<Json<ChannelResponse>, ChatError> {
    // Validate input
    body.validate()
        .map_err(|e| ChatError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Check channel exists
    let _ = db::find_channel_by_id(&state.db, id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Forbidden);
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
    .ok_or(ChatError::ChannelNotFound)?;

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
) -> Result<StatusCode, ChatError> {
    // Check if user has VIEW_CHANNEL and MANAGE_CHANNELS permissions
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Forbidden);
    }

    let deleted = db::delete_channel(&state.db, id).await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChatError::ChannelNotFound)
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
) -> Result<Json<Vec<MemberResponse>>, ChatError> {
    // Check channel access (VIEW_CHANNEL permission or DM participant)
    crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

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
) -> Result<StatusCode, ChatError> {
    // Check channel access and MANAGE_CHANNELS permission
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Forbidden);
    }

    // Check user exists
    let _ = db::find_user_by_id(&state.db, body.user_id)
        .await?
        .ok_or(ChatError::Validation("User not found".to_string()))?;

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
) -> Result<StatusCode, ChatError> {
    // Check channel access and MANAGE_CHANNELS permission
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| ChatError::Forbidden)?;

    if !ctx.has_permission(crate::permissions::GuildPermissions::MANAGE_CHANNELS) {
        return Err(ChatError::Forbidden);
    }

    let removed = db::remove_channel_member(&state.db, channel_id, user_id).await?;

    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ChatError::ChannelNotFound)
    }
}

// ============================================================================
// Mark as Read (Guild Channels)
// ============================================================================

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
) -> Result<Json<()>, ChatError> {
    // 1. Verify channel exists and is a guild channel (not a DM)
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    let guild_id = channel.guild_id.ok_or(ChatError::ChannelNotFound)?;

    // 2. Verify user is a guild member
    if !queries::is_guild_member(&state.db, guild_id, auth.id).await? {
        return Err(ChatError::Forbidden);
    }

    let now = chrono::Utc::now();

    // 3. Atomic forward-only UPSERT into channel_read_state.
    // The WHERE clause inside the query ensures the read cursor only moves
    // forward by comparing message timestamps, preventing stale requests
    // from moving it backward.
    queries::upsert_channel_read_state(
        &state.db,
        auth.id,
        channel_id,
        now,
        body.last_read_message_id,
    )
    .await?;

    // 4. Broadcast ChannelRead event to all user's other sessions
    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth.id,
        &ServerEvent::ChannelRead {
            channel_id,
            last_read_message_id: Some(body.last_read_message_id),
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
