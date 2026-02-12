//! Router configuration for information pages.

use axum::routing::{delete, get, patch, post};
use axum::Router;

use super::handlers;
use crate::api::AppState;

/// Router for platform pages (mounted at /api/pages).
///
/// Routes:
/// - GET  /                    - List platform pages
/// - POST /                    - Create platform page (admin)
/// - GET  /pending-acceptance  - Get pages needing user acceptance
/// - GET  /by-slug/{slug}       - Get platform page by slug
/// - PATCH /{id}                - Update platform page (admin)
/// - DELETE /{id}               - Delete platform page (admin)
/// - POST /reorder             - Reorder platform pages (admin)
/// - POST /{id}/accept          - Accept a page
pub fn platform_pages_router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_platform_pages))
        .route("/", post(handlers::create_platform_page))
        .route("/pending-acceptance", get(handlers::get_pending_acceptance))
        .route("/reorder", post(handlers::reorder_platform_pages))
        .route("/by-slug/{slug}", get(handlers::get_platform_page))
        .route("/{id}", patch(handlers::update_platform_page))
        .route("/{id}", delete(handlers::delete_platform_page))
        .route("/{id}/accept", post(handlers::accept_page))
}

/// Router for guild pages (mounted at `/api/guilds/{guild_id}/pages`).
///
/// Routes:
/// - GET  /               - List guild pages
/// - POST /               - Create guild page (`MANAGE_PAGES`)
/// - GET  /by-slug/{slug}  - Get guild page by slug
/// - PATCH /{id}           - Update guild page (`MANAGE_PAGES`)
/// - DELETE /{id}          - Delete guild page (`MANAGE_PAGES`)
/// - POST /reorder        - Reorder guild pages (`MANAGE_PAGES`)
pub fn guild_pages_router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guild_pages))
        .route("/", post(handlers::create_guild_page))
        .route("/reorder", post(handlers::reorder_guild_pages))
        .route("/by-slug/{slug}", get(handlers::get_guild_page))
        .route("/{id}", patch(handlers::update_guild_page))
        .route("/{id}", delete(handlers::delete_guild_page))
        .route("/{id}/accept", post(handlers::accept_guild_page))
}
