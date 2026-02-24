//! Guild Discovery Module
//!
//! Provides public browsing of discoverable guilds and join-via-discovery.

pub mod handlers;
pub mod types;

use axum::routing::{get, post};
use axum::Router;

use crate::api::AppState;

/// Public routes (no auth required) — guild browsing.
pub fn public_router() -> Router<AppState> {
    Router::new().route("/guilds", get(handlers::browse_guilds))
}

/// Protected routes (auth required) — joining guilds.
pub fn protected_router() -> Router<AppState> {
    Router::new().route("/guilds/{id}/join", post(handlers::join_discoverable))
}
