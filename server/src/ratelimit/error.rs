//! Rate limiting error types for HTTP responses.

use axum::http::header::HeaderValue;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::ratelimit::RateLimitResult;

/// Errors that can occur during rate limit checks.
#[derive(Debug)]
pub enum RateLimitError {
    /// Redis is unavailable (fail-open, but should be logged).
    RedisUnavailable,
    /// Request exceeded the rate limit.
    LimitExceeded(RateLimitResult),
    /// IP is temporarily blocked due to repeated failures.
    IpBlocked { retry_after: u64 },
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RedisUnavailable => write!(f, "Redis service unavailable"),
            Self::LimitExceeded(res) => {
                write!(f, "Rate limit exceeded. Retry after {}s", res.retry_after)
            }
            Self::IpBlocked { retry_after } => write!(f, "IP blocked. Retry after {retry_after}s"),
        }
    }
}

/// JSON response body for rate limit errors.
#[derive(Serialize)]
pub struct RateLimitErrorResponse {
    /// Error code identifier.
    pub error: &'static str,
    /// Human-readable error message.
    pub message: String,
    /// Seconds to wait before retrying.
    pub retry_after: u64,
    /// Maximum requests allowed in the window.
    pub limit: u32,
    /// Remaining requests (always 0 when rate limited).
    pub remaining: u32,
}

impl IntoResponse for RateLimitError {
    fn into_response(self) -> Response {
        match self {
            Self::RedisUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "service_unavailable"})),
            )
                .into_response(),
            Self::LimitExceeded(result) => {
                let body = RateLimitErrorResponse {
                    error: "rate_limited",
                    message: format!("Too many requests. Wait {} seconds.", result.retry_after),
                    retry_after: result.retry_after,
                    limit: result.limit,
                    remaining: 0,
                };
                let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
                let headers = response.headers_mut();
                if let Ok(v) = HeaderValue::from_str(&result.retry_after.to_string()) {
                    headers.insert("Retry-After", v);
                }
                response
            }
            Self::IpBlocked { retry_after } => {
                let body = RateLimitErrorResponse {
                    error: "ip_blocked",
                    message: format!("IP blocked. Wait {retry_after} seconds."),
                    retry_after,
                    limit: 0,
                    remaining: 0,
                };
                let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
                let headers = response.headers_mut();
                if let Ok(v) = HeaderValue::from_str(&retry_after.to_string()) {
                    headers.insert("Retry-After", v);
                }
                response
            }
        }
    }
}
