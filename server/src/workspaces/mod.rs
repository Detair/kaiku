//! Personal Workspaces
//!
//! User-owned cross-guild channel collections for "Mission Control" views.

pub mod error;
pub mod handlers;
mod queries;
pub mod types;

use axum::routing::{delete, get, patch, post};
use axum::Router;

use crate::api::AppState;

/// Create workspace routes.
///
/// Mounted at `/api/me/workspaces` in the main router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(handlers::list_workspaces).post(handlers::create_workspace),
        )
        .route("/reorder", post(handlers::reorder_workspaces))
        .route(
            "/{id}",
            get(handlers::get_workspace)
                .patch(handlers::update_workspace)
                .delete(handlers::delete_workspace),
        )
        .route("/{id}/entries", post(handlers::add_entry))
        .route("/{id}/entries/{entry_id}", delete(handlers::remove_entry))
        .route("/{id}/reorder", patch(handlers::reorder_entries))
}
