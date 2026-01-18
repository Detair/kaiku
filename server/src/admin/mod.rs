//! System Admin Module
//!
//! Provides admin-only endpoints for platform management:
//! - Non-elevated: list users, list guilds, audit log, elevate/de-elevate session
//! - Elevated: ban users, suspend guilds, manage announcements

pub mod handlers;
pub mod middleware;
pub mod types;

use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};

use crate::api::AppState;

pub use middleware::{require_elevated, require_system_admin};
pub use types::{AdminError, ElevatedAdmin, SystemAdminUser};

/// Create the admin router.
///
/// All routes require system admin privileges (applied via middleware).
/// Routes under the elevated router additionally require an elevated session.
pub fn router(state: AppState) -> Router<AppState> {
    // Elevated routes (require both system admin and elevated session)
    let elevated_routes = Router::new()
        .route(
            "/users/:id/ban",
            post(handlers::ban_user).delete(handlers::unban_user),
        )
        .route(
            "/guilds/:id/suspend",
            post(handlers::suspend_guild).delete(handlers::unsuspend_guild),
        )
        .route("/announcements", post(handlers::create_announcement))
        .layer(from_fn_with_state(state.clone(), require_elevated));

    // Non-elevated admin routes
    let base_routes = Router::new()
        .route("/health", get(|| async { "admin ok" }))
        .route("/users", get(handlers::list_users))
        .route("/guilds", get(handlers::list_guilds))
        .route("/audit-log", get(handlers::get_audit_log))
        .route(
            "/elevate",
            post(handlers::elevate_session).delete(handlers::de_elevate_session),
        )
        .merge(elevated_routes);

    // Apply system admin middleware to all routes
    base_routes.layer(from_fn_with_state(state, require_system_admin))
}
