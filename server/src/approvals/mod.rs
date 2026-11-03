//! Device signatures and approvals (spec B.2, contract C26).
//!
//! **SECURITY CRITICAL.** See `AGENTS.md` in this directory.

pub mod cascade;
pub mod context;
pub mod core;
pub mod devices;
pub mod error;
pub mod handlers;
pub mod secrets;
pub mod signing;
pub mod transition;
pub mod types;

#[cfg(test)]
mod golden_tests;

use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::{delete, get, post};
use axum::Router;

use crate::api::AppState;
use crate::ratelimit::{rate_limit_by_user, with_category, RateLimitCategory};

/// `/api/approvals` (bearer; `ApprovalLogin` per user).
pub fn approvals_router(state: &AppState) -> Router<AppState> {
    Router::new()
        .route("/step-up", post(handlers::step_up))
        .route("/{id}/answer", post(handlers::answer))
        .route("/{id}/deny", post(handlers::deny))
        .route("/{id}/release", post(handlers::release))
        .layer(from_fn_with_state(state.clone(), rate_limit_by_user))
        .layer(from_fn(with_category(RateLimitCategory::ApprovalLogin)))
}

/// `/api/approval-devices` (bearer).
pub fn devices_router() -> Router<AppState> {
    Router::new()
        .route("/", get(devices::handlers::list))
        .route("/{id}", delete(devices::handlers::remove))
        .route("/{id}/invalidated", post(devices::handlers::invalidated))
}
