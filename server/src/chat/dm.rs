//! Direct Message Channel Management
//!
//! Handles creation and management of DM channels (1:1 and group DMs).
use axum::{
    extract::{Path, State, Multipart},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::AppState,
    chat::uploads::UploadError,
    auth::AuthUser,
    db::{self, Channel, ChannelType},
    ws::{broadcast_to_user, ServerEvent},
};

use super::channels::{ChannelError, ChannelResponse};

struct UsernameRecord {
    username: String,
}

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct CreateDMRequest {
    /// User ID(s) to create DM with (1 for 1:1, multiple for group DM)
    #[validate(length(min = 1, max = 9, message = "Must have 1-9 other participants"))]
    pub participant_ids: Vec<Uuid>,
    /// Optional name for group DMs
    #[validate(length(max = 100))]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DMResponse {
    #[serde(flatten)]
    pub channel: ChannelResponse,
    pub participants: Vec<DMParticipant>,
}

/// Last message info for DM list preview
#[derive(Debug, Serialize)]
pub struct LastMessagePreview {
    pub id: Uuid,
    pub content: String,
    pub user_id: Uuid,
    pub username: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Enhanced DM response with unread count and last message
#[derive(Debug, Serialize)]
pub struct DMListResponse {
    #[serde(flatten)]
    pub channel: ChannelResponse,
    pub participants: Vec<DMParticipant>,
    pub last_message: Option<LastMessagePreview>,
    pub unread_count: i64,
}

#[derive(Debug, Serialize)]
pub struct DMParticipant {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Database Functions
// ============================================================================

/// Get or create a 1:1 DM channel between two users
pub async fn get_or_create_dm(
    pool: &sqlx::PgPool,
    user1_id: Uuid,
    user2_id: Uuid,
) -> sqlx::Result<Channel> {
    // Check for existing DM between these two users
    let existing = sqlx::query_as::<_, Channel>(
        r"SELECT c.id, c.name, c.channel_type, c.category_id, c.guild_id,
                  c.topic, c.user_limit, c.position, c.max_screen_shares, c.created_at, c.updated_at
           FROM channels c
           JOIN dm_participants p1 ON c.id = p1.channel_id AND p1.user_id = $1
           JOIN dm_participants p2 ON c.id = p2.channel_id AND p2.user_id = $2
           WHERE c.channel_type = 'dm' AND c.guild_id IS NULL
           AND (SELECT COUNT(*) FROM dm_participants WHERE channel_id = c.id) = 2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(pool)
    .await?;

    if let Some(dm) = existing {
        return Ok(dm);
    }

    // Create new DM channel
    let channel_id = Uuid::now_v7();

    // Generate name from usernames
    let names: Vec<UsernameRecord> = sqlx::query_as!(
        UsernameRecord,
        "SELECT username FROM users WHERE id = $1 OR id = $2 ORDER BY username",
        user1_id,
        user2_id
    )
    .fetch_all(pool)
    .await?;

    let dm_name = names
        .iter()
        .map(|r| r.username.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let channel = sqlx::query_as::<_, Channel>(
        r"INSERT INTO channels (id, name, channel_type, guild_id, position)
           VALUES ($1, $2, 'dm', NULL, 0)
           RETURNING id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at",
    )
    .bind(channel_id)
    .bind(&dm_name)
    .fetch_one(pool)
    .await?;

    // Add both users as participants
    sqlx::query!(
        "INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2), ($1, $3)",
        channel_id,
        user1_id,
        user2_id
    )
    .execute(pool)
    .await?;

    Ok(channel)
}

/// Create a group DM channel with multiple participants
pub async fn create_group_dm(
    pool: &sqlx::PgPool,
    creator_id: Uuid,
    participant_ids: &[Uuid],
    name: Option<&str>,
) -> sqlx::Result<Channel> {
    // Validate participant count (1-9 others + creator = 2-10 total)
    if participant_ids.is_empty() || participant_ids.len() > 9 {
        return Err(sqlx::Error::Protocol(
            "Group DMs must have 2-10 participants total".into(),
        ));
    }

    let channel_id = Uuid::now_v7();

    // Generate name if not provided
    let channel_name = if let Some(name) = name {
        name.to_string()
    } else {
        // Get usernames for auto-generated name
        let mut all_ids = vec![creator_id];
        all_ids.extend_from_slice(participant_ids);

        let names: Vec<UsernameRecord> = sqlx::query_as!(
            UsernameRecord,
            "SELECT username FROM users WHERE id = ANY($1) ORDER BY username",
            &all_ids[..]
        )
        .fetch_all(pool)
        .await?;

        names
            .iter()
            .map(|r| r.username.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Create channel
    let channel = sqlx::query_as::<_, Channel>(
        r"INSERT INTO channels (id, name, channel_type, guild_id, position)
           VALUES ($1, $2, 'dm', NULL, 0)
           RETURNING id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at",
    )
    .bind(channel_id)
    .bind(&channel_name)
    .fetch_one(pool)
    .await?;

    // Add creator as participant
    sqlx::query!(
        "INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)",
        channel_id,
        creator_id
    )
    .execute(pool)
    .await?;

    // Add other participants
    for participant_id in participant_ids {
        sqlx::query!(
            "INSERT INTO dm_participants (channel_id, user_id) VALUES ($1, $2)",
            channel_id,
            participant_id
        )
        .execute(pool)
        .await?;
    }

    Ok(channel)
}

/// Get DM participants for a channel
pub async fn get_dm_participants(
    pool: &sqlx::PgPool,
    channel_id: Uuid,
) -> sqlx::Result<Vec<DMParticipant>> {
    let participants = sqlx::query_as!(
        DMParticipant,
        r#"SELECT
            u.id as user_id,
            u.username,
            u.display_name,
            u.avatar_url,
            dp.joined_at
           FROM dm_participants dp
           JOIN users u ON u.id = dp.user_id
           WHERE dp.channel_id = $1
           ORDER BY dp.joined_at ASC"#,
        channel_id
    )
    .fetch_all(pool)
    .await?;

    Ok(participants)
}

/// List all DM channels for a user
pub async fn list_user_dms(pool: &sqlx::PgPool, user_id: Uuid) -> sqlx::Result<Vec<Channel>> {
    let channels = sqlx::query_as::<_, Channel>(
        r"SELECT c.id, c.name, c.channel_type, c.category_id, c.guild_id,
                  c.topic, c.user_limit, c.position, c.max_screen_shares, c.created_at, c.updated_at
           FROM channels c
           JOIN dm_participants dp ON c.id = dp.channel_id
           WHERE dp.user_id = $1 AND c.channel_type = 'dm'
           ORDER BY c.updated_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(channels)
}

// ============================================================================
// Handlers
// ============================================================================

/// Create or get a DM channel
/// POST /api/dm
pub async fn create_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateDMRequest>,
) -> Result<(StatusCode, Json<DMResponse>), ChannelError> {
    body.validate()
        .map_err(|e| ChannelError::Validation(e.to_string()))?;

    // Check for duplicate participant IDs
    let mut unique_ids = body.participant_ids.clone();
    unique_ids.sort();
    unique_ids.dedup();
    if unique_ids.len() != body.participant_ids.len() {
        return Err(ChannelError::Validation(
            "Duplicate participant IDs".to_string(),
        ));
    }

    // Check that auth user is not in participant list
    if body.participant_ids.contains(&auth.id) {
        return Err(ChannelError::Validation(
            "Cannot include yourself in participant list".to_string(),
        ));
    }

    // Verify all participants exist
    for participant_id in &body.participant_ids {
        db::find_user_by_id(&state.db, *participant_id)
            .await?
            .ok_or_else(|| {
                ChannelError::Validation("One or more participants not found".to_string())
            })?;
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
pub async fn list_dms(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<DMListResponse>>, ChannelError> {
    let channels = list_user_dms(&state.db, auth.id).await?;

    let mut responses = Vec::new();
    for channel in channels {
        let participants = get_dm_participants(&state.db, channel.id).await?;

        // Get last message
        let last_message = sqlx::query_as!(
            LastMessagePreview,
            r#"SELECT m.id, m.content, m.user_id, u.username, m.created_at
               FROM messages m
               JOIN users u ON u.id = m.user_id
               WHERE m.channel_id = $1
               ORDER BY m.created_at DESC
               LIMIT 1"#,
            channel.id
        )
        .fetch_optional(&state.db)
        .await?;

        // Get unread count
        let read_state_row = sqlx::query!(
            r#"SELECT last_read_at FROM dm_read_state
               WHERE user_id = $1 AND channel_id = $2"#,
            auth.id,
            channel.id
        )
        .fetch_optional(&state.db)
        .await?;

        let unread_count = if let Some(read_state) = read_state_row {
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!" FROM messages
                   WHERE channel_id = $1 AND created_at > $2"#,
                channel.id,
                read_state.last_read_at
            )
            .fetch_one(&state.db)
            .await?
        } else {
            // No read state = all messages are unread
            sqlx::query_scalar!(
                r#"SELECT COUNT(*) as "count!" FROM messages WHERE channel_id = $1"#,
                channel.id
            )
            .fetch_one(&state.db)
            .await?
        };

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
pub async fn get_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<Json<DMResponse>, ChannelError> {
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    // Verify it's a DM channel
    if channel.channel_type != ChannelType::Dm {
        return Err(ChannelError::NotFound);
    }

    // Verify auth user is a participant
    let is_participant = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as "exists!""#,
        channel_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    if !is_participant {
        return Err(ChannelError::Forbidden);
    }

    let participants = get_dm_participants(&state.db, channel.id).await?;

    Ok(Json(DMResponse {
        channel: channel.into(),
        participants,
    }))
}

/// Leave a group DM
/// POST /api/dm/:id/leave
pub async fn leave_dm(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<StatusCode, ChannelError> {
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    // Verify it's a DM channel
    if channel.channel_type != ChannelType::Dm {
        return Err(ChannelError::NotFound);
    }

    // Remove user from participants
    let result: sqlx::postgres::PgQueryResult = sqlx::query!(
        "DELETE FROM dm_participants WHERE channel_id = $1 AND user_id = $2",
        channel_id,
        auth.id
    )
    .execute(&state.db)
    .await?;
    
    let removed = result.rows_affected();

    if removed == 0 {
        return Err(ChannelError::NotFound);
    }

    // Check if channel is now empty
    let participant_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM dm_participants WHERE channel_id = $1"#,
        channel_id
    )
    .fetch_one(&state.db)
    .await?;

    // If channel is empty, delete it
    if participant_count == 0 {
        db::delete_channel(&state.db, channel_id).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Update Group DM Name
// ============================================================================

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateDMNameRequest {
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: String,
}

/// Update a group DM's display name
/// PATCH /api/dm/:id/name
pub async fn update_dm_name(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<UpdateDMNameRequest>,
) -> Result<Json<DMResponse>, ChannelError> {
    body.validate()
        .map_err(|e| ChannelError::Validation(e.to_string()))?;

    // Verify channel exists and is a DM
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    if channel.channel_type != ChannelType::Dm {
        return Err(ChannelError::NotFound);
    }

    // Verify auth user is a participant
    let is_participant = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as "exists!""#,
        channel_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    if !is_participant {
        return Err(ChannelError::Forbidden);
    }

    // Verify it's a group DM (more than 2 participants)
    let participant_count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM dm_participants WHERE channel_id = $1"#,
        channel_id
    )
    .fetch_one(&state.db)
    .await?;

    if participant_count <= 2 {
        return Err(ChannelError::Validation(
            "Cannot rename 1:1 DM channels".to_string(),
        ));
    }

    // Update the channel name
    let updated_channel = sqlx::query_as::<_, crate::db::Channel>(
        r"UPDATE channels SET name = $1, updated_at = NOW()
          WHERE id = $2
          RETURNING id, name, channel_type, category_id, guild_id, topic, user_limit, position, max_screen_shares, created_at, updated_at",
    )
    .bind(&body.name)
    .bind(channel_id)
    .fetch_one(&state.db)
    .await?;

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

/// Response for DM icon upload
#[derive(Debug, Serialize)]
pub struct DMIconResponse {
    pub icon_url: String,
}

/// Upload a custom icon for a DM channel
/// POST /api/dm/:id/icon
pub async fn upload_dm_icon(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<Json<DMIconResponse>, UploadError> {
    // Verify channel exists and is a DM
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(UploadError::Validation("Channel not found".to_string()))?;

    if channel.channel_type != ChannelType::Dm {
        return Err(UploadError::Validation("Not a DM channel".to_string()));
    }

    // Verify auth user is a participant
    let is_participant = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as "exists!""#,
        channel_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    if !is_participant {
        return Err(UploadError::Forbidden);
    }

    // Process file upload (similar to uploads.rs)
    let s3 = state.s3.as_ref().ok_or(UploadError::NotConfigured)?;

    let mut file_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            let _filename = field.file_name().map(String::from); // Consumed for validation
            
            let data = field.bytes().await.map_err(|e| UploadError::Validation(e.to_string()))?;
            
            if data.len() > state.config.max_avatar_size {
                return Err(UploadError::TooLarge { max_size: state.config.max_avatar_size });
            }
            
            file_data = Some(data.to_vec());
            break; // Only need one file
        }
    }

    let file_data = file_data.ok_or(UploadError::NoFile)?;
    
    // Validate actual file content using magic bytes (don't trust client-provided MIME type)
    let format = image::guess_format(&file_data)
        .map_err(|_| UploadError::Validation("Unable to detect image format".to_string()))?;

    let (content_type, extension) = match format {
        image::ImageFormat::Png => ("image/png", "png"),
        image::ImageFormat::Jpeg => ("image/jpeg", "jpg"),
        image::ImageFormat::Gif => ("image/gif", "gif"),
        image::ImageFormat::WebP => ("image/webp", "webp"),
        _ => return Err(UploadError::Validation("Unsupported image format. Only PNG, JPEG, GIF, and WebP are allowed.".to_string())),
    };
    
    let file_id = Uuid::now_v7();
    let s3_key = format!("avatars/channels/{channel_id}/{file_id}.{extension}");

    // Upload to S3
    s3.upload(&s3_key, file_data, content_type)
        .await
        .map_err(|e| UploadError::Storage(e.to_string()))?; // S3Error to string

    // Store S3 Key in DB
    sqlx::query!(
        "UPDATE channels SET icon_url = $1, updated_at = NOW() WHERE id = $2",
        s3_key,
        channel_id
    )
    .execute(&state.db)
    .await?;

    // Return API URL
    let icon_url = format!("/api/dm/{channel_id}/icon");

    Ok(Json(DMIconResponse { icon_url }))
}

/// Get DM icon (redirects to S3 presigned URL).
pub async fn get_dm_icon(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
) -> Result<impl IntoResponse, UploadError> {
    // Check channel exists
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(UploadError::Validation("Channel not found".to_string()))?;

    // Check if DM
    if channel.channel_type != ChannelType::Dm {
        return Err(UploadError::Validation("Not a DM channel".to_string()));
    }

    // Check participation
    let is_participant = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as "exists!""#,
        channel_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    if !is_participant {
        return Err(UploadError::Forbidden);
    }

    // Get S3 key from DB
    let s3_key = channel.icon_url.ok_or(UploadError::Validation("No icon set".to_string()))?;

    // Generate presigned URL
    let s3 = state.s3.as_ref().ok_or(UploadError::NotConfigured)?;
    let presigned_url = s3
        .presign_get(&s3_key)
        .await
        .map_err(|e| UploadError::Storage(e.to_string()))?;

    // Redirect
    Ok(axum::response::Redirect::temporary(&presigned_url))
}

// ============================================================================
// Mark as Read
// ============================================================================

/// Mark DM as read request body
#[derive(Debug, Deserialize)]
pub struct MarkAsReadRequest {
    pub last_read_message_id: Option<Uuid>,
}

/// Mark DM as read response
#[derive(Debug, Serialize)]
pub struct MarkAsReadResponse {
    pub channel_id: Uuid,
    pub last_read_at: chrono::DateTime<chrono::Utc>,
    pub last_read_message_id: Option<Uuid>,
    pub unread_count: i64,
}

/// Mark a DM channel as read
/// POST /api/dm/:id/read
pub async fn mark_as_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(body): Json<MarkAsReadRequest>,
) -> Result<Json<MarkAsReadResponse>, ChannelError> {
    // Verify channel exists and user is a participant
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(ChannelError::NotFound)?;

    if channel.channel_type != ChannelType::Dm {
        return Err(ChannelError::NotFound);
    }

    let is_participant = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM dm_participants WHERE channel_id = $1 AND user_id = $2) as "exists!""#,
        channel_id,
        auth.id
    )
    .fetch_one(&state.db)
    .await?;

    if !is_participant {
        return Err(ChannelError::Forbidden);
    }

    let now = chrono::Utc::now();

    // Upsert read state
    sqlx::query!(
        r#"INSERT INTO dm_read_state (user_id, channel_id, last_read_at, last_read_message_id)
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

    // Broadcast dm_read event to all user's other WebSocket sessions
    // Note: Broadcast failure shouldn't fail the request since the DB state is already updated
    if let Err(e) = broadcast_to_user(
        &state.redis,
        auth.id,
        &ServerEvent::DmRead {
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
            "Failed to broadcast DmRead event"
        );
    }

    Ok(Json(MarkAsReadResponse {
        channel_id,
        last_read_at: now,
        last_read_message_id: body.last_read_message_id,
        unread_count: 0,
    }))
}
