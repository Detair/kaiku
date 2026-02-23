//! Connection History API
//!
//! Provides endpoints for users to view their voice connection quality history.

pub(crate) mod handlers;

use axum::routing::get;
use axum::Router;

use crate::api::AppState;

/// Create the connectivity router with history endpoints.
///
/// Routes:
/// - GET /summary - 30-day aggregate stats and daily breakdown
/// - GET /sessions - Paginated list of session summaries
/// - GET `/sessions/{session_id`} - Session detail with metrics
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/summary", get(handlers::get_summary))
        .route("/sessions", get(handlers::get_sessions))
        .route("/sessions/{session_id}", get(handlers::get_session_detail))
}
