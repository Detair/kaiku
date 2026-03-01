//! File Upload Handling
//!
//! Handles file uploads to S3-compatible storage and metadata management.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderName, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::messages::{detect_mention_type, AttachmentInfo, AuthorProfile, MessageResponse};
use super::s3::S3Client;
use crate::api::AppState;
use crate::auth::jwt::validate_access_token;
use crate::auth::AuthUser;
use crate::db;
use crate::ws::{broadcast_to_channel, ServerEvent};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during file upload operations.
#[derive(Debug, Error)]
pub enum UploadError {
    /// File uploads are not configured.
    #[error("File uploads are not configured")]
    NotConfigured,

    /// File not found.
    #[error("File not found")]
    NotFound,

    /// File too large.
    #[error("File too large (max: {max_size} bytes)")]
    TooLarge {
        /// Maximum allowed size in bytes.
        max_size: usize,
    },

    /// Invalid MIME type.
    #[error("Invalid file type: {mime_type}")]
    InvalidMimeType {
        /// The rejected MIME type.
        mime_type: String,
    },

    /// No file provided.
    #[error("No file provided")]
    NoFile,

    /// Invalid filename.
    #[error("Invalid filename")]
    InvalidFilename,

    /// Message not found.
    #[error("Message not found")]
    MessageNotFound,

    /// Access denied.
    #[error("Access denied")]
    Forbidden,

    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Database error.
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    /// Validation error.
    #[error("Validation error: {0}")]
    Validation(String),
}

impl IntoResponse for UploadError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "STORAGE_NOT_CONFIGURED",
                self.to_string(),
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "FILE_NOT_FOUND", self.to_string()),
            Self::TooLarge { .. } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "FILE_TOO_LARGE",
                self.to_string(),
            ),
            Self::InvalidMimeType { .. } => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "INVALID_MIME_TYPE",
                self.to_string(),
            ),
            Self::NoFile => (StatusCode::BAD_REQUEST, "NO_FILE", self.to_string()),
            Self::InvalidFilename => (
                StatusCode::BAD_REQUEST,
                "INVALID_FILENAME",
                self.to_string(),
            ),
            Self::MessageNotFound => (StatusCode::NOT_FOUND, "MESSAGE_NOT_FOUND", self.to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN", self.to_string()),
            Self::Storage(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "STORAGE_ERROR",
                "Storage operation failed".to_string(),
            ),
            Self::Database(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                "Database operation failed".to_string(),
            ),
            Self::Validation(_) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                self.to_string(),
            ),
        };

        let body = Json(serde_json::json!({
            "error": code,
            "message": message,
        }));

        (status, body).into_response()
    }
}

// ============================================================================
// Request/Response Types
// ============================================================================

/// Response for successful file upload.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UploadedFile {
    /// Attachment ID.
    pub id: Uuid,
    /// Original filename.
    pub filename: String,
    /// MIME type.
    pub mime_type: String,
    /// File size in bytes.
    pub size: i64,
    /// URL to access the file.
    pub url: String,
}

/// Response for attachment metadata.
#[derive(Debug, Serialize)]
pub struct AttachmentResponse {
    /// Attachment ID.
    pub id: Uuid,
    /// Message ID this attachment belongs to.
    pub message_id: Uuid,
    /// Original filename.
    pub filename: String,
    /// MIME type.
    pub mime_type: String,
    /// File size in bytes.
    pub size_bytes: i64,
    /// When the attachment was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<db::FileAttachment> for AttachmentResponse {
    fn from(a: db::FileAttachment) -> Self {
        Self {
            id: a.id,
            message_id: a.message_id,
            filename: a.filename,
            mime_type: a.mime_type,
            size_bytes: a.size_bytes,
            created_at: a.created_at,
        }
    }
}

// ============================================================================
// Constants
// ============================================================================

/// Default allowed MIME types for uploads.
const DEFAULT_ALLOWED_TYPES: &[&str] = &[
    // Images
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    // Documents
    "application/pdf",
    "text/plain",
    // Audio
    "audio/mpeg",
    "audio/ogg",
    "audio/wav",
    // Video
    "video/mp4",
    "video/webm",
];

