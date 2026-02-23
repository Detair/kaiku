//! Types for information pages feature.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Full page data including content.
#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Page {
    pub id: Uuid,
    pub guild_id: Option<Uuid>,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub content_hash: String,
    pub position: i32,
    pub requires_acceptance: bool,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Page metadata for listing (without content for efficiency).
#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct PageListItem {
    pub id: Uuid,
    pub guild_id: Option<Uuid>,
    pub title: String,
    pub slug: String,
    pub position: i32,
    pub requires_acceptance: bool,
    pub updated_at: DateTime<Utc>,
}

/// Request body for creating a new page.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreatePageRequest {
    /// Page title (required).
    pub title: String,
    /// URL-friendly slug (auto-generated from title if not provided).
    pub slug: Option<String>,
    /// Markdown content (required).
    pub content: String,
    /// Whether users must accept this page (default: false).
    pub requires_acceptance: Option<bool>,
}

/// Request body for updating an existing page.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePageRequest {
    /// New title (optional).
    pub title: Option<String>,
    /// New slug (optional).
    pub slug: Option<String>,
    /// New content (optional).
    pub content: Option<String>,
    /// New acceptance requirement (optional).
    pub requires_acceptance: Option<bool>,
}

/// Request body for reordering pages.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReorderRequest {
    /// Ordered list of page IDs representing the new order.
    pub page_ids: Vec<Uuid>,
}

/// User's acceptance record for a page.
#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct PageAcceptance {
    pub user_id: Uuid,
    pub page_id: Uuid,
    /// Content hash at time of acceptance (for tracking version changes).
    pub content_hash: String,
    pub accepted_at: DateTime<Utc>,
}

/// Page audit log entry.
#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct PageAuditEntry {
    pub id: Uuid,
    pub page_id: Uuid,
    pub action: String,
    pub actor_id: Uuid,
    pub previous_content_hash: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}
