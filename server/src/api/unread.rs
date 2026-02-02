//! Unread Aggregation API
//!
//! Provides endpoints for querying unread message counts across guilds and DMs.

use axum::{extract::State, http::StatusCode, Json};

use crate::{auth::AuthUser, db};

use super::AppState;

/// Get aggregate unread counts for the authenticated user.
///
/// Returns unread counts grouped by guild, plus DM unreads.
/// This is the primary endpoint for the Home unread dashboard.
///
/// # Route
/// `GET /api/me/unread`
///
/// # Authentication
/// Requires valid JWT token.
///
/// # Returns
/// - 200 OK: `UnreadAggregate` with guild and DM unread counts
/// - 500 Internal Server Error: Database error
#[tracing::instrument(skip(state))]
pub async fn get_unread_aggregate(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<db::UnreadAggregate>, (StatusCode, String)> {
    let aggregate = db::get_unread_aggregate(&state.db, auth_user.id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %auth_user.id, "Failed to fetch unread aggregate");
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch unread counts".to_string())
        })?;

    Ok(Json(aggregate))
}