/// Validate file content against its claimed MIME type using magic byte detection.
///
/// Returns the verified MIME type (detected from content, or the claimed type for
/// formats where magic byte detection isn't possible like plain text).
fn validate_file_content(data: &[u8], claimed_mime: &str) -> Result<String, UploadError> {
    // For text/plain: infer can't detect plain text via magic bytes.
    // Accept if the content is valid UTF-8 and contains no null bytes (binary indicator).
    if claimed_mime == "text/plain" {
        if std::str::from_utf8(data).is_ok() && !data.contains(&0) {
            return Ok(claimed_mime.to_string());
        }
        return Err(UploadError::InvalidMimeType {
            mime_type: "binary data claimed as text/plain".to_string(),
        });
    }

    // Use magic byte detection for all other types
    let detected = if let Some(kind) = infer::get(data) {
        kind.mime_type().to_string()
    } else {
        // No magic bytes recognized — reject the file
        tracing::warn!(
            claimed_mime = %claimed_mime,
            size = data.len(),
            "File content does not match any known magic byte signature"
        );
        return Err(UploadError::InvalidMimeType {
            mime_type: format!("{claimed_mime} (content unrecognizable)"),
        });
    };

    // Allow if detected type matches claimed type
    if detected == claimed_mime {
        return Ok(detected);
    }

    // Allow known equivalent pairs (e.g. audio/ogg detected as video/ogg)
    let compatible = matches!(
        (claimed_mime, detected.as_str()),
        ("audio/ogg", "video/ogg") | ("audio/wav", "audio/x-wav")
    );

    if compatible {
        return Ok(claimed_mime.to_string());
    }

    tracing::warn!(
        claimed_mime = %claimed_mime,
        detected_mime = %detected,
        "File content type mismatch"
    );
    Err(UploadError::InvalidMimeType {
        mime_type: format!("{claimed_mime} (detected: {detected})"),
    })
}

// ============================================================================
// Handlers
// ============================================================================

/// Upload a file attachment.
///
/// POST /api/messages/upload
///
/// Expects multipart form with:
/// - `file`: The file data
/// - `message_id`: UUID of the message to attach to
#[utoipa::path(
    post,
    path = "/api/messages/upload",
    tag = "uploads",
    request_body(content = Vec<u8>, content_type = "multipart/form-data"),
    responses(
        (status = 201, body = UploadedFile),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth_user, multipart))]
