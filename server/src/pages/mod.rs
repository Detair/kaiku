//! Information pages module.
//!
//! Provides platform-level and guild-level information pages for:
//! - Terms of Service, Privacy Policy (platform)
//! - Guild rules, FAQ, welcome pages (guild)
//!
//! Features:
//! - Markdown content with Mermaid diagram support
//! - Version tracking via content hashing
//! - Revision history with restore capability
//! - Guild-scoped page categories
//! - User acceptance tracking
//! - Audit logging

pub mod constants;
pub mod error;
pub mod guild_pages;
pub mod page_categories;
pub mod platform;
pub mod queries;
pub mod revisions;
mod shared;
pub mod types;

use axum::routing::{delete, get, patch, post};
use axum::Router;
pub use constants::*;
pub use queries::*;
pub use types::*;

use crate::api::AppState;

/// Router for platform pages (mounted at /api/pages).
pub fn platform_pages_router() -> Router<AppState> {
    Router::new()
        .route("/", get(platform::list_platform_pages))
        .route("/", post(platform::create_platform_page))
        .route("/pending-acceptance", get(platform::get_pending_acceptance))
        .route("/reorder", post(platform::reorder_platform_pages))
        .route("/by-slug/{slug}", get(platform::get_platform_page))
        .route("/{id}", patch(platform::update_platform_page))
        .route("/{id}", delete(platform::delete_platform_page))
        .route("/{id}/accept", post(platform::accept_page))
        .route(
            "/{page_id}/revisions",
            get(revisions::list_platform_page_revisions),
        )
        .route(
            "/{page_id}/revisions/{n}",
            get(revisions::get_platform_page_revision),
        )
        .route(
            "/{page_id}/revisions/{n}/restore",
            post(revisions::restore_platform_page_revision),
        )
}

/// Router for guild pages (mounted at `/api/guilds/{guild_id}/pages`).
pub fn guild_pages_router() -> Router<AppState> {
    Router::new()
        .route("/", get(guild_pages::list_guild_pages))
        .route("/", post(guild_pages::create_guild_page))
        .route("/reorder", post(guild_pages::reorder_guild_pages))
        .route("/by-slug/{slug}", get(guild_pages::get_guild_page))
        .route("/{id}", patch(guild_pages::update_guild_page))
        .route("/{id}", delete(guild_pages::delete_guild_page))
        .route("/{id}/accept", post(guild_pages::accept_guild_page))
        .route(
            "/{page_id}/revisions",
            get(revisions::list_guild_page_revisions),
        )
        .route(
            "/{page_id}/revisions/{n}",
            get(revisions::get_guild_page_revision),
        )
        .route(
            "/{page_id}/revisions/{n}/restore",
            post(revisions::restore_guild_page_revision),
        )
}

/// Router for guild page categories (mounted at `/api/guilds/{guild_id}/page-categories`).
pub fn guild_page_categories_router() -> Router<AppState> {
    Router::new()
        .route("/", get(page_categories::list_guild_categories))
        .route("/", post(page_categories::create_guild_category))
        .route("/reorder", post(page_categories::reorder_guild_categories))
        .route("/{cat_id}", patch(page_categories::update_guild_category))
        .route("/{cat_id}", delete(page_categories::delete_guild_category))
}
