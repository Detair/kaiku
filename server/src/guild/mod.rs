//! Guild (Server) Management Module
//!
//! Handles guild creation, membership, and management.

pub mod handlers;
pub mod types;

use axum::{
    routing::{get, post},
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
        .route("/:id/channels", get(handlers::list_channels))
}
