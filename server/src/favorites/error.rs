//! Error types for favorites operations.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

/// Errors that can occur in the favorites API.
#[derive(Debug, thiserror::Error)]
pub enum FavoritesError {
    #[error("Channel not found")]
    ChannelNotFound,
    #[error("Channel cannot be favorited (DM channels not allowed)")]
    InvalidChannel,
    #[error("Maximum favorites limit reached (25)")]
    LimitExceeded,
    #[error("Channel already favorited")]
    AlreadyFavorited,
    #[error("Channel is not favorited")]
    NotFavorited,
    #[error("Invalid channel IDs in reorder request")]
    InvalidChannels,
    #[error("Invalid guild IDs in reorder request")]
    InvalidGuilds,
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for FavoritesError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self {
            Self::ChannelNotFound => (
                StatusCode::NOT_FOUND,
                "channel_not_found",
                "Channel not found",
            ),
            Self::InvalidChannel => (
                StatusCode::BAD_REQUEST,
                "invalid_channel",
                "DM channels cannot be favorited",
            ),
            Self::LimitExceeded => (
                StatusCode::BAD_REQUEST,
                "limit_exceeded",
                "Maximum 25 favorites allowed",
            ),
            Self::AlreadyFavorited => (
                StatusCode::CONFLICT,
                "already_favorited",
                "Channel already in favorites",
            ),
            Self::NotFavorited => (
                StatusCode::NOT_FOUND,
                "favorite_not_found",
                "Channel is not favorited",
            ),
            Self::InvalidChannels => (
                StatusCode::BAD_REQUEST,
                "invalid_channels",
                "Reorder contains invalid channel IDs",
            ),
            Self::InvalidGuilds => (
                StatusCode::BAD_REQUEST,
                "invalid_guilds",
                "Reorder contains invalid guild IDs",
            ),
            Self::Database(err) => {
                tracing::error!("Database error in favorites: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "Database error",
                )
            }
        };
        (
            status,
            Json(serde_json::json!({ "error": code, "message": message })),
        )
            .into_response()
    }
}
