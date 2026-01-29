//! Guild Emojis API
//!
//! Handlers for managing custom guild emojis.

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use uuid::Uuid;
use validator::Validate;

use crate::{
    api::AppState,
    auth::AuthUser,
    guild::types::{CreateEmojiRequest, GuildEmoji, UpdateEmojiRequest},
    ws::ServerEvent,
};
use fred::interfaces::PubsubInterface;
// Use direct Redis publish for now as broadcast_guild_emoji_update isn't in mod.rs yet 
// but we added GuildEmojiUpdated to ServerEvent.

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum EmojiError {
    #[error("Guild not found")]
    GuildNotFound,
    #[error("Emoji not found")]
    EmojiNotFound,
    #[error("Insufficient permissions")]
    Forbidden,
    #[error("Invalid filename")]
    InvalidFilename,
    #[error("File too large (maximum {max_size} bytes)")]
    FileTooLarge { max_size: usize },
    #[error("Invalid file type (must be PNG, JPEG, GIF, or WebP)")]
    InvalidFileType,
    #[error("No file provided")]
    NoFile,
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for EmojiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            EmojiError::FileTooLarge { max_size } => {
                let message = format!("File too large (max {} for emojis)", crate::util::format_file_size(max_size));
                (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({
                        "error": "FILE_TOO_LARGE",
                        "message": message,
                        "max_size_bytes": max_size
                    }))
                ).into_response()
            }
            _ => {
                let (status, code, message) = match &self {
                    EmojiError::GuildNotFound => (StatusCode::NOT_FOUND, "GUILD_NOT_FOUND", "Guild not found"),
                    EmojiError::EmojiNotFound => (StatusCode::NOT_FOUND, "EMOJI_NOT_FOUND", "Emoji not found"),
                    EmojiError::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN", "Insufficient permissions"),
                    EmojiError::InvalidFilename => (
                        StatusCode::BAD_REQUEST,
                        "INVALID_FILENAME",
                        "Invalid filename",
                    ),
                    EmojiError::InvalidFileType => (
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "INVALID_FILE_TYPE",
                        "Invalid file type",
                    ),
                    EmojiError::NoFile => (StatusCode::BAD_REQUEST, "NO_FILE", "No file provided"),
                    EmojiError::Storage(msg) => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "STORAGE_ERROR",
                        msg.as_str(),
                    ),
                    EmojiError::Validation(msg) => (
                        StatusCode::BAD_REQUEST,
                        "VALIDATION_ERROR",
                        msg.as_str(),
                    ),
                    EmojiError::Database(err) => {
                        tracing::error!("Database error: {}", err);
                        (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", "Database error")
                    }
                    EmojiError::FileTooLarge { .. } => unreachable!("Handled above"),
                };
                (status, Json(json!({ "error": code, "message": message }))).into_response()
            }
        }
    }
}

// ============================================================================
// Internal Helpers
// ============================================================================

async fn check_guild_membership(
    db: &sqlx::PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM guild_members WHERE guild_id = $1 AND user_id = $2)",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(db)
    .await?;

    Ok(result.0)
}

// ============================================================================
// Router
// ============================================================================

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_emojis).post(create_emoji))
        .route("/:emoji_id", get(get_emoji).patch(update_emoji).delete(delete_emoji))
}

// ============================================================================
// Handlers
// ============================================================================

