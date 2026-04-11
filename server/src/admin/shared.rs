//! Shared DTOs and helpers used across admin handler sub-modules.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Pagination query parameters with optional search.
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PaginationParams {
    /// Maximum number of items to return.
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Number of items to skip.
    #[serde(default)]
    pub offset: i64,
    /// Search query (searches username, `display_name`, email for users; name for guilds).
    pub search: Option<String>,
}

#[allow(clippy::missing_const_for_fn)]
pub(super) fn default_limit() -> i64 {
    50
}

/// Generic paginated response wrapper.
#[derive(Debug, Serialize, ToSchema)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Delete response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeleteResponse {
    pub deleted: bool,
    pub id: Uuid,
}

/// Escape a string for CSV (handles commas and quotes).
pub(super) fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ============================================================================
// Redis cache helpers
// ============================================================================

use fred::prelude::*;
use sqlx::PgPool;
use uuid::Uuid as AdminUuid;

/// Check if a user is an elevated admin (for WebSocket subscription check).
///
/// Checks Redis cache first; falls back to the database on a cache miss.
/// Security: always falls back to database on cache miss to ensure fail-secure behavior.
pub async fn is_elevated_admin(redis: &Client, db: &PgPool, user_id: AdminUuid) -> bool {
    // Check cache first (fast path)
    let cache_key = format!("admin:elevated:{user_id}");
    let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

    if let Some(value) = cached {
        return value == "1";
    }

    // Cache miss - fallback to database (fail-secure)
    let is_elevated = check_elevated_in_db(db, user_id).await;

    // Cache the result (60s TTL to balance freshness and load)
    if is_elevated {
        cache_elevated_status(redis, user_id, true, 60).await;
    }

    is_elevated
}

/// Check elevated session status directly in the database.
async fn check_elevated_in_db(db: &PgPool, user_id: AdminUuid) -> bool {
    super::queries::has_active_elevated_session(db, user_id)
        .await
        .unwrap_or(false)
}

/// Cache elevated admin status in Redis (called after elevation).
pub async fn cache_elevated_status(
    redis: &Client,
    user_id: AdminUuid,
    is_elevated: bool,
    ttl_secs: i64,
) {
    let cache_key = format!("admin:elevated:{user_id}");
    let value = if is_elevated { "1" } else { "0" };

    let _: Result<(), _> = redis
        .set(
            &cache_key,
            value,
            Some(Expiration::EX(ttl_secs)),
            None,
            false,
        )
        .await;
}
