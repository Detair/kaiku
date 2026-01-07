//! Database Models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// User model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub password_hash: Option<String>,
    pub auth_method: AuthMethod,
    pub external_id: Option<String>,
    pub avatar_url: Option<String>,
    pub status: UserStatus,
    pub mfa_secret: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Authentication method.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "auth_method", rename_all = "lowercase")]
pub enum AuthMethod {
    Local,
    Oidc,
}

/// User online status.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_status", rename_all = "lowercase")]
pub enum UserStatus {
    Online,
    Away,
    Busy,
    Offline,
}

/// Channel model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Channel {
    pub id: Uuid,
    pub name: String,
    pub channel_type: ChannelType,
    pub category_id: Option<Uuid>,
    pub topic: Option<String>,
    pub user_limit: Option<i32>,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Channel type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "channel_type", rename_all = "lowercase")]
pub enum ChannelType {
    Text,
    Voice,
    Dm,
}

/// Message model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub content: String,
    pub encrypted: bool,
    pub nonce: Option<String>,
    pub reply_to: Option<Uuid>,
    pub edited_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Role model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Role {
    pub id: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub permissions: serde_json::Value,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}

/// Channel member model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ChannelMember {
    pub channel_id: Uuid,
    pub user_id: Uuid,
    pub role_id: Option<Uuid>,
    pub joined_at: DateTime<Utc>,
}

/// File attachment model.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FileAttachment {
    pub id: Uuid,
    pub message_id: Uuid,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub s3_key: String,
    pub created_at: DateTime<Utc>,
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
