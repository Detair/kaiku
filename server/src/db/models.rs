//! Database Models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// User model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    /// Unique user ID.
    pub id: Uuid,
    /// Unique username for login.
    pub username: String,
    /// Display name shown to other users.
    pub display_name: String,
    /// Email address (optional).
    pub email: Option<String>,
    /// Argon2id password hash (for local auth).
    pub password_hash: Option<String>,
    /// Authentication method (local or OIDC).
    pub auth_method: AuthMethod,
    /// External ID from OIDC provider.
    pub external_id: Option<String>,
    /// Avatar image URL.
    pub avatar_url: Option<String>,
    /// Current online status.
    pub status: UserStatus,
    /// Encrypted MFA secret for TOTP.
    pub mfa_secret: Option<String>,
    /// Whether this user account is a bot.
    pub is_bot: bool,
    /// The user who owns this bot (only set for bot users).
    pub bot_owner_id: Option<Uuid>,
    /// When the user requested account deletion.
    pub deletion_requested_at: Option<DateTime<Utc>>,
    /// When the account is scheduled to be hard-deleted.
    pub deletion_scheduled_at: Option<DateTime<Utc>>,
    /// When the user was created.
    pub created_at: DateTime<Utc>,
    /// When the user was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Authentication method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "auth_method", rename_all = "lowercase")]
pub enum AuthMethod {
    /// Local password authentication.
    Local,
    /// `OpenID` Connect authentication.
    Oidc,
}

/// User online status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "user_status", rename_all = "lowercase")]
pub enum UserStatus {
    /// User is actively using the app.
    Online,
    /// User is idle.
    Away,
    /// User is busy (do not disturb).
    Busy,
    /// User is offline.
    Offline,
}

/// Default value for `max_screen_shares` field.
const fn default_max_screen_shares() -> i32 {
    1
}

/// Channel model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Channel {
    /// Unique channel ID.
    pub id: Uuid,
    /// Channel name.
    pub name: String,
    /// Channel type (text, voice, or DM).
    pub channel_type: ChannelType,
    /// Parent category ID (for organization).
    pub category_id: Option<Uuid>,
    /// Guild this channel belongs to (None for DMs).
    pub guild_id: Option<Uuid>,
    /// Channel description/topic.
    pub topic: Option<String>,
    /// Channel icon URL (for DMs/Group DMs).
    pub icon_url: Option<String>,
    /// Max users allowed in voice channel.
    pub user_limit: Option<i32>,
    /// Display position in channel list.
    pub position: i32,
    /// Maximum concurrent screen shares (voice channels only).
    #[serde(default = "default_max_screen_shares")]
    pub max_screen_shares: i32,
    /// When the channel was created.
    pub created_at: DateTime<Utc>,
    /// When the channel was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Channel type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "channel_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    /// Text chat channel.
    Text,
    /// Voice channel.
    Voice,
    /// Direct message channel.
    Dm,
}

/// Message model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Message {
    /// Unique message ID.
    pub id: Uuid,
    /// Channel this message belongs to.
    pub channel_id: Uuid,
    /// User who sent the message (None if user was deleted/anonymized).
    pub user_id: Option<Uuid>,
    /// Message content (plaintext or encrypted).
    pub content: String,
    /// Whether the message is E2EE encrypted.
    pub encrypted: bool,
    /// Encryption nonce (for E2EE).
    pub nonce: Option<String>,
    /// Message ID this is replying to.
    pub reply_to: Option<Uuid>,
    /// Thread parent message ID (NULL for top-level messages).
    pub parent_id: Option<Uuid>,
    /// Number of replies in this thread (only meaningful for parent messages).
    #[serde(default)]
    pub thread_reply_count: i32,
    /// Timestamp of the last reply in this thread.
    pub thread_last_reply_at: Option<DateTime<Utc>>,
    /// When the message was edited.
    pub edited_at: Option<DateTime<Utc>>,
    /// When the message was deleted (soft delete).
    pub deleted_at: Option<DateTime<Utc>>,
    /// When the message was created.
    pub created_at: DateTime<Utc>,
}