pub async fn upload_file(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadedFile>), UploadError> {
    // Check S3 is configured
    let s3 = state.s3.as_ref().ok_or(UploadError::NotConfigured)?;

    let mut file_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut message_id: Option<Uuid> = None;

    // Parse multipart form
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "file" => {
                filename = field.file_name().map(String::from);
                content_type = field.content_type().map(String::from);

                let data = field
                    .bytes()
                    .await
                    .map_err(|e| UploadError::Validation(e.to_string()))?;

                // Check file size
                if data.len() > state.config.max_upload_size {
                    return Err(UploadError::TooLarge {
                        max_size: state.config.max_upload_size,
                    });
                }

                file_data = Some(data.to_vec());
            }
            "message_id" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| UploadError::Validation(e.to_string()))?;
                message_id = Some(
                    text.parse()
                        .map_err(|_| UploadError::Validation("Invalid message_id".to_string()))?,
                );
            }
            _ => {
                // Ignore unknown fields
            }
        }
    }

    // Validate required fields
    let file_data = file_data.ok_or(UploadError::NoFile)?;
    let filename = filename.ok_or(UploadError::InvalidFilename)?;
    let message_id = message_id.ok_or(UploadError::Validation(
        "message_id is required".to_string(),
    ))?;

    // Sanitize filename
    let safe_filename = sanitize_filename(&filename);
    if safe_filename.is_empty() {
        return Err(UploadError::InvalidFilename);
    }

    // Determine content type
    let content_type = content_type
        .or_else(|| {
            mime_guess::from_path(&filename)
                .first()
                .map(|m| m.to_string())
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Validate MIME type
    let allowed_types: Vec<&str> = state.config.allowed_mime_types.as_ref().map_or_else(
        || DEFAULT_ALLOWED_TYPES.to_vec(),
        |v| v.iter().map(std::string::String::as_str).collect(),
    );

    if !allowed_types.contains(&content_type.as_str()) {
        return Err(UploadError::InvalidMimeType {
            mime_type: content_type,
        });
    }

    // Validate actual file content matches claimed MIME type (magic byte check)
    let content_type = validate_file_content(&file_data, &content_type)?;

    // Verify message exists and user has access
    let message = db::find_message_by_id(&state.db, message_id)
        .await?
        .ok_or(UploadError::MessageNotFound)?;

    // Only message author can attach files (anonymized messages cannot have files added)
    if message.user_id != Some(auth_user.id) {
        return Err(UploadError::Forbidden);
    }

    // Generate S3 key
    let file_id = Uuid::now_v7();
    let extension = std::path::Path::new(&safe_filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let s3_key = format!(
        "attachments/{}/{}/{}.{}",
        message.channel_id, message_id, file_id, extension
    );

    // Process image before S3 upload (clones data internally for spawn_blocking)
    let file_size = file_data.len() as i64;
    let media = process_and_upload_variants(s3, &file_data, &content_type, &s3_key).await;

    // Upload original to S3
    if let Err(e) = s3.upload(&s3_key, file_data, &content_type).await {
        // Clean up orphaned variant objects
        let mut keys = Vec::new();
        if let Some(k) = media.thumb_key {
            keys.push(k);
        }
        if let Some(k) = media.medium_key {
            keys.push(k);
        }
        if !keys.is_empty() {
            cleanup_s3_objects(s3.clone(), keys);
        }
        return Err(UploadError::Storage(e.to_string()));
    }

    // Save metadata to database
    let attachment = db::create_file_attachment(
        &state.db,
        message_id,
        &safe_filename,
        &content_type,
        file_size,
        &s3_key,
        media.width,
        media.height,
        media.blurhash.as_deref(),
        media.thumb_key.as_deref(),
        media.medium_key.as_deref(),
        media.processing_status,
    )
    .await
    .map_err(|e| {
        // Clean up orphaned S3 objects (original + variants)
        let mut keys = vec![s3_key.clone()];
        if let Some(k) = media.thumb_key.clone() {
            keys.push(k);
        }
        if let Some(k) = media.medium_key.clone() {
            keys.push(k);
        }
        cleanup_s3_objects(s3.clone(), keys);
        tracing::error!(
            message_id = %message_id,
            "Failed to create attachment record, cleaning up S3 objects: {e}"
        );
        e
    })?;

    // Generate download URL
    let url = format!("/api/messages/attachments/{}", attachment.id);

    tracing::info!(
        attachment_id = %attachment.id,
        message_id = %message_id,
        filename = %safe_filename,
        size = file_size,
        "File uploaded successfully"
    );

    Ok((
        StatusCode::CREATED,
        Json(UploadedFile {
            id: attachment.id,
            filename: safe_filename,
            mime_type: content_type,
            size: file_size,
            url,
        }),
    ))
}

/// Upload a file and create a message in one request.
///
/// POST /`api/messages/channel/:channel_id/upload`
///
/// Expects multipart form with:
/// - `file`: The file data (required)
/// - `content`: Optional message text content
#[utoipa::path(
    post,
    path = "/api/messages/channel/{channel_id}/upload",
    tag = "uploads",
    params(("channel_id" = Uuid, Path, description = "Channel ID")),
    request_body(content = Vec<u8>, content_type = "multipart/form-data"),
    responses(
        (status = 201, body = MessageResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth_user, multipart))]
pub async fn upload_message_with_file(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(channel_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<MessageResponse>), UploadError> {
    // Check S3 is configured
    let s3 = state.s3.as_ref().ok_or(UploadError::NotConfigured)?;

    // Check channel exists
    let channel = db::find_channel_by_id(&state.db, channel_id)
        .await?
        .ok_or(UploadError::Validation("Channel not found".to_string()))?;

    // Check channel access (VIEW_CHANNEL permission or DM participant)
    let ctx = crate::permissions::require_channel_access(&state.db, auth_user.id, channel_id)
        .await
        .map_err(|_| UploadError::Forbidden)?;

    // For guild channels, also check SEND_MESSAGES permission
    if channel.guild_id.is_some()
        && !ctx.has_permission(crate::permissions::GuildPermissions::SEND_MESSAGES)
    {
        return Err(UploadError::Forbidden);
    }

    let mut file_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;
    let mut content: String = String::new();

    // Parse multipart form
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "file" => {
                filename = field.file_name().map(String::from);
                content_type = field.content_type().map(String::from);

                let data = field
                    .bytes()
                    .await
                    .map_err(|e| UploadError::Validation(e.to_string()))?;

                // Check file size
                if data.len() > state.config.max_upload_size {
                    return Err(UploadError::TooLarge {
                        max_size: state.config.max_upload_size,
                    });
                }

                file_data = Some(data.to_vec());
            }
            "content" => {
                content = field
                    .text()
                    .await
                    .map_err(|e| UploadError::Validation(e.to_string()))?;
            }
            _ => {
                // Ignore unknown fields
            }
        }
    }

    // Validate required fields
    let file_data = file_data.ok_or(UploadError::NoFile)?;
    let filename = filename.ok_or(UploadError::InvalidFilename)?;

    // Sanitize filename
    let safe_filename = sanitize_filename(&filename);
    if safe_filename.is_empty() {
        return Err(UploadError::InvalidFilename);
    }

    // Determine content type
    let file_content_type = content_type
        .or_else(|| {
            mime_guess::from_path(&filename)
                .first()
                .map(|m| m.to_string())
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());

    // Validate MIME type
    let allowed_types: Vec<&str> = state.config.allowed_mime_types.as_ref().map_or_else(
        || DEFAULT_ALLOWED_TYPES.to_vec(),
        |v| v.iter().map(std::string::String::as_str).collect(),
    );

    if !allowed_types.contains(&file_content_type.as_str()) {
        return Err(UploadError::InvalidMimeType {
            mime_type: file_content_type,
        });
    }

    // Validate actual file content matches claimed MIME type (magic byte check)
    let file_content_type = validate_file_content(&file_data, &file_content_type)?;

    // Validate message content length if provided
    if !content.is_empty() {
        super::messages::validate_message_content(&content)
            .map_err(|e| UploadError::Validation(e.to_string()))?;
    }
    // Content filtering on message text (if non-empty, guild channels only)
    if !content.is_empty() {
        if let Some(guild_id) = channel.guild_id {
            if let Ok(engine) = state.filter_cache.get_or_build(&state.db, guild_id).await {
                let result = engine.check(&content);
                if result.blocked {
                    for m in &result.matches {
                        crate::moderation::filter_queries::log_moderation_action(
                            &state.db,
                            &crate::moderation::filter_queries::LogActionParams {
                                guild_id,
                                user_id: auth_user.id,
                                channel_id,
                                action: m.action,
                                category: Some(m.category),
                                matched_pattern: &m.matched_pattern,
                                original_content: &content,
                                custom_pattern_id: m.custom_pattern_id,
                            },
                        )
                        .await
                        .ok();
                    }
                    return Err(UploadError::Validation(
                        "Your message was blocked by the server's content filter.".to_string(),
                    ));
                }
                // For "log" and "warn" actions, still log but allow the upload
                for m in result.matches.iter().filter(|m| {
                    m.action == crate::moderation::filter_types::FilterAction::Log
                        || m.action == crate::moderation::filter_types::FilterAction::Warn
                }) {
                    crate::moderation::filter_queries::log_moderation_action(
                        &state.db,
                        &crate::moderation::filter_queries::LogActionParams {
                            guild_id,
                            user_id: auth_user.id,
                            channel_id,
                            action: m.action,
                            category: Some(m.category),
                            matched_pattern: &m.matched_pattern,
                            original_content: &content,
                            custom_pattern_id: m.custom_pattern_id,
                        },
                    )
                    .await
                    .ok();
                }
            }
        }
    }

    // Create the message first
    // Note: Empty content is allowed when attaching files (file-only messages)
    // This differs from regular text messages which require validation:
    // - Regular text: <= 4000 characters (excluding fenced code blocks)
    // - Total: <= 10000 characters (including code blocks)
    let message = db::create_message(
        &state.db,
        channel_id,
        auth_user.id,
        &content,
        false, // encrypted
        None,  // nonce
        None,  // reply_to
    )
    .await?;

    // Generate S3 key using actual message ID
    let file_id = Uuid::now_v7();
    let extension = std::path::Path::new(&safe_filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let s3_key = format!(
        "attachments/{}/{}/{}.{}",
        channel_id, message.id, file_id, extension
    );

    // Process image before S3 upload (clones data internally for spawn_blocking)
    let file_size = file_data.len() as i64;
    let media = process_and_upload_variants(s3, &file_data, &file_content_type, &s3_key).await;

    // Upload original to S3 - if this fails, message is already created (acceptable trade-off)
    if let Err(e) = s3.upload(&s3_key, file_data, &file_content_type).await {
        // Clean up orphaned variant objects
        let mut keys = Vec::new();
        if let Some(k) = media.thumb_key {
            keys.push(k);
        }
        if let Some(k) = media.medium_key {
            keys.push(k);
        }
        if !keys.is_empty() {
            cleanup_s3_objects(s3.clone(), keys);
        }
        tracing::error!(
            "S3 upload failed for message {}: {}. Message exists without attachment.",
            message.id,
            e
        );
        return Err(UploadError::Storage(e.to_string()));
    }

    // Save attachment metadata to database
    let attachment = db::create_file_attachment(
        &state.db,
        message.id,
        &safe_filename,
        &file_content_type,
        file_size,
        &s3_key,
        media.width,
        media.height,
        media.blurhash.as_deref(),
        media.thumb_key.as_deref(),
        media.medium_key.as_deref(),
        media.processing_status,
    )
    .await
    .map_err(|e| {
        // If attachment record creation fails after S3 upload, we have orphaned S3 objects
        let mut keys = vec![s3_key.clone()];
        if let Some(k) = media.thumb_key.clone() {
            keys.push(k);
        }
        if let Some(k) = media.medium_key.clone() {
            keys.push(k);
        }
        cleanup_s3_objects(s3.clone(), keys);
        tracing::error!(
            "Failed to create attachment record for message {}: {}",
            message.id,
            e
        );
        e
    })?;

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

    let mention_type = detect_mention_type(&message.content, Some(&author.username));

    let response = MessageResponse {
        id: message.id,
        channel_id: message.channel_id,
        author: author.clone(),
        content: message.content,
        encrypted: message.encrypted,
        attachments: vec![AttachmentInfo::from_db(&attachment)],
        reply_to: message.reply_to,
        parent_id: message.parent_id,
        thread_reply_count: message.thread_reply_count,
        thread_last_reply_at: message.thread_last_reply_at,
        thread_info: None,
        edited_at: message.edited_at,
        created_at: message.created_at,
        mention_type,
        reactions: None,
    };

    // Broadcast new message via Redis pub-sub
    let message_json = serde_json::to_value(&response).unwrap_or_default();
    if let Err(e) = broadcast_to_channel(
        &state.redis,
        channel_id,
        &ServerEvent::MessageNew {
            channel_id,
            message: message_json,
        },
    )
    .await
    {
        tracing::error!(
            "Failed to broadcast message {} to channel {}: {}. Message saved but clients may not receive real-time update.",
            message.id,
            channel_id,
            e
        );
    }

    tracing::info!(
        message_id = %message.id,
        attachment_id = %attachment.id,
        filename = %safe_filename,
        size = file_size,
        "Message with file uploaded successfully"
    );

    Ok((StatusCode::CREATED, Json(response)))
}

/// Get attachment metadata.
///
/// GET /api/messages/attachments/:id
#[utoipa::path(
    get,
    path = "/api/messages/attachments/{id}",
    tag = "messages",
    params(("id" = Uuid, Path, description = "Attachment ID")),
    responses(
        (status = 200, description = "Attachment file"),
    ),
    security(("bearer_auth" = [])),
)]
pub async fn get_attachment(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<AttachmentResponse>, UploadError> {
    let has_access = db::check_attachment_access(&state.db, id, auth_user.id)
        .await
        .map_err(UploadError::Database)?;

    if !has_access {
        return Err(UploadError::Forbidden);
    }

    let attachment = db::find_file_attachment_by_id(&state.db, id)
        .await?
        .ok_or(UploadError::NotFound)?;

    Ok(Json(attachment.into()))
}

/// Query parameters for download endpoint.
#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    /// Optional JWT token for authentication (alternative to Authorization header).
    /// Used for browser requests like <img src="..."> that can't set headers.
    pub token: Option<String>,
    /// Optional variant to download: "thumbnail" (256px) or "medium" (1024px).
    pub variant: Option<String>,
}

