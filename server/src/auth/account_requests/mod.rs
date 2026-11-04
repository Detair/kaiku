//! Account requests (spec SM3; F6 recovery, F8 bootstrap, F5c admin setup;
//! contract C27 "Account requests").
//!
//! **SECURITY CRITICAL.** A request is a delayed change to an account's
//! authenticators. It records the factor generations it used and its
//! independent cancellers (other admins, a confirmed recovery contact
//! confirmed at least one delay period earlier, the account's own strong
//! authenticators); without one it cannot start, and with none left it is
//! `void`. It waits `ACCOUNT_REQUEST_DELAY_HOURS` (24 by default) and tells
//! the account's devices (also those named lost), its contact and every
//! admin. Anyone of them may cancel (owner rule, B.2); the starting client
//! can only withdraw. Completion re-checks the generations and a route
//! (B.7 rule 4).

pub mod client;
pub mod core;
pub mod handlers;
pub mod recovery;
pub mod types;

use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{get, post};
use axum::Router;

pub use self::core::request_id_by_secret;
use crate::api::AppState;
use crate::ratelimit::{rate_limit_by_ip, with_category, RateLimitCategory};

/// The public request routes under `/auth` (`AuthFlow` per IP).
pub fn public_router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/account-requests", post(handlers::start))
        .route(
            "/account-requests/current",
            get(handlers::current).delete(handlers::withdraw),
        )
        .route(
            "/account-requests/oidc/authorize/{provider_id}",
            post(recovery::oidc_request_authorize),
        )
        .merge(client::routes())
        .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
        .layer(from_fn(with_category(RateLimitCategory::AuthFlow)))
}

/// The bearer routes (`/api/me/account-requests`,
/// `/api/account-requests/{id}/cancel`), merged at the API root.
pub fn bearer_router() -> Router<AppState> {
    Router::new()
        .route("/api/me/account-requests", get(handlers::list_own))
        .route("/api/account-requests/{id}/cancel", post(handlers::cancel))
}