/// List guild emojis.
///
/// `GET /api/guilds/:id/emojis`
pub async fn list_emojis(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
    auth_user: AuthUser,
) -> Result<Json<Vec<GuildEmoji>>, EmojiError> {
    // Check guild membership
    if !check_guild_membership(&state.db, guild_id, auth_user.id).await? {
        return Err(EmojiError::GuildNotFound);
    }

    let emojis = sqlx::query_as::<_, GuildEmoji>(
        r"
        SELECT * FROM guild_emojis
        WHERE guild_id = $1
        ORDER BY created_at DESC
        ",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(emojis))
}

/// Get specific emoji.
///
/// `GET /api/guilds/:id/emojis/:emoji_id`
pub async fn get_emoji(
    State(state): State<AppState>,
    Path((guild_id, emoji_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
) -> Result<Json<GuildEmoji>, EmojiError> {
    // Check guild membership
    if !check_guild_membership(&state.db, guild_id, auth_user.id).await? {
        return Err(EmojiError::GuildNotFound);
    }

    let emoji = sqlx::query_as::<_, GuildEmoji>(
        r"
        SELECT * FROM guild_emojis
        WHERE id = $1 AND guild_id = $2
        ",
    )
    .bind(emoji_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(EmojiError::EmojiNotFound)?;

    Ok(Json(emoji))
}

/// Create a custom emoji.
///
/// `POST /api/guilds/:id/emojis`
/// Expects multipart form with `name` and `file`.
#[tracing::instrument(skip(state, auth_user, multipart))]
pub async fn create_emoji(
    State(state): State<AppState>,
    Path(guild_id): Path<Uuid>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<Json<GuildEmoji>, EmojiError> {
    if !check_guild_membership(&state.db, guild_id, auth_user.id).await? {
        return Err(EmojiError::GuildNotFound);
    }

    let s3 = state.s3.as_ref().ok_or(EmojiError::Storage("S3 not configured".into()))?;

    let mut name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;

    // Parse multipart
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or_default().to_string();
        match field_name.as_str() {
            "name" => {
                let text = field.text().await.map_err(|e| EmojiError::Validation(e.to_string()))?;
                name = Some(text);
            }
            "file" => {
                let data = field.bytes().await.map_err(|e| EmojiError::Validation(e.to_string()))?;
                if data.len() > state.config.max_emoji_size {
                    return Err(EmojiError::FileTooLarge {
                        max_size: state.config.max_emoji_size
                    });
                }
                file_data = Some(data.to_vec());
            }
            _ => {}
        }
    }

    let file_data = file_data.ok_or(EmojiError::NoFile)?;
    let name_str = name.ok_or(EmojiError::Validation("Name required".into()))?;

    // Validate request manually since we parsed multipart
    let req = CreateEmojiRequest { name: name_str.clone() };
    if let Err(e) = req.validate() {
        return Err(EmojiError::Validation(e.to_string()));
    }

    // Validate actual file content using magic bytes (don't trust client-provided MIME type)
    let format = image::guess_format(&file_data)
        .map_err(|_| EmojiError::Validation("Unable to detect image format".to_string()))?;

    let (content_type, extension) = match format {
        image::ImageFormat::Png => ("image/png", "png"),
        image::ImageFormat::Jpeg => ("image/jpeg", "jpg"),
        image::ImageFormat::Gif => ("image/gif", "gif"),
        image::ImageFormat::WebP => ("image/webp", "webp"),
        _ => return Err(EmojiError::Validation("Unsupported image format. Only PNG, JPEG, GIF, and WebP are allowed.".to_string())),
    };

    let animated = content_type == "image/gif";
    let emoji_id = Uuid::now_v7();

    let s3_key = format!("emojis/{}/{}.{}", guild_id, emoji_id, extension);
    
    // Upload to S3
    s3.upload(&s3_key, file_data, &content_type)
        .await
        .map_err(|e| EmojiError::Storage(e.to_string()))?;

    let image_url = format!("/api/guilds/{}/emojis/{}/image", guild_id, emoji_id);

    // Insert into DB
    let emoji = sqlx::query_as::<_, GuildEmoji>(
        r"
        INSERT INTO guild_emojis (id, guild_id, name, image_url, animated, uploaded_by)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        ",
    )
    .bind(emoji_id)
    .bind(guild_id)
    .bind(&req.name)
    .bind(&image_url) // Store the proxy URL (placeholder)
    .bind(animated)
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;

    // Broadcast update
    // Re-query full list for broadcast

    // Re-query full list for broadcast
    let all_emojis = sqlx::query_as::<_, GuildEmoji>(
        "SELECT * FROM guild_emojis WHERE guild_id = $1 ORDER BY created_at DESC"
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    let event = ServerEvent::GuildEmojiUpdated {
        guild_id,
        emojis: all_emojis,
    };

    // Manual broadcast using Redis
    let channel = crate::ws::channels::guild_events(guild_id);
    match serde_json::to_string(&event) {
        Ok(payload) => {
            if let Err(e) = state.redis.publish::<(), _, _>(channel, payload).await {
                tracing::error!(
                    error = %e,
                    guild_id = %guild_id,
                    event = "GuildEmojiUpdated",
                    "Failed to broadcast emoji creation via Redis - other clients will not receive real-time update"
                );
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                guild_id = %guild_id,
                "Failed to serialize GuildEmojiUpdated event - broadcast skipped"
            );
        }
    }

    Ok(Json(emoji))
}

/// Update an emoji.
///
/// `PATCH /api/guilds/:id/emojis/:emoji_id`
pub async fn update_emoji(
    State(state): State<AppState>,
    Path((guild_id, emoji_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
    Json(req): Json<UpdateEmojiRequest>,
) -> Result<Json<GuildEmoji>, EmojiError> {
    if let Err(e) = req.validate() {
        return Err(EmojiError::Validation(e.to_string()));
    }

    // Check ownership/permissions
    let emoji = sqlx::query_as::<_, GuildEmoji>(
        "SELECT * FROM guild_emojis WHERE id = $1 AND guild_id = $2"
    )
    .bind(emoji_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(EmojiError::EmojiNotFound)?;

    if emoji.uploaded_by != auth_user.id {
         return Err(EmojiError::Forbidden);
    }

    let updated = sqlx::query_as::<_, GuildEmoji>(
        r"
        UPDATE guild_emojis
        SET name = $1
        WHERE id = $2 AND guild_id = $3
        RETURNING *
        ",
    )
    .bind(&req.name)
    .bind(emoji_id)
    .bind(guild_id)
    .fetch_one(&state.db)
    .await?;

     // Broadcast update (full list)
    let all_emojis = sqlx::query_as::<_, GuildEmoji>(
        "SELECT * FROM guild_emojis WHERE guild_id = $1 ORDER BY created_at DESC"
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    let event = ServerEvent::GuildEmojiUpdated {
        guild_id,
        emojis: all_emojis,
    };

    let channel = crate::ws::channels::guild_events(guild_id);
    match serde_json::to_string(&event) {
        Ok(payload) => {
            if let Err(e) = state.redis.publish::<(), _, _>(channel, payload).await {
                tracing::error!(
                    error = %e,
                    guild_id = %guild_id,
                    event = "GuildEmojiUpdated",
                    "Failed to broadcast emoji update via Redis - other clients will not receive real-time update"
                );
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                guild_id = %guild_id,
                "Failed to serialize GuildEmojiUpdated event - broadcast skipped"
            );
        }
    }

    Ok(Json(updated))
}

/// Delete an emoji.
///
/// `DELETE /api/guilds/:id/emojis/:emoji_id`
pub async fn delete_emoji(
    State(state): State<AppState>,
    Path((guild_id, emoji_id)): Path<(Uuid, Uuid)>,
    auth_user: AuthUser,
) -> Result<StatusCode, EmojiError> {
    // Check existence and fetch details for S3 deletion
    let emoji = sqlx::query_as::<_, GuildEmoji>(
        "SELECT * FROM guild_emojis WHERE id = $1 AND guild_id = $2"
    )
    .bind(emoji_id)
    .bind(guild_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(EmojiError::EmojiNotFound)?;

    if emoji.uploaded_by != auth_user.id {
         return Err(EmojiError::Forbidden);
    }

    // Delete from DB
    sqlx::query("DELETE FROM guild_emojis WHERE id = $1")
        .bind(emoji_id)
        .execute(&state.db)
        .await?;

    // Delete from S3 (best effort)
    if let Some(s3) = &state.s3 {
        let extensions = ["png", "jpg", "gif", "webp"];
        for ext in extensions {
             let key = format!("emojis/{}/{}.{}", guild_id, emoji_id, ext);
             let _ = s3.delete(&key).await;
        }
    }

    // Broadcast update (full list)
    let all_emojis = sqlx::query_as::<_, GuildEmoji>(
        "SELECT * FROM guild_emojis WHERE guild_id = $1 ORDER BY created_at DESC"
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    let event = ServerEvent::GuildEmojiUpdated {
        guild_id,
        emojis: all_emojis,
    };

    let channel = crate::ws::channels::guild_events(guild_id);
    match serde_json::to_string(&event) {
        Ok(payload) => {
            if let Err(e) = state.redis.publish::<(), _, _>(channel, payload).await {
                tracing::error!(
                    error = %e,
                    guild_id = %guild_id,
                    event = "GuildEmojiUpdated",
                    "Failed to broadcast emoji deletion via Redis - other clients will not receive real-time update"
                );
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                guild_id = %guild_id,
                "Failed to serialize GuildEmojiUpdated event - broadcast skipped"
            );
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
