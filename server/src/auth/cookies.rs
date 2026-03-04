//! Cookie helpers for browser-mode refresh token storage.
//!
//! In browser mode the refresh token is stored as an `HttpOnly` cookie
//! so it cannot be read by JavaScript (XSS mitigation).  Tauri clients
//! continue to use the JSON body.

use axum_extra::extract::cookie::{Cookie, SameSite};
use time::Duration;

use crate::config::Config;

/// Cookie name for the refresh token.
pub const REFRESH_COOKIE_NAME: &str = "kaiku_refresh";

/// Parse the configured `SameSite` policy string into the enum variant.
///
/// Assumes the value has already been validated at startup (`Config::from_env`).
fn parse_same_site(config: &Config) -> SameSite {
    match config.cookie_same_site.as_str() {
        "strict" => SameSite::Strict,
        "none" => SameSite::None,
        other => {
            if other != "lax" {
                tracing::warn!(
                    value = %other,
                    "Unexpected cookie_same_site value, defaulting to Lax"
                );
            }
            SameSite::Lax
        }
    }
}

/// Build a base cookie with shared security attributes.
fn base_cookie(config: &Config) -> Cookie<'static> {
    let mut cookie = Cookie::build((REFRESH_COOKIE_NAME, String::new()))
        .http_only(true)
        .same_site(parse_same_site(config))
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
