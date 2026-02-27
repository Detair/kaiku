//! Guild (Server) Management Module
//!
//! Handles guild creation, membership, invites, roles, categories, search, and management.

pub mod categories;
pub mod emojis;
pub mod handlers;
pub mod invites;
pub mod limits;
pub mod roles;
pub mod search;
pub mod types;

use axum::routing::{delete, get, patch, post};
use axum::Router;

use crate::api::AppState;
use crate::pages;

/// Create the guild router with all endpoints
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guilds).post(handlers::create_guild))
        .route(
            "/{id}",
            get(handlers::get_guild)
                .patch(handlers::update_guild)
                .delete(handlers::delete_guild),
        )
        .route("/{id}/leave", post(handlers::leave_guild))
        .route("/{id}/members", get(handlers::list_members))
        .route("/{id}/members/{user_id}", delete(handlers::kick_member))
        .route("/{id}/bots", get(handlers::list_guild_bots))
        .route("/{id}/bots/{bot_id}/add", post(handlers::add_bot_to_guild))
        .route(
            "/{id}/bots/{bot_id}",
            delete(handlers::remove_bot_from_guild),
        )
        .route("/{id}/usage", get(handlers::get_guild_usage))
        .route("/{id}/channels", get(handlers::list_channels))
        .route("/{id}/channels/reorder", post(handlers::reorder_channels))
        .route("/{id}/read-all", post(handlers::mark_all_channels_read))
        .route("/{id}/commands", get(handlers::list_guild_commands))
        // Guild settings
        .route(
            "/{id}/settings",
            get(handlers::get_guild_settings).patch(handlers::update_guild_settings),
        )
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
