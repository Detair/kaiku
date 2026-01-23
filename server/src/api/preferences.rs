//! User Preferences API
//!
//! Endpoints for managing user preferences that sync across devices.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Response for preferences endpoints
#[derive(Debug, Serialize)]
pub struct PreferencesResponse {
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

/// Request body for updating preferences
#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub preferences: serde_json::Value,
}

/// Database row for user_preferences
#[derive(Debug, sqlx::FromRow)]
pub struct UserPreferencesRow {
    pub user_id: Uuid,
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}
