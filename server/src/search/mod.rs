//! Global Message Search
//!
//! Full-text search across all guilds and DMs the authenticated user has access to.

pub mod error;
pub mod handlers;
pub mod types;

pub use error::GlobalSearchError;
pub use types::{GlobalSearchQuery, GlobalSearchResponse, GlobalSearchResult};

use axum::routing::get;
use axum::Router;

use crate::api::AppState;

/// Create the global search router, mounted at `/api/search`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(handlers::search_all))
}
