//! Channel Types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Channel type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelType {
    /// Text chat channel.
    Text,
    /// Voice channel.
    Voice,
    /// Direct message channel.
    Dm,
}

/// Channel data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    /// Unique channel ID.
    pub id: Uuid,
    /// Channel name.
    pub name: String,
    /// Channel type.
    pub channel_type: ChannelType,
    /// Parent category ID.
    pub category_id: Option<Uuid>,
    /// Channel description/topic.
    pub topic: Option<String>,
    /// Max users in voice channel.
    pub user_limit: Option<u32>,
    /// Display position.
    pub position: i32,
    /// When created.
    pub created_at: DateTime<Utc>,
}

/// Channel category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelCategory {
    /// Category ID.
    pub id: Uuid,
    /// Category name.
    pub name: String,
    /// Display position.
    pub position: i32,
    /// Channels in this category.
    pub channels: Vec<Channel>,
}
