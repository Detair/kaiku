//! Bot Management API
//!
//! Handlers for creating and managing bot applications and their slash commands.

pub mod commands;
pub mod error;
pub mod handlers;
pub mod types;

pub use error::BotError;
pub use types::{ApplicationResponse, BotTokenResponse, CreateApplicationRequest, UpdateIntentsRequest};

use axum::routing::{delete, get, post, put};
use axum::Router;

use crate::api::AppState;

/// Create the bots router, mounted at `/api/applications`.
///
/// Combines both bot application handlers and slash command handlers.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(handlers::list_applications).post(handlers::create_application))
        .route("/{id}", get(handlers::get_application).delete(handlers::delete_application))
        .route("/{id}/bot", post(handlers::create_bot))
        .route("/{id}/reset-token", post(handlers::reset_bot_token))
        .route("/{id}/intents", put(handlers::update_gateway_intents))
        .route(
            "/{id}/commands",
            get(commands::list_commands)
                .put(commands::register_commands)
                .delete(commands::delete_all_commands),
        )
        .route(
            "/{id}/commands/{command_id}",
            delete(commands::delete_command),
        )
}
