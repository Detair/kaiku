//! Request and response types for the preferences API.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Response for preferences endpoints.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PreferencesResponse {
    #[schema(value_type = Object)]
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}

/// Request body for updating preferences.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePreferencesRequest {
    #[schema(value_type = Object)]
    pub preferences: serde_json::Value,
}

/// Database row for `user_preferences`.
#[derive(Debug, sqlx::FromRow)]
pub struct UserPreferencesRow {
    pub user_id: Uuid,
    pub preferences: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}
