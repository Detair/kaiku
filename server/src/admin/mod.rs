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
/// Most routes require system admin privileges (applied via middleware).
/// Routes under the elevated router additionally require an elevated session.
/// The `/status` endpoint is accessible to any authenticated user.
pub fn router(state: AppState) -> Router<AppState> {
    // Elevated routes (require both system admin and elevated session)
    let elevated_routes = Router::new()
        .route(
            "/users/:id/ban",
            post(handlers::ban_user).delete(handlers::unban_user),
        )
        .route("/users/:id/unban", post(handlers::unban_user))
        .route("/users/bulk-ban", post(handlers::bulk_ban_users))
        .route(
            "/guilds/:id/suspend",
            post(handlers::suspend_guild).delete(handlers::unsuspend_guild),
        )
        .route("/guilds/:id/unsuspend", post(handlers::unsuspend_guild))
        .route("/guilds/bulk-suspend", post(handlers::bulk_suspend_guilds))
        .route("/announcements", post(handlers::create_announcement))
        .layer(from_fn_with_state(state.clone(), require_elevated));

    // Non-elevated admin routes (require system admin)
    let admin_routes = Router::new()
        .route("/health", get(|| async { "admin ok" }))
        .route("/stats", get(handlers::get_admin_stats))
        .route("/users", get(handlers::list_users))
        .route("/users/export", get(handlers::export_users_csv))
        .route("/users/:id/details", get(handlers::get_user_details))
        .route("/guilds", get(handlers::list_guilds))
        .route("/guilds/export", get(handlers::export_guilds_csv))
        .route("/guilds/:id/details", get(handlers::get_guild_details))
        .route("/audit-log", get(handlers::get_audit_log))
        .route(
            "/elevate",
            post(handlers::elevate_session).delete(handlers::de_elevate_session),
        )
        .merge(elevated_routes)
        .layer(from_fn_with_state(state, require_system_admin));

    // Public admin routes (any authenticated user)
    // /status endpoint allows users to check their own admin status
    Router::new()
        .route("/status", get(handlers::get_admin_status))
        .merge(admin_routes)
}
