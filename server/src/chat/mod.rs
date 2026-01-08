//! Chat Service
//!
//! Handles channels, messages, and file uploads.

mod channels;
mod messages;
pub mod s3;
mod uploads;

use axum::{
    routing::{delete, get, patch, post},
    Router,
};

use crate::api::AppState;

pub use s3::S3Client;

/// Create channels router.
pub fn channels_router() -> Router<AppState> {
    Router::new()
        .route("/", get(channels::list))
        .route("/", post(channels::create))
        .route("/:id", get(channels::get))
        .route("/:id", patch(channels::update))
        .route("/:id", delete(channels::delete))
        .route("/:id/members", get(channels::list_members))
        .route("/:id/members", post(channels::add_member))
        .route("/:id/members/:user_id", delete(channels::remove_member))
}

/// Create messages router.
pub fn messages_router() -> Router<AppState> {
    Router::new()
        .route(
            "/channel/:channel_id",
            get(messages::list).post(messages::create),
        )
        .route("/:id", patch(messages::update).delete(messages::delete))
        .route("/upload", post(uploads::upload_file))
        .route("/attachments/:id", get(uploads::get_attachment))
        .route("/attachments/:id/download", get(uploads::download))
}
