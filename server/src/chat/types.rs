//! Chat DTOs
//!
//! Request and response types for the chat HTTP API — channels, messages,
//! direct messages, permission overrides, uploads, and DM search.
//! Internal database row structs and processing helpers that never cross
//! the API boundary intentionally stay in their handler modules.

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::db::{self, ChannelType};

// ============================================================================
// Channels
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

/// Request body for marking a guild channel as read.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MarkChannelAsReadRequest {
    pub last_read_message_id: Uuid,
}

// ============================================================================
// Messages
// ============================================================================

/// Author profile for message responses.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
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
            avatar_url: super::files::maybe_file_url(user.avatar_url),
            status: format!("{:?}", user.status).to_lowercase(),
        }
    }
}

/// Attachment info for message responses (matches client Attachment type).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AttachmentInfo {
    pub id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medium_url: Option<String>,
}

impl AttachmentInfo {
    /// Create from a `FileAttachment` database model.
    pub fn from_db(attachment: &db::FileAttachment) -> Self {
        let base_url = format!("/api/messages/attachments/{}/download", attachment.id);
        let thumbnail_url = attachment
            .thumbnail_s3_key
            .as_ref()
            .map(|_| format!("{base_url}?variant=thumbnail"));
        let medium_url = attachment
            .medium_s3_key
            .as_ref()
            .map(|_| format!("{base_url}?variant=medium"));
        Self {
            id: attachment.id,
            filename: attachment.filename.clone(),
            mime_type: attachment.mime_type.clone(),
            size: attachment.size_bytes,
            url: base_url,
            width: attachment.width,
            height: attachment.height,
            blurhash: attachment.blurhash.clone(),
            thumbnail_url,
            medium_url,
        }
    }
}

/// Mention type for notification sounds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MentionType {
    /// Direct @username mention
    Direct,
    /// @everyone mention
    Everyone,
    /// @here mention (online users only)
    Here,
}

/// Thread info for parent messages (returned alongside message responses).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ThreadInfoResponse {
    pub reply_count: i32,
    pub last_reply_at: Option<DateTime<Utc>>,
    pub participant_ids: Vec<Uuid>,
    pub participant_avatars: Vec<Option<String>>,
    /// Whether the thread has unread replies for the requesting user.
    /// Only populated for authenticated message list requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_unread: Option<bool>,
}

/// Full message response with author info (matches client Message type).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct MessageResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub author: AuthorProfile,
    pub content: String,
    pub encrypted: bool,
    pub attachments: Vec<AttachmentInfo>,
    pub reply_to: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub thread_reply_count: i32,
    pub thread_last_reply_at: Option<DateTime<Utc>>,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// Type of mention in this message (for notification sounds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mention_type: Option<MentionType>,
    /// Reactions on this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactions: Option<Vec<ReactionInfo>>,
    /// Thread info (only present for messages with thread replies).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_info: Option<ThreadInfoResponse>,
    /// Whether this message is pinned in its channel.
    #[serde(default)]
    pub pinned: bool,
    /// Message type: "user" for normal messages, "system" for system events.
    pub message_type: String,
    /// Client-generated nonce for optimistic message matching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Rich embeds (bot-authored). Omitted for plain messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embeds: Option<Vec<crate::chat::embeds::Embed>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReactionInfo {
    pub emoji: String,
    pub count: i64,
    pub me: bool,
    pub users: Vec<Uuid>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListMessagesQuery {
    pub before: Option<Uuid>,
    #[serde(default)]
    pub around: Option<Uuid>,
    #[serde(default = "default_message_limit")]
    pub limit: i64,
}

pub(super) const fn default_message_limit() -> i64 {
    50
}

/// Cursor-based paginated response wrapper.
///
/// Used for endpoints with infinite scroll patterns (e.g., message history).
/// Unlike offset-based pagination (which uses `total`, `limit`, `offset`),
/// cursor-based pagination uses a reference point (`next_cursor`) and
/// indicates whether more items exist (`has_more`).
///
/// This is more efficient for large datasets where counting total items
/// is expensive and unnecessary (e.g., chat messages).
#[derive(Debug, Serialize, ToSchema)]
pub struct CursorPaginatedResponse<T> {
    /// The items for this page.
    pub items: Vec<T>,
    /// Whether more items exist beyond this page.
    pub has_more: bool,
    /// Cursor for fetching the next page (ID of the oldest item).
    /// Pass this as `before` parameter to get the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateMessageRequest {
    #[validate(custom(function = "validate_message_content"))]
    pub content: String,
    #[serde(default)]
    pub encrypted: bool,
    #[validate(length(max = 64))]
    pub nonce: Option<String>,
    pub reply_to: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    /// Rich embeds — accepted from bot authors only.
    #[serde(default)]
    pub embeds: Option<Vec<crate::chat::embeds::Embed>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListThreadRepliesQuery {
    pub after: Option<Uuid>,
    #[serde(default = "default_message_limit")]
    pub limit: i64,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateMessageRequest {
    #[validate(custom(function = "validate_message_content"))]
    pub content: String,
}

/// Detect mention type in message content.
/// Returns the highest priority mention type found.
pub fn detect_mention_type(content: &str, author_username: Option<&str>) -> Option<MentionType> {
    static MENTION_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"@(\w+)").unwrap());

    // Check for @everyone first (highest priority for notifications)
    if content.contains("@everyone") {
        return Some(MentionType::Everyone);
    }

    // Check for @here
    if content.contains("@here") {
        return Some(MentionType::Here);
    }

    // Check for direct @username mentions (excluding self-mentions)
    let mention_pattern = &*MENTION_RE;
    for cap in mention_pattern.captures_iter(content) {
        let mentioned = &cap[1];
        // Skip if mentioning self
        if let Some(author) = author_username {
            if mentioned.eq_ignore_ascii_case(author) {
                continue;
            }
        }
        // Any other @mention is a direct mention
        if mentioned != "everyone" && mentioned != "here" {
            return Some(MentionType::Direct);
        }
    }

