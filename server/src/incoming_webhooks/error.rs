//! Incoming webhooks module error types.
//!
//! Token-authenticated routes are consumed by Discord webhook client
//! libraries, so error bodies carry Discord's numeric `code` values alongside
//! Kaiku's string `error` codes.

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use thiserror::Error;

use crate::permissions::PermissionError;

/// Incoming webhook errors.
#[derive(Error, Debug)]
pub enum IncomingWebhookError {
    #[error("Database error")]
    Database(#[from] sqlx::Error),
    /// Webhook id does not exist (Discord code 10015).
    #[error("Unknown Webhook")]
    UnknownWebhook,
    /// Webhook id exists but the token does not match (Discord code 50027).
    #[error("Invalid Webhook Token")]
    InvalidToken,
    /// Message id not found or not created by this webhook (Discord code 10008).
    #[error("Unknown Message")]
    UnknownMessage,
    /// Target channel missing (Discord code 10003).
    #[error("Unknown Channel")]
    UnknownChannel,
    /// Execute body had neither content nor embeds (Discord code 50006).
    #[error("Cannot send an empty message")]
    EmptyMessage,
    /// Invalid request body (Discord code 50035).
    #[error("{0}")]
    Validation(String),
    /// Forum channel execute without `thread_name`/`thread_id`.
    #[error("Webhook messages in a forum channel require thread_name or thread_id")]
    ThreadRequired,
    /// Reply target thread is locked (Discord code 160005).
    #[error("Thread is locked")]
    ThreadLocked,
    /// Content blocked by the guild's moderation filter.
    #[error("Message blocked by content filter")]
    ContentFiltered,
    /// Per-channel webhook cap reached (Discord code 30007).
    #[error("Maximum number of webhooks reached (15)")]
    MaxWebhooksReached,
    #[error("{0}")]
    Permission(#[from] PermissionError),
    /// Per-webhook execute rate limit exceeded.
    #[error("You are being rate limited.")]
    RateLimited { retry_after: f64 },
}

impl IncomingWebhookError {
    /// (HTTP status, Kaiku string code, Discord numeric code).
    const fn parts(&self) -> (StatusCode, &'static str, u32) {
        match self {
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", 0),
            Self::UnknownWebhook => (StatusCode::NOT_FOUND, "UNKNOWN_WEBHOOK", 10015),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "INVALID_WEBHOOK_TOKEN", 50027),
            Self::UnknownMessage => (StatusCode::NOT_FOUND, "UNKNOWN_MESSAGE", 10008),
            Self::UnknownChannel => (StatusCode::NOT_FOUND, "UNKNOWN_CHANNEL", 10003),
            Self::EmptyMessage => (StatusCode::BAD_REQUEST, "EMPTY_MESSAGE", 50006),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR", 50035),
            Self::ThreadRequired => (StatusCode::BAD_REQUEST, "THREAD_REQUIRED", 50035),
            Self::ThreadLocked => (StatusCode::FORBIDDEN, "THREAD_LOCKED", 160_005),
            Self::ContentFiltered => (StatusCode::BAD_REQUEST, "CONTENT_FILTERED", 50035),
            Self::MaxWebhooksReached => (StatusCode::BAD_REQUEST, "MAX_WEBHOOKS", 30007),
            Self::Permission(_) => (StatusCode::FORBIDDEN, "PERMISSION_DENIED", 50013),
            Self::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", 0),
        }
    }
}

impl IntoResponse for IncomingWebhookError {
    fn into_response(self) -> Response {
        if let Self::Database(e) = &self {
            tracing::error!(error = %e, "Incoming webhook database operation failed");
        }
        // Discord-shaped 429: float retry_after seconds + Retry-After header so
        // discord.js/discord.py/serenity back off correctly.
        if let Self::RateLimited { retry_after } = &self {
            let body = Json(serde_json::json!({
                "message": "You are being rate limited.",
                "retry_after": retry_after,
                "global": false,
            }));
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, retry_after.ceil().to_string())],
                body,
            )
                .into_response();
        }
        let (status, error_code, discord_code) = self.parts();
        let message = if matches!(self, Self::Database(_)) {
            "Internal server error".to_string()
        } else {
            self.to_string()
        };
        (
            status,
            Json(serde_json::json!({
                "error": error_code,
                "message": message,
                "code": discord_code,
            })),
        )
            .into_response()
    }
}
