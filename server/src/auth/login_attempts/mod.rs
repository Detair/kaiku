//! Login attempts (spec SM1; F2 QR sign-in, F3/F4; contract C27).
//!
//! **SECURITY CRITICAL.** A PC that wants a session creates an attempt and
//! keeps a collection secret (browser: an `HttpOnly` cookie; native: a
//! verifier whose S256 challenge it sent). The attempt carries a 256-bit QR
//! token (rotated every 60 s, stored only as a keyed hash). A key device
//! claims the attempt with the scanned token (`POST /api/approvals/claim`),
//! which creates a `login` challenge (C26); its UV signature over the token
//! approves. The PC then collects a **probation** session with its secret.
//!
//! Lock order (B.7 rule 1): device-side transitions (claim, answer, deny,
//! release) lock accounts → device → session → challenge → attempt;
//! PC-side transitions that mint a session (collect, TOTP) lock the account
//! → attempt; poll and cancel touch only the attempt row and never a
//! challenge (an answer whose attempt has ended voids its own challenge).

pub mod core;
pub mod handlers;
pub mod types;

use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::post;
use axum::Router;

use crate::api::AppState;
use crate::ratelimit::{rate_limit_by_ip, with_category, RateLimitCategory};

/// The public attempt routes under `/auth` (`AuthFlow` per IP, never
/// IP-blocked).
pub fn router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/login-attempts", post(handlers::create))
        .route("/login-attempts/{id}/poll", post(handlers::poll))
        .route("/login-attempts/{id}/collect", post(handlers::collect))
        .route("/login-attempts/{id}/cancel", post(handlers::cancel))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_ip))
        .layer(from_fn(with_category(RateLimitCategory::AuthFlow)))
}
