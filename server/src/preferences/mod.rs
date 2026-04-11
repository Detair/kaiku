//! User Preferences API
//!
//! Endpoints for managing user preferences that sync across devices.

pub mod error;
pub mod handlers;
pub mod types;
pub mod validation;

pub use error::PreferencesError;
pub use types::{PreferencesResponse, UpdatePreferencesRequest, UserPreferencesRow};

use axum::routing::get;
use axum::Router;

use crate::api::AppState;

/// Create the preferences router.
///
/// Routes:
/// - GET / - Get current user's preferences
/// - PUT / - Update current user's preferences (full replacement)
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(handlers::get_preferences).put(handlers::update_preferences))
}
