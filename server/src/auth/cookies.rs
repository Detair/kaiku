//! Cookie helpers for browser-mode refresh token storage.
//!
//! In browser mode the refresh token is stored as an `HttpOnly` cookie
//! so it cannot be read by JavaScript (XSS mitigation).  Tauri clients
//! continue to use the JSON body.

use axum_extra::extract::cookie::{Cookie, SameSite};
use time::Duration;

use crate::config::Config;

/// Cookie name for the refresh token.
/// The refresh cookie (B.4: `__Secure-` prefix, `HttpOnly`, `Secure`,
/// `SameSite=Strict`, `Path=/auth`). Without `COOKIE_SECURE` (local
/// development) the prefix is dropped: browsers refuse `__Secure-` cookies
/// without `Secure`.
pub const REFRESH_COOKIE: &str = "__Secure-kaiku_refresh";

/// The pre-v1.0 refresh cookie name, read once and cleared.
pub const LEGACY_REFRESH_COOKIE: &str = "kaiku_refresh";

/// `base` with the `__Secure-` prefix only when cookies are `Secure`.
#[must_use]
pub fn cookie_name(base: &'static str, config: &Config) -> &'static str {
    if config.cookie_secure {
        base
    } else {
        base.strip_prefix("__Secure-").unwrap_or(base)
    }
}

/// Build a base cookie with shared security attributes.
fn base_cookie(config: &Config) -> Cookie<'static> {
    named_cookie(cookie_name(REFRESH_COOKIE, config), config)
}

fn named_cookie(name: &'static str, config: &Config) -> Cookie<'static> {
    // Always Strict (B.4); `COOKIE_SAMESITE` no longer loosens it.
    let mut cookie = Cookie::build((name, String::new()))
        .http_only(true)
        .same_site(SameSite::Strict)
        .path("/auth")
        .secure(config.cookie_secure)
        .build();

    if let Some(domain) = &config.cookie_domain {
        cookie.set_domain(domain.clone());
    }

    cookie
}

/// Build an `HttpOnly` cookie that stores the refresh token.
pub fn build_refresh_cookie(token: &str, max_age_secs: i64, config: &Config) -> Cookie<'static> {
    let mut cookie = base_cookie(config);
    cookie.set_value(token.to_owned());
    cookie.set_max_age(Duration::seconds(max_age_secs));
    cookie
}

/// Build a cookie that instructs the browser to delete the refresh token (Max-Age=0).
pub fn build_clear_cookie(config: &Config) -> Cookie<'static> {
    let mut cookie = base_cookie(config);
    cookie.set_max_age(Duration::ZERO);
    cookie
}

/// Clear the pre-v1.0 cookie (`kaiku_refresh`) after it was read once.
#[must_use]
pub fn build_clear_legacy_cookie(config: &Config) -> Cookie<'static> {
    let mut cookie = named_cookie(LEGACY_REFRESH_COOKIE, config);
    cookie.set_max_age(Duration::ZERO);
    cookie
}
