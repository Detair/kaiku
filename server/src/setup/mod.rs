//! Server Setup API
//!
//! Endpoints for the first-time setup wizard that configures the server.

pub mod error;
pub mod handlers;
pub mod types;

pub use error::SetupError;
pub use types::{CompleteSetupRequest, SetupConfigResponse, SetupStatusResponse};

use axum::middleware::from_fn_with_state;
use axum::routing::{get, post};
use axum::Router;

use crate::api::AppState;

/// Create the setup router, mounted at `/api/setup`.
///
/// `status` and `config` are public; `complete` requires authentication.
pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/status", get(handlers::status))
        .route("/config", get(handlers::get_config))
        .route(
            "/complete",
            post(handlers::complete)
                .route_layer(from_fn_with_state(state, crate::auth::require_auth)),
        )
}
