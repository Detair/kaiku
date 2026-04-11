//! Request and response types for bot API endpoints.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Database row for bot application queries (used with `sqlx::query_as`).
#[derive(sqlx::FromRow)]
pub(super) struct ApplicationRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub bot_user_id: Option<Uuid>,
    pub public: bool,
    pub gateway_intents: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl From<ApplicationRow> for ApplicationResponse {
    fn from(r: ApplicationRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            description: r.description,
            bot_user_id: r.bot_user_id,
            public: r.public,
            gateway_intents: r.gateway_intents,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

/// Request body for creating a bot application.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateApplicationRequest {
    /// Application name (2-100 characters).
    pub name: String,
    /// Optional description (max 1000 characters).
    pub description: Option<String>,
}

/// Response for bot application.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApplicationResponse {
    /// Application ID.
    pub id: Uuid,
    /// Application name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Associated bot user ID (if bot has been created).
    pub bot_user_id: Option<Uuid>,
    /// Whether the bot is publicly listed.
    pub public: bool,
    /// Gateway intents for event filtering.
    pub gateway_intents: Vec<String>,
    /// When the application was created.
    pub created_at: String,
}

/// Response for bot token (only returned once on creation/reset).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BotTokenResponse {
    /// The bot token (only shown once).
    pub token: String,
    /// Associated bot user ID.
    pub bot_user_id: Uuid,
}

/// Request to update gateway intents.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateIntentsRequest {
    /// List of intent names (e.g., `["messages", "members", "commands"]`).
    pub intents: Vec<String>,
}
