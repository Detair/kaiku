//! Chat Service
//!
//! Handles channels, messages, and file uploads.

pub(crate) mod channel_pins;
pub(crate) mod channels;
pub mod components;
pub mod dm;
pub mod dm_search;
pub mod embeds;
pub mod error;
pub mod files;
pub mod forum;
pub(crate) mod media_processing;
pub(crate) mod messages;
pub mod overrides;
pub mod pins;
pub mod queries;
pub mod reactions;
pub mod s3;
pub(crate) mod screenshare;
pub mod types;
pub mod unread;
pub(crate) mod uploads;

use axum::routing::{delete, get, patch, post, put};
use axum::Router;
pub use error::ChatError;
pub use s3::S3Client;

use crate::api::AppState;

/// Create channels router.
pub fn channels_router() -> Router<AppState> {
    Router::new()
        .route("/", post(channels::create))
        .route("/{id}", get(channels::get))
        .route("/{id}", patch(channels::update))
        .route("/{id}", delete(channels::delete))
        .route("/{id}/members", get(channels::list_members))
        .route("/{id}/members", post(channels::add_member))
        .route("/{id}/members/{user_id}", delete(channels::remove_member))
        // Permission overrides
        .route("/{id}/overrides", get(overrides::list_overrides))
        .route(
            "/{id}/overrides/{role_id}",
            put(overrides::set_override).delete(overrides::delete_override),
        )
        // Read state
        .route("/{id}/read", post(channels::mark_as_read))
        // Screen Share
        .route("/{id}/screenshare/check", post(screenshare::check))
        .route("/{id}/screenshare/start", post(screenshare::start))
        .route("/{id}/screenshare/stop", post(screenshare::stop))
        // Forum posts + tags (channel-scoped)
        .route(
            "/{id}/posts",
            get(forum::list_posts).post(forum::create_post),
        )
        .route("/{id}/tags", get(forum::list_tags).post(forum::create_tag))
        .route("/{id}/tags/{tag_id}", delete(forum::delete_tag))
}

/// Forum post moderation router (mounted at `/api/forum`).
pub fn forum_router() -> Router<AppState> {
    Router::new().route(
        "/posts/{post_id}",
        patch(forum::update_post).delete(forum::delete_post),
    )
}

/// Create messages router (protected routes).
pub fn messages_router() -> Router<AppState> {
    Router::new()
        .route(
            "/channel/{channel_id}",
            get(messages::list).post(messages::create),
        )
        .route(
            "/channel/{channel_id}/upload",
            post(uploads::upload_message_with_file),
        )
        .route("/{id}", patch(messages::update).delete(messages::delete))
        .route("/{parent_id}/thread", get(messages::list_thread_replies))
        .route("/{parent_id}/thread/read", post(messages::mark_thread_read))
        .route("/upload", post(uploads::upload_file))
        .route("/attachments/{id}", get(uploads::get_attachment))
        .route("/attachments/{id}/url", get(uploads::get_signed_url))
}

/// Create public messages router (routes that handle their own auth).
/// The download route accepts auth via query parameter for browser requests.
pub fn messages_public_router() -> Router<AppState> {
    Router::new().route("/attachments/{id}/download", get(uploads::download))
}

/// Create DM (Direct Message) router.
pub fn dm_router() -> Router<AppState> {
    Router::new()
        .route("/", get(dm::list_dms).post(dm::create_dm))
        .route("/read-all", post(dm::mark_all_dms_read))
        .route("/{id}", get(dm::get_dm))
        .route("/{id}/leave", post(dm::leave_dm))
        .route("/{id}/name", patch(dm::update_dm_name))
        .route("/{id}/read", post(dm::mark_as_read))
        .route("/{id}/icon", get(dm::get_dm_icon).post(dm::upload_dm_icon))
}
