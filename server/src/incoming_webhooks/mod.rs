//! Incoming (Discord-compatible) webhooks.
//!
//! External services POST to `/api/webhooks/{id}/{token}` to create messages
//! in a channel — the same URL contract as Discord, so game servers, CI
//! systems, Grafana, etc. work by pasting a Kaiku webhook URL wherever a
//! Discord one is expected. Includes the `/slack` compat route.
//!
//! Not to be confused with the *outgoing* webhooks module
//! ([`crate::webhooks`]), which delivers platform events to bot endpoints.

pub mod error;
pub mod execute;
pub mod handlers;
pub mod queries;
pub mod slack;
pub mod types;

use axum::routing::{get, post};
use axum::Router;
pub use error::IncomingWebhookError;

use crate::api::AppState;

/// Session-authenticated management routes (nested under the auth'd tier;
/// all require `MANAGE_WEBHOOKS`).
pub fn management_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/channels/{channel_id}/webhooks",
            get(handlers::list_channel_webhooks).post(handlers::create_channel_webhook),
        )
        .route(
            "/api/guilds/{guild_id}/webhooks",
            get(handlers::list_guild_webhooks),
        )
        .route(
            "/api/webhooks/{webhook_id}",
            get(handlers::get_webhook)
                .patch(handlers::modify_webhook)
                .delete(handlers::delete_webhook),
        )
}

/// Public token-authenticated routes (the URL token is the credential).
/// Layered with IP rate limiting in `api::create_router`.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/webhooks/{webhook_id}/{token}",
            get(handlers::get_webhook_with_token)
                .patch(handlers::modify_webhook_with_token)
                .delete(handlers::delete_webhook_with_token)
                .post(execute::execute_webhook),
        )
        .route(
            "/api/webhooks/{webhook_id}/{token}/slack",
            post(slack::execute_slack_webhook),
        )
        .route(
            "/api/webhooks/{webhook_id}/{token}/messages/{message_id}",
            get(execute::get_webhook_message)
                .patch(execute::edit_webhook_message)
                .delete(execute::delete_webhook_message),
        )
}