/// Download a file (stream from S3).
///
/// GET /api/messages/attachments/:id/download
///
/// Supports two authentication methods:
/// 1. Authorization header (Bearer token) - standard API auth
/// 2. `token` query parameter - for browser requests that can't set headers
#[utoipa::path(
    get,
    path = "/api/messages/attachments/{id}/download",
    tag = "messages",
    params(("id" = Uuid, Path, description = "Attachment ID")),
    responses(
        (status = 200, description = "File download"),
    ),
    security(),
)]
pub async fn download(
    State(state): State<AppState>,
    auth_user: Option<AuthUser>,
    Path(id): Path<Uuid>,
    Query(query): Query<DownloadQuery>,
) -> Result<Response, UploadError> {
    // Check S3 is configured
    let s3 = state.s3.as_ref().ok_or(UploadError::NotConfigured)?;

    // Get user ID from either AuthUser (header) or token query parameter
    let user_id = if let Some(user) = auth_user {
        user.id
    } else if let Some(token) = query.token {
        // Validate token from query parameter
        let claims = validate_access_token(&token, &state.config.jwt_public_key)
            .map_err(|_| UploadError::Forbidden)?;
        claims
            .sub
            .parse::<Uuid>()
            .map_err(|_| UploadError::Forbidden)?
    } else {
        return Err(UploadError::Forbidden);
    };

    // Check permissions
    let has_access = db::check_attachment_access(&state.db, id, user_id)
        .await
        .map_err(UploadError::Database)?;

    if !has_access {
        return Err(UploadError::Forbidden);
    }

    // Get attachment metadata
    let attachment = db::find_file_attachment_by_id(&state.db, id)
        .await?
        .ok_or(UploadError::NotFound)?;

    // Determine S3 key and content type based on requested variant
    let (s3_key, content_type) = match query.variant.as_deref() {
        Some("thumbnail") => {
            let key = attachment
                .thumbnail_s3_key
                .as_deref()
                .unwrap_or(&attachment.s3_key);
            let ct = if attachment.thumbnail_s3_key.is_some() {
                "image/webp".to_string()
            } else {
                attachment.mime_type.clone()
            };
            (key.to_string(), ct)
        }
        Some("medium") => {
            let key = attachment
                .medium_s3_key
                .as_deref()
                .unwrap_or(&attachment.s3_key);
            let ct = if attachment.medium_s3_key.is_some() {
                "image/webp".to_string()
            } else {
                attachment.mime_type.clone()
            };
            (key.to_string(), ct)
        }
        Some(invalid) => {
            return Err(UploadError::Validation(format!(
                "Invalid variant '{invalid}'. Supported values are 'thumbnail' and 'medium'"
            )));
        }
        None => (attachment.s3_key.clone(), attachment.mime_type.clone()),
    };

    // Fetch from S3
    let stream = s3
        .get_object_stream(&s3_key)
        .await
        .map_err(|e| UploadError::Storage(e.to_string()))?;

    // Create stream body
    // ByteStream can be converted directly to Axum Body via into_inner()
    let sdk_body = stream.into_inner();
    let body = axum::body::Body::new(sdk_body);

    // Adjust filename extension when serving a WebP variant
    let display_filename = if content_type == "image/webp" && content_type != attachment.mime_type {
        let stem = attachment
            .filename
            .rsplit_once('.')
            .map_or(attachment.filename.as_str(), |(stem, _)| stem);
        format!("{stem}.webp")
    } else {
        attachment.filename.clone()
    };

    // Set headers
    let disposition = if content_type.starts_with("image/")
        || content_type.starts_with("video/")
        || content_type.starts_with("audio/")
    {
        "inline"
    } else {
        "attachment"
    };
    let headers = [
        (axum::http::header::CONTENT_TYPE, content_type.clone()),
        (
            axum::http::header::CONTENT_DISPOSITION,
            format!("{disposition}; filename=\"{display_filename}\""),
        ),
        (
            axum::http::header::CACHE_CONTROL,
            "private, max-age=31536000, immutable".to_string(),
        ),
        (
            HeaderName::from_static("x-content-type-options"),
            "nosniff".to_string(),
        ),
    ];

    Ok((headers, body).into_response())
}

