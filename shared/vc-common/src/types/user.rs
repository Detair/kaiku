//! User Types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// User status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    /// User is online.
    Online,
    /// User is idle/away.
    Away,
    /// User is busy (do not disturb).
    Busy,
    /// User is offline.
    Offline,
}

impl Default for UserStatus {
    fn default() -> Self {
        Self::Offline
    }
}

/// User profile (public information).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// User ID.
    pub id: Uuid,
    /// Username (unique).
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Avatar image URL.
    pub avatar_url: Option<String>,
    /// Current status.
    pub status: UserStatus,
}

/// Full user data (for authenticated user).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// User ID.
    pub id: Uuid,
    /// Username (unique).
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Email address.
    pub email: Option<String>,
    /// Avatar image URL.
    pub avatar_url: Option<String>,
    /// Current status.
    pub status: UserStatus,
    /// Whether MFA is enabled.
    pub mfa_enabled: bool,
    /// When user was created.
    pub created_at: DateTime<Utc>,
}