/// Role model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Role {
    /// Unique role ID.
    pub id: Uuid,
    /// Role name.
    pub name: String,
    /// Display color (hex code).
    pub color: Option<String>,
    /// Permission flags (JSON object).
    #[schema(value_type = Object)]
    pub permissions: serde_json::Value,
    /// Display position in role hierarchy.
    pub position: i32,
    /// When the role was created.
    pub created_at: DateTime<Utc>,
}

/// Channel member model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChannelMember {
    /// Channel ID.
    pub channel_id: Uuid,
    /// User ID.
    pub user_id: Uuid,
    /// User's role in this channel.
    pub role_id: Option<Uuid>,
    /// When the user joined the channel.
    pub joined_at: DateTime<Utc>,
}

/// File attachment model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FileAttachment {
    /// Unique attachment ID.
    pub id: Uuid,
    /// Message this attachment belongs to.
    pub message_id: Uuid,
    /// Original filename.
    pub filename: String,
    /// MIME type (e.g., image/png).
    pub mime_type: String,
    /// File size in bytes.
    pub size_bytes: i64,
    /// S3 object key for retrieval.
    pub s3_key: String,
    /// When the attachment was uploaded.
    pub created_at: DateTime<Utc>,
    /// Image width in pixels (if image).
    pub width: Option<i32>,
    /// Image height in pixels (if image).
    pub height: Option<i32>,
    /// Blurhash placeholder string (if image).
    pub blurhash: Option<String>,
    /// S3 key for 256px thumbnail variant (if image).
    pub thumbnail_s3_key: Option<String>,
    /// S3 key for 1024px medium variant (if image).
    pub medium_s3_key: Option<String>,
    /// Processing status: pending, processed, failed, skipped.
    pub processing_status: String,
}

/// Session model for refresh token tracking.
#[derive(Debug, Clone, FromRow)]
pub struct Session {
    /// Session ID.
    pub id: Uuid,
    /// User this session belongs to.
    pub user_id: Uuid,
    /// SHA256 hash of the refresh token.
    pub token_hash: String,
    /// When the session/token expires.
    pub expires_at: DateTime<Utc>,
    /// IP address of the client (stored as string for simplicity).
    pub ip_address: Option<String>,
    /// User agent of the client.
    pub user_agent: Option<String>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
}

/// MFA backup code model.
#[derive(Debug, Clone, FromRow)]
pub struct MfaBackupCode {
    /// Code ID.
    pub id: Uuid,
    /// User this code belongs to.
    pub user_id: Uuid,
    /// Argon2id hash of the backup code.
    pub code_hash: String,
    /// When the code was used (None if still valid).
    pub used_at: Option<DateTime<Utc>>,
    /// When the code was created.
    pub created_at: DateTime<Utc>,
}

/// Password reset token model.
#[derive(Debug, Clone, FromRow)]
pub struct PasswordResetToken {
    /// Token ID.
    pub id: Uuid,
    /// User this token belongs to.
    pub user_id: Uuid,
    /// SHA256 hash of the reset token.
    pub token_hash: String,
    /// When the token expires.
    pub expires_at: DateTime<Utc>,
    /// When the token was used (None if unused).
    pub used_at: Option<DateTime<Utc>>,
    /// When the token was created.
    pub created_at: DateTime<Utc>,
}

/// OIDC/OAuth2 provider configuration stored in the database.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OidcProviderRow {
    /// Unique provider ID.
    pub id: Uuid,
    /// URL-safe slug for routing (e.g., "github", "google", "my-keycloak").
    pub slug: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Lucide icon hint (e.g., "github", "chrome", "key").
    pub icon_hint: Option<String>,
    /// Provider type: "preset" or "custom".
    pub provider_type: String,
    /// OIDC discovery URL (None for manual config like GitHub).
    pub issuer_url: Option<String>,
    /// Manual authorization endpoint.
    pub authorization_url: Option<String>,
    /// Manual token endpoint.
    pub token_url: Option<String>,
    /// Manual userinfo endpoint.
    pub userinfo_url: Option<String>,
    /// `OAuth2` client ID.
    pub client_id: String,
    /// Encrypted client secret (AES-256-GCM).
    pub client_secret_encrypted: String,
    /// `OAuth2` scopes (space or comma separated).
    pub scopes: String,
    /// Whether this provider is enabled.
    pub enabled: bool,
    /// Display position.
    pub position: i32,
    /// When the provider was created.
    pub created_at: DateTime<Utc>,
    /// When the provider was last updated.
    pub updated_at: DateTime<Utc>,
    /// User who created the provider.
    pub created_by: Option<Uuid>,
}

