//! Guild (Server) Management Module
//!
//! Handles guild creation, membership, invites, roles, categories, search, and management.

pub mod bots;
pub mod categories;
pub mod core;
pub mod emojis;
pub mod error;
pub mod invites;
pub mod members;
pub mod queries;
pub mod roles;
pub mod search;
pub mod settings;
pub mod types;

// Re-export `limits` from `queries::limits` so existing call sites
// (`guild::limits::*`) keep working without churn.
use axum::routing::{delete, get, patch, post};
use axum::Router;
pub use error::GuildError;
pub use queries::limits;

use crate::api::AppState;
use crate::pages;

/// Create the guild router with all endpoints
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(core::list_guilds).post(core::create_guild))
        .route(
            "/{id}",
            get(core::get_guild)
                .patch(core::update_guild)
                .delete(core::delete_guild),
        )
        .route("/{id}/leave", post(members::leave_guild))
        .route("/{id}/members", get(members::list_members))
        .route("/{id}/members/{user_id}", delete(members::kick_member))
        .route("/{id}/bots", get(bots::list_guild_bots))
        .route("/{id}/bots/{bot_id}/add", post(bots::add_bot_to_guild))
        .route("/{id}/bots/{bot_id}", delete(bots::remove_bot_from_guild))
        .route("/{id}/usage", get(settings::get_guild_usage))
        .route("/{id}/channels", get(settings::list_channels))
        .route("/{id}/channels/reorder", post(settings::reorder_channels))
        .route("/{id}/read-all", post(settings::mark_all_channels_read))
        .route("/{id}/commands", get(bots::list_guild_commands))
        // Guild settings
        .route(
            "/{id}/settings",
            get(settings::get_guild_settings).patch(settings::update_guild_settings),
        )
        .route(
            "/{id}/dismiss-discovery-prompt",
            post(settings::dismiss_discovery_prompt),
        )
        // Banner upload
        .route("/{id}/banner", post(settings::upload_guild_banner))
        // Role routes
        .route(
            "/{id}/roles",
            get(roles::list_roles).post(roles::create_role),
        )
        .route(
            "/{id}/roles/{role_id}",
            patch(roles::update_role).delete(roles::delete_role),
        )
        .route(
            "/{id}/members/{user_id}/roles/{role_id}",
            post(roles::assign_role).delete(roles::remove_role),
        )
        // Invite routes
        .route(
            "/{id}/invites",
            get(invites::list_invites).post(invites::create_invite),
        )
        .route("/{id}/invites/{code}", delete(invites::delete_invite))
        // Category routes
        .route(
            "/{id}/categories",
            get(categories::list_categories).post(categories::create_category),
        )
        .route(
            "/{id}/categories/{category_id}",
            patch(categories::update_category).delete(categories::delete_category),
        )
        .route(
            "/{id}/categories/reorder",
            post(categories::reorder_categories),
        )
        // Pages routes (nested)
        .nest("/{id}/pages", pages::guild_pages_router())
        // Page categories routes (nested)
        .nest(
            "/{id}/page-categories",
            pages::guild_page_categories_router(),
        )
        // Emoji routes
        .nest("/{id}/emojis", emojis::router())
}

/// Create the invite join router (separate for public access pattern)
pub fn invite_router() -> Router<AppState> {
    Router::new().route("/{code}/join", post(invites::join_via_invite))
}
