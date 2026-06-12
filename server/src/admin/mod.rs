//! System Admin Module
//!
//! Provides admin-only endpoints for platform management:
//! - Non-elevated: list users, list guilds, audit log, elevate/de-elevate session
//! - Elevated: ban users, suspend guilds, manage announcements

pub mod audit;
pub mod error;
pub mod guilds;
pub mod middleware;
pub mod observability;
pub mod queries;
pub(crate) mod shared;
pub mod system;
pub mod types;
pub mod users;

use axum::middleware::from_fn_with_state;
use axum::routing::{delete, get, post, put};
use axum::Router;
pub use error::AdminError;
pub use middleware::{require_elevated, require_system_admin};
pub use shared::{cache_elevated_status, is_elevated_admin};
pub use types::{ElevatedAdmin, SystemAdminUser};

use crate::api::AppState;

/// Create the admin router.
///
/// Most routes require system admin privileges (applied via middleware).
/// Routes under the elevated router additionally require an elevated session.
/// The `/status` endpoint is accessible to any authenticated user.
pub fn router(state: AppState) -> Router<AppState> {
    // Elevated routes (require both system admin and elevated session)
    let elevated_routes = Router::new()
        // Report management
        .route(
            "/reports",
            get(crate::moderation::admin_handlers::list_reports),
        )
        .route(
            "/reports/stats",
            get(crate::moderation::admin_handlers::report_stats),
        )
        .route(
            "/reports/{id}",
            get(crate::moderation::admin_handlers::get_report),
        )
        .route(
            "/reports/{id}/claim",
            post(crate::moderation::admin_handlers::claim_report),
        )
        .route(
            "/reports/{id}/resolve",
            post(crate::moderation::admin_handlers::resolve_report),
        )
        // User management
        .route(
            "/users/{id}/ban",
            post(users::ban_user).delete(users::unban_user),
        )
        .route("/users/{id}/unban", post(users::unban_user))
        .route("/users/bulk-ban", post(users::bulk_ban_users))
        .route("/users/{id}", delete(users::delete_user))
        .route(
            "/guilds/{id}/suspend",
            post(guilds::suspend_guild).delete(guilds::unsuspend_guild),
        )
        .route("/guilds/{id}/unsuspend", post(guilds::unsuspend_guild))
        .route("/guilds/bulk-suspend", post(guilds::bulk_suspend_guilds))
        .route("/guilds/{id}", delete(guilds::delete_guild))
        .route("/announcements", post(system::create_announcement))
        // Auth settings (OIDC provider management)
        .route(
            "/auth-settings",
            get(system::get_auth_settings).put(system::update_auth_settings),
        )
        .route(
            "/oidc-providers",
            get(system::list_oidc_providers).post(system::create_oidc_provider),
        )
        .route(
            "/oidc-providers/{id}",
            put(system::update_oidc_provider).delete(system::delete_oidc_provider),
        )
        // Per-guild page limits
        .route(
            "/guilds/{id}/page-limits",
            get(guilds::get_guild_page_limits).patch(guilds::set_guild_page_limits),
        )
        .layer(from_fn_with_state(state.clone(), require_elevated));

    // Non-elevated admin routes (require system admin)
    let admin_routes = Router::new()
        .route("/health", get(|| async { "admin ok" }))
        .route("/stats", get(system::get_admin_stats))
        .route("/diagnostics", get(system::get_diagnostics))
        .route("/users", get(users::list_users))
        .route("/users/export", get(users::export_users_csv))
        .route("/users/{id}/details", get(users::get_user_details))
        .route("/guilds", get(guilds::list_guilds))
        .route("/guilds/export", get(guilds::export_guilds_csv))
        .route("/guilds/{id}/details", get(guilds::get_guild_details))
        .route("/audit-log", get(audit::get_audit_log))
        .route(
            "/elevate",
            post(system::elevate_session).delete(system::de_elevate_session),
        )
        .nest("/observability", observability::router())
        .merge(elevated_routes)
        .layer(from_fn_with_state(state, require_system_admin));

    // Public admin routes (any authenticated user)
    // /status endpoint allows users to check their own admin status
    Router::new()
        .route("/status", get(system::get_admin_status))
        .merge(admin_routes)
}
