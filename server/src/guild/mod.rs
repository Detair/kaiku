//! Guild (Server) Management Module
//!
//! Handles guild creation, membership, invites, roles, categories, search, and management.

pub mod categories;
pub mod handlers;
pub mod invites;
pub mod roles;
pub mod search;
pub mod types;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::api::AppState;

/// Create the guild router with all endpoints
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_guilds).post(handlers::create_guild))
        .route(
            "/:id",
            get(handlers::get_guild)
                .patch(handlers::update_guild)
                .delete(handlers::delete_guild),
        )
        .route("/:id/join", post(handlers::join_guild))
        .route("/:id/leave", post(handlers::leave_guild))
        .route("/:id/members", get(handlers::list_members))
        .route("/:id/members/:user_id", delete(handlers::kick_member))
        .route("/:id/channels", get(handlers::list_channels))
        .route("/:id/channels/reorder", post(handlers::reorder_channels))
        // Role routes
        .route(
            "/:id/roles",
            get(roles::list_roles).post(roles::create_role),
        )
        .route(
            "/:id/roles/:role_id",
            patch(roles::update_role).delete(roles::delete_role),
        )
        .route(
            "/:id/members/:user_id/roles/:role_id",
            post(roles::assign_role).delete(roles::remove_role),
        )
        // Invite routes
        .route(
            "/:id/invites",
            get(invites::list_invites).post(invites::create_invite),
        )
        .route("/:id/invites/:code", delete(invites::delete_invite))
        // Category routes
        .route(
            "/:id/categories",
            get(categories::list_categories).post(categories::create_category),
        )
        .route(
            "/:id/categories/:category_id",
            patch(categories::update_category).delete(categories::delete_category),
        )
        .route("/:id/categories/reorder", post(categories::reorder_categories))
        // Search route
        .route("/:id/search", get(search::search_messages))
}

/// Create the invite join router (separate for public access pattern)
pub fn invite_router() -> Router<AppState> {
    Router::new().route("/:code/join", post(invites::join_via_invite))
}