// ============================================================================
// Helpers
// ============================================================================

/// Output of image processing + variant S3 upload pipeline.
struct MediaProcessingOutput {
    width: Option<i32>,
    height: Option<i32>,
    blurhash: Option<String>,
    thumb_key: Option<String>,
    medium_key: Option<String>,
    processing_status: &'static str,
}

/// Process an image and upload thumbnail/medium variants to S3.
///
/// Returns metadata for storing in the database. Processing failures are
/// logged and result in `processing_status = "failed"` — they never propagate
/// as errors to avoid blocking the upload.
async fn process_and_upload_variants(
    s3: &S3Client,
    file_data: &[u8],
    content_type: &str,
    base_s3_key: &str,
) -> MediaProcessingOutput {
    if !content_type.starts_with("image/") {
        return MediaProcessingOutput {
            width: None,
            height: None,
            blurhash: None,
            thumb_key: None,
            medium_key: None,
            processing_status: "skipped",
        };
    }

    // process_image takes &[u8] but spawn_blocking needs 'static
    let data = file_data.to_vec();
    let mime = content_type.to_string();
    let meta = match tokio::task::spawn_blocking(move || {
        super::media_processing::process_image(&data, &mime)
    })
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Image processing failed, storing without variants");
            return MediaProcessingOutput {
                width: None,
                height: None,
                blurhash: None,
                thumb_key: None,
                medium_key: None,
                processing_status: "failed",
            };
        }
        Err(e) => {
            tracing::warn!(error = %e, "Image processing task panicked");
            return MediaProcessingOutput {
                width: None,
                height: None,
                blurhash: None,
                thumb_key: None,
                medium_key: None,
                processing_status: "failed",
            };
        }
    };

    // Upload variants to S3
    let base_key = base_s3_key
        .rsplit_once('.')
        .map_or(base_s3_key, |(base, _)| base);

    let thumb_key = if let Some(ref thumb) = meta.thumbnail {
        let key = format!("{base_key}_thumb.webp");
        if let Err(e) = s3
            .upload(&key, thumb.data.clone(), &thumb.content_type)
            .await
        {
            tracing::warn!(error = %e, "Failed to upload thumbnail variant");
            None
        } else {
            tracing::debug!(
                width = thumb.width,
                height = thumb.height,
                "Uploaded thumbnail variant"
            );
            Some(key)
        }
    } else {
        None
    };

    let medium_key = if let Some(ref medium) = meta.medium {
        let key = format!("{base_key}_medium.webp");
        if let Err(e) = s3
            .upload(&key, medium.data.clone(), &medium.content_type)
            .await
        {
            tracing::warn!(error = %e, "Failed to upload medium variant");
            None
        } else {
            tracing::debug!(
                width = medium.width,
                height = medium.height,
                "Uploaded medium variant"
            );
            Some(key)
        }
    } else {
        None
    };

    // Determine processing status: "processed" if all expected variants uploaded,
    // "partial" if some variant uploads failed
    let expected_thumb = meta.thumbnail.is_some();
    let expected_medium = meta.medium.is_some();
    let all_uploaded =
        (!expected_thumb || thumb_key.is_some()) && (!expected_medium || medium_key.is_some());
    let processing_status = if all_uploaded { "processed" } else { "partial" };

    MediaProcessingOutput {
        width: Some(meta.width.min(i32::MAX as u32) as i32),
        height: Some(meta.height.min(i32::MAX as u32) as i32),
        blurhash: Some(meta.blurhash),
        thumb_key,
        medium_key,
        processing_status,
    }
}

