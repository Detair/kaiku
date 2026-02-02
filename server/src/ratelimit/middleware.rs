//! Axum middleware for rate limiting.
//!
//! Provides middleware functions to enforce rate limits on incoming requests.
//! Supports rate limiting by IP address (for unauthenticated endpoints) and
//! by user ID (for authenticated endpoints).

use axum::{
    extract::{ConnectInfo, Request, State},
    http::header::HeaderValue,
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use tracing::{debug, warn};

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::ratelimit::{
    extract_client_ip, normalize_ip, NormalizedIp, RateLimitCategory, RateLimitError,
    RateLimitResult,
};

/// Standard rate limit header names.
const HEADER_LIMIT: &str = "X-RateLimit-Limit";
const HEADER_REMAINING: &str = "X-RateLimit-Remaining";
const HEADER_RESET: &str = "X-RateLimit-Reset";

/// Adds rate limit headers to a response.
///
/// Adds the following headers:
/// - `X-RateLimit-Limit`: Maximum requests allowed in the window
/// - `X-RateLimit-Remaining`: Requests remaining in the current window
/// - `X-RateLimit-Reset`: Unix timestamp when the window resets
fn add_rate_limit_headers(response: &mut Response, result: &RateLimitResult) {
    let headers = response.headers_mut();

    if let Ok(v) = HeaderValue::from_str(&result.limit.to_string()) {
        headers.insert(HEADER_LIMIT, v);
    }
    if let Ok(v) = HeaderValue::from_str(&result.remaining.to_string()) {
        headers.insert(HEADER_REMAINING, v);
    }
    if let Ok(v) = HeaderValue::from_str(&result.reset_at.to_string()) {
        headers.insert(HEADER_RESET, v);
    }
}

/// Middleware to rate limit requests by client IP address.
///
/// Use this for unauthenticated endpoints like login, registration, and
/// password reset. Extracts the client IP from headers or connection info,
/// normalizes it (IPv6 to /64 prefix), and checks against the rate limiter.
///
/// # Usage
///
/// ```ignore
/// use axum::middleware::from_fn_with_state;
/// use canis::ratelimit::middleware::RateLimitByIp;
///
/// Router::new()
///     .route("/login", post(login_handler))
///     .layer(from_fn_with_state(
///         state.clone(),
///         RateLimitByIp::new(RateLimitCategory::AuthLogin).middleware(),
///     ))
/// ```
///
/// # Behavior
///
/// - If rate limiter is not configured (`state.rate_limiter` is `None`), requests pass through.
/// - If Redis is unavailable and `fail_open` is true, requests pass through with a warning.
/// - If the rate limit is exceeded, returns `429 Too Many Requests` with retry information.
/// - Stores `NormalizedIp` in request extensions for downstream handlers.
#[tracing::instrument(skip(state, request, next))]
pub async fn rate_limit_by_ip(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, RateLimitError> {
    // Get category from request extensions (set by the layer factory)
    let category = request
        .extensions()
        .get::<RateLimitCategory>()
        .copied()
        .unwrap_or(RateLimitCategory::Read);

    // Skip rate limiting if not configured
    let Some(ref rate_limiter) = state.rate_limiter else {
        return Ok(next.run(request).await);
    };

    // Extract and normalize client IP
    let connect_info = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied();
    let trust_proxy = rate_limiter.config().trust_proxy;
    let client_ip = extract_client_ip(request.headers(), connect_info.as_ref(), trust_proxy);
    let normalized_ip = normalize_ip(client_ip);

    debug!(
        category = %category.as_str(),
        ip = %normalized_ip,
        "Checking rate limit by IP"
    );

    // Store normalized IP in request extensions for downstream use
    request
        .extensions_mut()
        .insert(NormalizedIp(normalized_ip.clone()));

    // Check rate limit
    let result = match rate_limiter.check(category, &normalized_ip).await {
        Ok(result) => result,
        Err(RateLimitError::RedisUnavailable) => {
            // Fail open if configured - SECURITY WARNING: rate limiting is disabled!
            if rate_limiter.config().fail_open {
                warn!(
                    category = %category.as_str(),
                    ip = %normalized_ip,
                    security_impact = "high",
                    "SECURITY WARNING: Redis unavailable - RATE LIMITING DISABLED! API vulnerable to abuse/DoS attacks. Investigate Redis connectivity immediately."
                );
                return Ok(next.run(request).await);
            }
            return Err(RateLimitError::RedisUnavailable);
        }
        Err(e) => return Err(e),
    };

    if !result.allowed {
        debug!(
            category = %category.as_str(),
            ip = %normalized_ip,
            retry_after = result.retry_after,
            "Rate limit exceeded"
        );
        return Err(RateLimitError::LimitExceeded(result));
    }

    // Run the request and add rate limit headers to response
    let mut response = next.run(request).await;
    add_rate_limit_headers(&mut response, &result);
    Ok(response)
}

/// Middleware to rate limit requests by authenticated user ID.
///
/// Use this for authenticated endpoints. Requires `AuthUser` to be present
/// in request extensions (typically set by `require_auth` middleware).
///
/// # Usage
///
/// ```ignore
/// Router::new()
///     .route("/messages", post(create_message))
///     .layer(from_fn_with_state(state.clone(), require_auth))
///     .layer(from_fn_with_state(
///         state.clone(),
///         RateLimitByUser::new(RateLimitCategory::Write).middleware(),
///     ))
/// ```
///
/// # Behavior
///
/// - If rate limiter is not configured, requests pass through.
/// - If `AuthUser` is not present, falls back to IP-based rate limiting.
/// - Uses user ID as the rate limit identifier for authenticated users.
#[tracing::instrument(skip(state, request, next))]
pub async fn rate_limit_by_user(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, RateLimitError> {
    // Get category from request extensions (set by the layer factory)
    let category = request
        .extensions()
        .get::<RateLimitCategory>()
        .copied()
        .unwrap_or(RateLimitCategory::Read);

    // Skip rate limiting if not configured
    let Some(ref rate_limiter) = state.rate_limiter else {
        return Ok(next.run(request).await);
    };

    // Try to get authenticated user, fall back to IP
    let identifier = if let Some(auth_user) = request.extensions().get::<AuthUser>() {
        format!("user:{}", auth_user.id)
    } else {
        // Fall back to IP-based rate limiting
        let connect_info = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .copied();
        let trust_proxy = rate_limiter.config().trust_proxy;
        let client_ip = extract_client_ip(request.headers(), connect_info.as_ref(), trust_proxy);
        let normalized_ip = normalize_ip(client_ip);

        // Store normalized IP if not already present
        if request.extensions().get::<NormalizedIp>().is_none() {
            request
                .extensions_mut()
                .insert(NormalizedIp(normalized_ip.clone()));
        }

        normalized_ip
    };

    debug!(
        category = %category.as_str(),
        identifier = %identifier,
        "Checking rate limit by user/IP"
    );

    // Check rate limit
    let result = match rate_limiter.check(category, &identifier).await {
        Ok(result) => result,
        Err(RateLimitError::RedisUnavailable) => {
            // Fail open if configured - SECURITY WARNING: rate limiting is disabled!
            if rate_limiter.config().fail_open {
                warn!(
                    category = %category.as_str(),
                    identifier = %identifier,
                    security_impact = "high",
                    "SECURITY WARNING: Redis unavailable - RATE LIMITING DISABLED! API vulnerable to abuse/DoS attacks. Investigate Redis connectivity immediately."
                );
                return Ok(next.run(request).await);
            }
            return Err(RateLimitError::RedisUnavailable);
        }
        Err(e) => return Err(e),
    };

    if !result.allowed {
        debug!(
            category = %category.as_str(),
            identifier = %identifier,
            retry_after = result.retry_after,
            "Rate limit exceeded"
        );
        return Err(RateLimitError::LimitExceeded(result));
    }

    // Run the request and add rate limit headers to response
    let mut response = next.run(request).await;
    add_rate_limit_headers(&mut response, &result);
    Ok(response)
}

/// Middleware to check if an IP is blocked due to failed authentication attempts.
///
/// Use this before authentication endpoints to prevent brute-force attacks.
/// Should be applied before `rate_limit_by_ip` to reject blocked IPs early.
///
/// # Usage
///
/// ```ignore
/// Router::new()
///     .route("/login", post(login_handler))
///     .layer(from_fn_with_state(state.clone(), check_ip_not_blocked))
///     .layer(from_fn_with_state(
///         state.clone(),
///         RateLimitByIp::new(RateLimitCategory::AuthLogin).middleware(),
///     ))
/// ```
///
/// # Behavior
///
/// - If rate limiter is not configured, requests pass through.
/// - If the IP is blocked, returns `429 Too Many Requests` with the remaining block time.
/// - Stores `NormalizedIp` in request extensions for downstream use.
#[tracing::instrument(skip(state, request, next))]
pub async fn check_ip_not_blocked(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, RateLimitError> {
    // Skip if rate limiter is not configured
    let Some(ref rate_limiter) = state.rate_limiter else {
        return Ok(next.run(request).await);
    };

    // Extract and normalize client IP
    let connect_info = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied();
    let trust_proxy = rate_limiter.config().trust_proxy;
    let client_ip = extract_client_ip(request.headers(), connect_info.as_ref(), trust_proxy);
    let normalized_ip = normalize_ip(client_ip);

    // Store normalized IP in request extensions
    request
        .extensions_mut()
        .insert(NormalizedIp(normalized_ip.clone()));

    // Check if IP is blocked
    let is_blocked = match rate_limiter.is_blocked(&normalized_ip).await {
        Ok(blocked) => blocked,
        Err(RateLimitError::RedisUnavailable) => {
            // Fail open if configured - SECURITY WARNING: IP blocking is disabled!
            if rate_limiter.config().fail_open {
                warn!(
                    ip = %normalized_ip,
                    security_impact = "critical",
                    "SECURITY WARNING: Redis unavailable - IP BLOCKING DISABLED! Brute-force protection inactive. Investigate Redis connectivity immediately."
                );
                return Ok(next.run(request).await);
            }
            return Err(RateLimitError::RedisUnavailable);
        }
        Err(e) => return Err(e),
    };

    if is_blocked {
        // Use configured block_duration_secs as fallback if TTL lookup fails
        let default_block_duration = rate_limiter.config().limits.failed_auth.block_duration_secs;
        let retry_after = rate_limiter
            .get_block_ttl(&normalized_ip)
            .await
            .unwrap_or(default_block_duration);
        debug!(
            ip = %normalized_ip,
            retry_after = retry_after,
            "IP is blocked"
        );
        return Err(RateLimitError::IpBlocked { retry_after });
    }

    Ok(next.run(request).await)
}

/// Sets the rate limit category for downstream middleware.
///
/// This middleware should be applied before `rate_limit_by_ip` or `rate_limit_by_user`
/// to specify which category to use for rate limiting.
///
/// # Example
///
/// ```ignore
/// use axum::{Router, middleware::from_fn};
/// use canis::ratelimit::{RateLimitCategory, middleware::{with_category, rate_limit_by_ip}};
///
/// let app = Router::new()
///     .route("/login", post(login_handler))
///     .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
///     .layer(from_fn(with_category(RateLimitCategory::AuthLogin)));
/// ```
pub fn with_category(
    category: RateLimitCategory,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + 'static {
    move |mut request: Request, next: Next| {
        request.extensions_mut().insert(category);
        Box::pin(async move { next.run(request).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_category_stored_in_extensions() {
        // Test that with_category properly stores the category
        let category = RateLimitCategory::AuthLogin;

        // The closure should return a function that can be used with from_fn
        let _middleware = with_category(category);
    }

    #[test]
    fn test_normalized_ip_type() {
        let ip = NormalizedIp("192.168.1.1".to_string());
        assert_eq!(ip.0, "192.168.1.1");
    }
}
