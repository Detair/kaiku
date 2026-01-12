//! Message Types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::UserProfile;

/// Message data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Message ID.
    pub id: Uuid,
    /// Channel containing message.
    pub channel_id: Uuid,
    /// Message author.
    pub author: UserProfile,
    /// Message content.
    pub content: String,
    /// Whether E2EE encrypted.
    pub encrypted: bool,
    /// File attachments.
    pub attachments: Vec<Attachment>,
    /// Message being replied to.
    pub reply_to: Option<Uuid>,
    /// When edited.
    pub edited_at: Option<DateTime<Utc>>,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// File attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Attachment ID.
    pub id: Uuid,
    /// Original filename.
    pub filename: String,
    /// MIME type.
    pub mime_type: String,
    /// File size in bytes.
    pub size: u64,
    /// Download URL.
    pub url: String,
}