/// Clean up S3 objects in the background (used when DB insert fails).
fn cleanup_s3_objects(s3: S3Client, keys: Vec<String>) {
    tokio::spawn(async move {
        for key in keys {
            if let Err(e) = s3.delete(&key).await {
                tracing::error!("Failed to cleanup orphaned S3 object {}: {}", key, e);
            }
        }
    });
}

/// Sanitize a filename to prevent path traversal and other issues.
fn sanitize_filename(filename: &str) -> String {
    // Extract just the filename part (no directory components)
    let name = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // Remove dangerous characters, keep alphanumeric, dots, dashes, underscores
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .take(255)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("test.png"), "test.png");
        assert_eq!(sanitize_filename("../../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("file-name_123.jpg"), "file-name_123.jpg");
        assert_eq!(sanitize_filename(""), "");
        assert_eq!(sanitize_filename("test<script>.png"), "testscript.png");
    }

    #[test]
    fn test_sanitize_removes_spaces() {
        // Spaces are removed (not in allowed chars)
        assert_eq!(
            sanitize_filename("file with spaces.jpg"),
            "filewithspaces.jpg"
        );
    }

    #[test]
    fn test_sanitize_truncates_long_names() {
        let long_name = "a".repeat(300) + ".txt";
        let result = sanitize_filename(&long_name);
        assert!(result.len() <= 255);
    }
}