/// Public-facing OIDC provider info (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PublicOidcProvider {
    /// URL-safe slug.
    pub slug: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Lucide icon hint.
    pub icon_hint: Option<String>,
}

/// Auth methods configuration.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AuthMethodsConfig {
    /// Whether local (password) auth is allowed.
    pub local: bool,
    /// Whether OIDC/SSO auth is allowed.
    pub oidc: bool,
}

impl Default for AuthMethodsConfig {
    fn default() -> Self {
        Self {
            local: true,
            oidc: false,
        }
    }
}

/// Channel unread count for a specific channel.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChannelUnread {
    /// Channel ID.
    pub channel_id: Uuid,
    /// Channel name.
    pub channel_name: String,
    /// Number of unread messages.
    pub unread_count: i64,
}

/// Guild unread summary for the unread aggregator.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GuildUnreadSummary {
    /// Guild ID.
    pub guild_id: Uuid,
    /// Guild name.
    pub guild_name: String,
    /// Channels with unread messages in this guild.
    pub channels: Vec<ChannelUnread>,
    /// Total unread count for this guild.
    pub total_unread: i64,
}

/// Aggregate unread counts across all guilds and DMs.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UnreadAggregate {
    /// Guild-based unreads.
    pub guilds: Vec<GuildUnreadSummary>,
    /// DM unreads.
    pub dms: Vec<ChannelUnread>,
    /// Total unread count across everything.
    pub total: i64,
}

/// Data export job model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DataExportJob {
    /// Unique job ID.
    pub id: Uuid,
    /// User who requested the export.
    pub user_id: Uuid,
    /// Job status: pending, processing, completed, failed, expired.
    pub status: String,
    /// S3 object key for the export archive.
    pub s3_key: Option<String>,
    /// Export archive size in bytes.
    pub file_size_bytes: Option<i64>,
    /// Error message if the job failed.
    pub error_message: Option<String>,
    /// When the export download expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job completed.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Bot application model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BotApplication {
    /// Unique application ID.
    pub id: Uuid,
    /// User who owns this application.
    pub owner_id: Uuid,
    /// Application name.
    pub name: String,
    /// Application description.
    pub description: Option<String>,
    /// Associated bot user ID.
    pub bot_user_id: Option<Uuid>,
    /// Argon2id hash of the bot token.
    pub token_hash: Option<String>,
    /// Whether the bot is listed publicly.
    pub public: bool,
    /// When the application was created.
    pub created_at: DateTime<Utc>,
    /// When the application was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Slash command model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SlashCommand {
    /// Unique command ID.
    pub id: Uuid,
    /// Application this command belongs to.
    pub application_id: Uuid,
    /// Guild this command is registered in (None for global commands).
    pub guild_id: Option<Uuid>,
    /// Command name.
    pub name: String,
    /// Command description.
    pub description: String,
    /// Command options/parameters as JSON.
    #[schema(value_type = Option<Object>)]
    pub options: Option<serde_json::Value>,
    /// When the command was created.
    pub created_at: DateTime<Utc>,
    /// When the command was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Guild bot installation model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GuildBotInstallation {
    /// Unique installation ID.
    pub id: Uuid,
    /// Guild where the bot is installed.
    pub guild_id: Uuid,
    /// Bot application that is installed.
    pub application_id: Uuid,
    /// User who installed the bot.
    pub installed_by: Uuid,
    /// When the bot was installed.
    pub installed_at: DateTime<Utc>,
}
