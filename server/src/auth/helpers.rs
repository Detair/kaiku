//! Shared helpers used across authentication HTTP handlers.

use axum::http::header::{ORIGIN, USER_AGENT};
use axum::http::HeaderMap;
use axum_extra::extract::CookieJar;

use super::cookies;
use super::error::{AuthError, AuthResult};
use crate::config::TAURI_ORIGINS;

/// Hash a token for secure storage using SHA256.
///
/// Used for storing refresh tokens — raw tokens are never stored.
#[must_use]
pub fn hash_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Extract a refresh token from either the JSON body or `HttpOnly` cookie.
pub(super) fn extract_refresh_token(
    body_token: Option<String>,
    jar: &CookieJar,
) -> AuthResult<String> {
    if let Some(token) = body_token.filter(|t| !t.is_empty()) {
        tracing::debug!("Refresh token provided via request body");
        return Ok(token);
    }

    if let Some(cookie) = jar.get(cookies::REFRESH_COOKIE_NAME) {
        let value = cookie.value().to_owned();
        if value.is_empty() {
            tracing::warn!("Refresh cookie present but empty");
        } else {
            tracing::debug!("Refresh token provided via HttpOnly cookie");
            return Ok(value);
        }
    }

    tracing::debug!("No refresh token found in body or cookie");
    Err(AuthError::InvalidToken)
}

/// Returns `true` when the refresh token should be included in the JSON body.
///
/// Browser (CORS) requests include the `Origin` header; for those clients the
/// refresh token is delivered solely via `HttpOnly` cookie.  Non-browser clients
/// omit `Origin` and receive the token in the response body, as does the Tauri
/// desktop webview (its origin is one of [`TAURI_ORIGINS`]; cookies do not
/// survive there).
pub(super) fn should_return_refresh_token(headers: &HeaderMap) -> bool {
    let origin = headers.get(ORIGIN);
    let deliver =
        origin.is_none_or(|value| value.to_str().is_ok_and(|v| TAURI_ORIGINS.contains(&v)));
    tracing::debug!(
        has_origin_header = origin.is_some(),
        deliver_in_body = deliver,
        "Refresh token delivery decision"
    );
    deliver
}

/// Extract the current session's token hash from cookie (browser) or `X-Refresh-Token` header
/// (Tauri).
///
/// Browser clients send the refresh token via an `HttpOnly` cookie.
/// Tauri clients cannot use cookies for same-origin requests to the API, so they
/// send the refresh token via the `X-Refresh-Token` header instead.
pub(super) fn extract_current_token_hash(headers: &HeaderMap, jar: &CookieJar) -> Option<String> {
    // Try cookie first (browser clients)
    if let Some(cookie) = jar.get(cookies::REFRESH_COOKIE_NAME) {
        return Some(hash_token(cookie.value()));
    }
    // Fall back to X-Refresh-Token header (Tauri clients)
    if let Some(header_value) = headers.get("x-refresh-token") {
        if let Ok(token) = header_value.to_str() {
            if !token.is_empty() {
                return Some(hash_token(token));
            }
        }
    }
    None
}

/// Extract User-Agent from headers (sanitized and truncated to 512 chars for DB storage).
pub(super) fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            // Sanitize: remove control characters (except whitespace) to prevent
            // injection attacks and display issues in admin panels
            // Then truncate to 512 chars to prevent DoS and match DB constraint
            s.chars()
                .filter(|c| !c.is_control() || c.is_whitespace())
                .take(512)
                .collect()
        })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::*;
    use crate::config::TAURI_ORIGINS;

    fn origin(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_str(value).unwrap());
        headers
    }

    #[test]
    fn refresh_token_returned_without_origin() {
        assert!(should_return_refresh_token(&HeaderMap::new()));
    }

    #[test]
    fn refresh_token_returned_for_tauri_origin() {
        for o in TAURI_ORIGINS {
            assert!(should_return_refresh_token(&origin(o)), "{o}");
        }
    }

    #[test]
    fn refresh_token_withheld_for_web_origin() {
        assert!(!should_return_refresh_token(&origin(
            "https://kaiku.example"
        )));
        assert!(!should_return_refresh_token(&origin(
            "https://kaiku.pmind.de"
        )));
    }
}
