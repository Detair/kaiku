//! Direct Message Channel Management
//!
//! Handles creation and management of DM channels (1:1 and group DMs).
use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use uuid::Uuid;
use validator::Validate;

use super::queries;
use super::types::{
    CreateDMRequest, DMIconResponse, DMListResponse, DMParticipant, DMResponse, MarkAsReadRequest,
    MarkAsReadResponse, UpdateDMNameRequest,
};
use super::ChatError;
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::db::{self, Channel, ChannelType};
use crate::social::block_cache;
use crate::ws::{broadcast_to_user, ServerEvent};

// ============================================================================
// Database Functions
// ============================================================================

/// Get or create a 1:1 DM channel between two users.
///
/// Wraps the lower-level query functions in `queries` so that the
/// channel-creation flow remains a single high-level entry point.
pub async fn get_or_create_dm(
    pool: &sqlx::PgPool,
    user1_id: Uuid,
    user2_id: Uuid,
) -> Result<Channel, ChatError> {
    if let Some(existing) = queries::find_direct_dm_channel(pool, user1_id, user2_id).await? {
        return Ok(existing);
    }

    // Create new DM channel
    let channel_id = Uuid::now_v7();

    // Generate name from usernames
    let names = queries::list_usernames_for_pair(pool, user1_id, user2_id).await?;
    let dm_name = names
        .iter()
        .map(|r| r.username.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let channel = queries::insert_dm_channel(pool, channel_id, &dm_name).await?;
    queries::insert_dm_participants_pair(pool, channel_id, user1_id, user2_id).await?;

    Ok(channel)
}

/// Create a group DM channel with multiple participants.
pub async fn create_group_dm(
    pool: &sqlx::PgPool,
    creator_id: Uuid,
    participant_ids: &[Uuid],
    name: Option<&str>,
) -> Result<Channel, ChatError> {
    // Validate participant count (1-9 others + creator = 2-10 total)
    if participant_ids.is_empty() || participant_ids.len() > 9 {
        return Err(ChatError::Validation(
            "Group DMs must have 2-10 participants total".to_string(),
        ));
    }

    let channel_id = Uuid::now_v7();

    // Generate name if not provided
    let channel_name = if let Some(name) = name {
        name.to_string()
    } else {
        let mut all_ids = vec![creator_id];
        all_ids.extend_from_slice(participant_ids);

        let names = queries::list_usernames_for_ids(pool, &all_ids).await?;
        names
            .iter()
            .map(|r| r.username.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Create channel
    let channel = queries::insert_dm_channel(pool, channel_id, &channel_name).await?;

    // Add creator and remaining participants
    queries::insert_dm_participant(pool, channel_id, creator_id).await?;
    for participant_id in participant_ids {
        queries::insert_dm_participant(pool, channel_id, *participant_id).await?;
    }

    Ok(channel)
}

/// Get DM participants for a channel.
pub async fn get_dm_participants(
    pool: &sqlx::PgPool,
    channel_id: Uuid,
) -> Result<Vec<DMParticipant>, ChatError> {
    queries::list_dm_participants(pool, channel_id).await
}

/// List all DM channels for a user.
pub async fn list_user_dms(pool: &sqlx::PgPool, user_id: Uuid) -> Result<Vec<Channel>, ChatError> {
    queries::list_user_dm_channels(pool, user_id).await
}

// ============================================================================
// Handlers
// ============================================================================

/// Create or get a DM channel
/// POST /api/dm
#[utoipa::path(
    post,
    path = "/api/dm",
    tag = "dm",
    request_body = CreateDMRequest,
    responses(
        (status = 201, body = DMResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn create_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateDMRequest>,
) -> Result<(StatusCode, Json<DMResponse>), ChatError> {
    body.validate()
        .map_err(|e| ChatError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Check for duplicate participant IDs
    let mut unique_ids = body.participant_ids.clone();
    unique_ids.sort();
    unique_ids.dedup();
    if unique_ids.len() != body.participant_ids.len() {
        return Err(ChatError::Validation(
            "Duplicate participant IDs".to_string(),
        ));
    }

    // Check that auth user is not in participant list
    if body.participant_ids.contains(&auth.id) {
        return Err(ChatError::Validation(
            "Cannot include yourself in participant list".to_string(),
        ));
    }

    // Verify all participants exist
    for participant_id in &body.participant_ids {
        db::find_user_by_id(&state.db, *participant_id)
            .await?
            .ok_or_else(|| {
                ChatError::Validation("One or more participants not found".to_string())
            })?;
    }

    // For 1:1 DMs, check if either user has blocked the other
    if body.participant_ids.len() == 1 {
        match block_cache::is_blocked_either_direction(
            &state.redis,
            auth.id,
            body.participant_ids[0],
        )
        .await
        {
            Ok(true) => {
                return Err(ChatError::Validation(
                    "Cannot create DM with this user".to_string(),
                ));
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    user_id = %auth.id,
                    target_id = %body.participant_ids[0],
                    fail_open = state.config.block_check_fail_open,
                    "Redis block check failed, using failsafe policy"
                );
                if !state.config.block_check_fail_open {
                    return Err(ChatError::Validation(
                        "Cannot create DM with this user".to_string(),
                    ));
                }
            }
        }
    }

    let channel = if body.participant_ids.len() == 1 {
        // 1:1 DM
        get_or_create_dm(&state.db, auth.id, body.participant_ids[0]).await?
    } else {
        // Group DM
        create_group_dm(
            &state.db,
            auth.id,
            &body.participant_ids,
            body.name.as_deref(),
        )
        .await?
    };

    // Get participants
    let participants = get_dm_participants(&state.db, channel.id).await?;

    let response = DMResponse {
        channel: channel.into(),
        participants,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

/// List all DM channels for the authenticated user
/// GET /api/dm
#[utoipa::path(
    get,
    path = "/api/dm",
    tag = "dm",
    responses(
        (status = 200, body = Vec<DMListResponse>),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn list_dms(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<DMListResponse>>, ChatError> {
    let channels = list_user_dms(&state.db, auth.id).await?;

    let mut responses = Vec::new();
    for channel in channels {
        let participants = get_dm_participants(&state.db, channel.id).await?;

        // Get last message
        let last_message = queries::fetch_last_message_preview(&state.db, channel.id).await?;

        // Get unread count
        let unread_count = queries::dm_unread_count(&state.db, auth.id, channel.id).await?;

        responses.push(DMListResponse {
            channel: channel.into(),
            participants,
            last_message,
            unread_count,
        });
    }

    // Sort by last message time (most recent first)
    responses.sort_by(|a, b| {
        let a_time = a.last_message.as_ref().map(|m| m.created_at);
        let b_time = b.last_message.as_ref().map(|m| m.created_at);
        b_time.cmp(&a_time)
    });

    Ok(Json(responses))
}

/// Get a specific DM channel
/// GET /api/dm/:id
#[utoipa::path(
    get,
    path = "/api/dm/{id}",
    tag = "dm",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 200, body = DMResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<DMResponse>, ChatError> {
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    // Verify it's a DM channel
    if channel.channel_type != ChannelType::Dm {
        return Err(ChatError::ChannelNotFound);
    }

    // Verify auth user is a participant
    if !queries::is_dm_participant(&state.db, channel_id, auth.id).await? {
        return Err(ChatError::Forbidden);
    }

    let participants = get_dm_participants(&state.db, channel.id).await?;

    Ok(Json(DMResponse {
        channel: channel.into(),
        participants,
    }))
}

/// Leave a group DM
/// POST /api/dm/:id/leave
#[utoipa::path(
    post,
    path = "/api/dm/{id}/leave",
    tag = "dm",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 204, description = "Left DM"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn leave_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<StatusCode, ChatError> {
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    // Verify it's a DM channel
    if channel.channel_type != ChannelType::Dm {
        return Err(ChatError::ChannelNotFound);
    }

    // Remove user from participants
    let removed = queries::remove_dm_participant(&state.db, channel_id, auth.id).await?;

    if !removed {
        return Err(ChatError::ChannelNotFound);
    }

    // Check if channel is now empty
    let participant_count = queries::count_dm_participants(&state.db, channel_id).await?;

    // If channel is empty, delete it
    if participant_count == 0 {
        db::delete_channel(&state.db, channel_id).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Update Group DM Name
// ============================================================================

/// Update a group DM's display name
/// PATCH /api/dm/:id/name
#[utoipa::path(
    patch,
    path = "/api/dm/{id}/name",
    tag = "dm",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    request_body = UpdateDMNameRequest,
    responses(
        (status = 200, body = DMResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn update_dm_name(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<UpdateDMNameRequest>,
) -> Result<Json<DMResponse>, ChatError> {
    body.validate()
        .map_err(|e| ChatError::Validation(crate::validation::format_validation_errors(&e)))?;

    // Verify channel exists and is a DM
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    if channel.channel_type != ChannelType::Dm {
        return Err(ChatError::ChannelNotFound);
    }

    // Verify auth user is a participant
    if !queries::is_dm_participant(&state.db, channel_id, auth.id).await? {
        return Err(ChatError::Forbidden);
    }

    // Verify it's a group DM (more than 2 participants)
    let participant_count = queries::count_dm_participants(&state.db, channel_id).await?;

    if participant_count <= 2 {
        return Err(ChatError::Validation(
            "Cannot rename 1:1 DM channels".to_string(),
        ));
    }

    // Update the channel name
    let updated_channel =
        queries::update_dm_channel_name(&state.db, channel_id, &body.name).await?;

    // Get participants
    let participants = get_dm_participants(&state.db, channel_id).await?;

    // Broadcast name change to all participants via the channel
    if let Err(e) = crate::ws::broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::DmNameUpdated {
            channel_id,
            name: body.name.clone(),
            updated_by: auth.id,
        },
    )
    .await
    {
        tracing::warn!(
            channel_id = %channel_id,
            error = %e,
            "Failed to broadcast DmNameUpdated event"
        );
    }

    Ok(Json(DMResponse {
        channel: updated_channel.into(),
        participants,
    }))
}

// ============================================================================
// Icon Upload
// ============================================================================

/// Upload a custom icon for a DM channel
/// POST /api/dm/:id/icon
#[utoipa::path(
    post,
    path = "/api/dm/{id}/icon",
    tag = "dm",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    request_body(content = Vec<u8>, content_type = "multipart/form-data"),
    responses(
        (status = 200, body = DMIconResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn upload_dm_icon(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<DMIconResponse>, ChatError> {
    // Verify channel exists and is a DM
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::Validation("Channel not found".to_string()))?;

    if channel.channel_type != ChannelType::Dm {
        return Err(ChatError::Validation("Not a DM channel".to_string()));
    }

    // Verify auth user is a participant
    if !queries::is_dm_participant(&state.db, channel_id, auth.id).await? {
        return Err(ChatError::Forbidden);
    }

    // Process file upload (similar to uploads.rs)
    let s3 = state.s3.as_ref().ok_or(ChatError::StorageNotConfigured)?;

    let mut file_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            let _filename = field.file_name().map(String::from); // Consumed for validation

            let data = field
                .bytes()
                .await
                .map_err(|e| ChatError::Validation(e.to_string()))?;

            if data.len() > state.config.max_avatar_size {
                return Err(ChatError::FileTooLarge {
                    max_size: state.config.max_avatar_size,
                });
            }

            file_data = Some(data.to_vec());
            break; // Only need one file
        }
    }

    let file_data = file_data.ok_or(ChatError::NoFile)?;

    // Validate actual file content using magic bytes (don't trust client-provided MIME type)
    let format = image::guess_format(&file_data)
        .map_err(|_| ChatError::Validation("Unable to detect image format".to_string()))?;

    let (content_type, extension) = match format {
        image::ImageFormat::Png => ("image/png", "png"),
        image::ImageFormat::Jpeg => ("image/jpeg", "jpg"),
        image::ImageFormat::Gif => ("image/gif", "gif"),
        image::ImageFormat::WebP => ("image/webp", "webp"),
        _ => {
            return Err(ChatError::Validation(
                "Unsupported image format. Only PNG, JPEG, GIF, and WebP are allowed.".to_string(),
            ))
        }
    };

    let file_id = Uuid::now_v7();
    let s3_key = format!("avatars/channels/{channel_id}/{file_id}.{extension}");

    // Upload to S3
    s3.upload(&s3_key, file_data, content_type)
        .await
        .map_err(|e| ChatError::Storage(e.to_string()))?; // S3Error to string

    // Store S3 Key in DB
    queries::set_channel_icon(&state.db, channel_id, &s3_key).await?;

    // Return API URL
    let icon_url = format!("/api/dm/{channel_id}/icon");

    Ok(Json(DMIconResponse { icon_url }))
}

/// Get DM icon (redirects to S3 presigned URL).
#[utoipa::path(
    get,
    path = "/api/dm/{id}/icon",
    tag = "dm",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    responses(
        (status = 307, description = "Redirect to icon URL"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_dm_icon(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<impl IntoResponse, ChatError> {
    // Check channel exists
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::Validation("Channel not found".to_string()))?;

    // Check if DM
    if channel.channel_type != ChannelType::Dm {
        return Err(ChatError::Validation("Not a DM channel".to_string()));
    }

    // Check participation
    if !queries::is_dm_participant(&state.db, channel_id, auth.id).await? {
        return Err(ChatError::Forbidden);
    }

    // Get S3 key from DB
    let s3_key = channel
        .icon_url
        .ok_or(ChatError::Validation("No icon set".to_string()))?;

    // Generate presigned URL
    let s3 = state.s3.as_ref().ok_or(ChatError::StorageNotConfigured)?;
    let presigned_url = s3
        .presign_get(&s3_key)
        .await
        .map_err(|e| ChatError::Storage(e.to_string()))?;

    // Redirect
    Ok(axum::response::Redirect::temporary(&presigned_url))
}

// ============================================================================
// Mark as Read
// ============================================================================

/// Mark a DM channel as read
/// POST /api/dm/:id/read
#[utoipa::path(
    post,
    path = "/api/dm/{id}/read",
    tag = "dm",
    params(("id" = Uuid, Path, description = "DM conversation ID")),
    request_body = MarkAsReadRequest,
    responses(
        (status = 200, body = MarkAsReadResponse),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn mark_as_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<MarkAsReadRequest>,
) -> Result<Json<MarkAsReadResponse>, ChatError> {
    // Verify channel exists and user is a participant
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChatError::ChannelNotFound)?;

    if channel.channel_type != ChannelType::Dm {
        return Err(ChatError::ChannelNotFound);
    }

    if !queries::is_dm_participant(&state.db, channel_id, auth.id).await? {
        return Err(ChatError::Forbidden);
    }

    let now = chrono::Utc::now();

    // Atomic forward-only upsert: only advances the cursor, never moves it backward
    queries::upsert_dm_read_state(
        &state.db,
        auth.id,
        channel_id,
        now,
        body.last_read_message_id,
    )
    .await?;

    // Broadcast dm_read event to all user's other WebSocket sessions
    // Note: Broadcast failure shouldn't fail the request since the DB state is already updated
    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth.id,
        &ServerEvent::DmRead {
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
            "Failed to broadcast DmRead event"
        );
    }

    Ok(Json(MarkAsReadResponse {
        channel_id,
        last_read_at: now,
        last_read_message_id: Some(body.last_read_message_id),
        unread_count: 0,
    }))
}

/// Mark all DM channels as read.
/// POST /api/dm/read-all
#[utoipa::path(
    post,
    path = "/api/dm/read-all",
    tag = "dm",
    responses(
        (status = 204, description = "All DMs marked as read"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state))]
pub async fn mark_all_dms_read(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, ChatError> {
    let now = chrono::Utc::now();

    // Batch UPSERT dm_read_state for all DM channels where user is participant
    let rows = queries::mark_all_dms_read(&state.db, auth.id, now).await?;

    // Broadcast DmRead events for each updated DM channel
    for (channel_id, last_read_message_id) in &rows {
        if let Err(e) = broadcast_to_user(
            &state.redis,
            auth.id,
            &ServerEvent::DmRead {
                channel_id: *channel_id,
                last_read_message_id: *last_read_message_id,
            },
        )
        .await
        {
            tracing::warn!(
                user_id = %auth.id,
                channel_id = %channel_id,
                error = %e,
                "Failed to broadcast DmRead event"
            );
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