    None
}

static CODE_BLOCK_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?s)```.*?```").unwrap());

// NOTE: This regex handles standard triple-backtick fences (```...```).
// Known limitations: fences with >3 backticks (e.g., ````) and triple backticks
// inside fenced blocks may be miscounted. A full CommonMark parser would handle
// these edge cases but is not warranted for validation purposes.
/// Custom validation for message content length
/// Standard messages: 1-4000 characters
/// Messages with code blocks: 1-10000 characters
pub fn validate_message_content(content: &str) -> Result<(), validator::ValidationError> {
    if content.is_empty() {
        return Err(validator::ValidationError::new("content_empty")
            .with_message("Content cannot be empty".into()));
    }

    let total_len = content.chars().count();

    // Overall hard limit of 10,000 characters
    if total_len > 10000 {
        return Err(validator::ValidationError::new("length").with_message(
            std::borrow::Cow::Borrowed("Content cannot exceed 10000 characters in total"),
        ));
    }

    // Strip code blocks to check the regular text limit (4,000 chars)
    let text_without_blocks = CODE_BLOCK_RE.replace_all(content, "");

    let regular_text_len = text_without_blocks.chars().count();

    if regular_text_len > 4000 {
        return Err(validator::ValidationError::new("length").with_message(
            std::borrow::Cow::Borrowed("Regular text content cannot exceed 4000 characters"),
        ));
    }

    Ok(())
}

// ============================================================================
// Direct Messages
// ============================================================================

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateDMRequest {
    /// User ID(s) to create DM with (1 for 1:1, multiple for group DM)
    #[validate(length(min = 1, max = 9, message = "Must have 1-9 other participants"))]
    pub participant_ids: Vec<Uuid>,
    /// Optional name for group DMs
    #[validate(length(max = 100))]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DMResponse {
    #[serde(flatten)]
    #[schema(inline)]
    pub channel: ChannelResponse,
    pub participants: Vec<DMParticipant>,
}

/// Last message info for DM list preview
#[derive(Debug, sqlx::FromRow, Serialize, utoipa::ToSchema)]
pub struct LastMessagePreview {
    pub id: Uuid,
    pub content: String,
    pub user_id: Option<Uuid>,
    pub username: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Enhanced DM response with unread count and last message
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DMListResponse {
    #[serde(flatten)]
    #[schema(inline)]
    pub channel: ChannelResponse,
    pub participants: Vec<DMParticipant>,
    pub last_message: Option<LastMessagePreview>,
    pub unread_count: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DMParticipant {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub joined_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateDMNameRequest {
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: String,
}

/// Response for DM icon upload
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DMIconResponse {
    pub icon_url: String,
}

/// Mark DM as read request body
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MarkAsReadRequest {
    pub last_read_message_id: Uuid,
}

/// Mark DM as read response
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MarkAsReadResponse {
    pub channel_id: Uuid,
    pub last_read_at: chrono::DateTime<chrono::Utc>,
    pub last_read_message_id: Option<Uuid>,
    pub unread_count: i64,
}

// ============================================================================
// Permission Overrides
// ============================================================================

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OverrideResponse {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub role_id: Uuid,
    pub allow_permissions: u64,
    pub deny_permissions: u64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetOverrideRequest {
    pub allow: Option<u64>,
    pub deny: Option<u64>,
}

// ============================================================================
// Uploads
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

/// Query parameters for signed URL endpoint.
#[derive(Debug, Deserialize)]
pub struct SignedUrlQuery {
    /// Optional variant: "thumbnail" (256px) or "medium" (1024px).
    pub variant: Option<String>,
}

/// Response containing a presigned S3 download URL.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SignedUrlResponse {
    /// Presigned S3 URL for direct download.
    pub url: String,
    /// Duration in seconds from generation until the URL expires.
    pub expires_in: i64,
}

/// Query parameters for download endpoint.
#[derive(Debug, Deserialize)]
pub struct DownloadQuery {
    /// **Deprecated:** Use `GET /api/messages/attachments/<id>/url` with Authorization header
    /// instead. When present, authenticates via this JWT token instead of the Authorization
    /// header.
    pub token: Option<String>,
    /// Optional variant to download: "thumbnail" (256px) or "medium" (1024px).
    pub variant: Option<String>,
}

// ============================================================================
// DM Search
// ============================================================================

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DmSearchQuery {
    /// Search query string (supports websearch syntax: AND, OR, quotes)
    pub q: String,
    /// Maximum results to return (default 25, max 100)
    #[serde(default = "default_dm_search_limit")]
    pub limit: i64,
    /// Offset for pagination (default 0)
    #[serde(default)]
    pub offset: i64,
    /// Filter: only messages after this date (ISO 8601)
    pub date_from: Option<DateTime<Utc>>,
    /// Filter: only messages before this date (ISO 8601)
    pub date_to: Option<DateTime<Utc>>,
    /// Filter: only messages in this DM channel
    pub channel_id: Option<Uuid>,
    /// Filter: only messages by this author
    pub author_id: Option<Uuid>,
    /// Filter: "link" or "file"
    pub has: Option<String>,
    /// Sort order: "relevance" (default) or "date"
    pub sort: Option<String>,
}

const fn default_dm_search_limit() -> i64 {
    25
}

/// Author info for search results.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DmSearchAuthor {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// DM search result item.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DmSearchResult {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub author: DmSearchAuthor,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub headline: String,
    pub rank: f32,
}

/// DM search response with results and pagination.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DmSearchResponse {
    pub results: Vec<DmSearchResult>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}
