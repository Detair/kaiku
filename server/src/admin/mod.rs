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
    routing::{delete, get, post, put},
    Router,
};

use crate::api::AppState;

pub use middleware::{require_elevated, require_system_admin};
pub use types::{AdminError, ElevatedAdmin, SystemAdminUser};

use fred::prelude::*;
use sqlx::PgPool;
use uuid::Uuid;

/// Check if a user is an elevated admin (for WebSocket subscription check).
/// This checks both system admin status and elevated session validity.
///
/// Security: Always falls back to database on cache miss to ensure fail-secure behavior.
pub async fn is_elevated_admin(redis: &Client, db: &PgPool, user_id: Uuid) -> bool {
    // Check cache first (fast path)
    let cache_key = format!("admin:elevated:{user_id}");
    let cached: Option<String> = redis.get(&cache_key).await.ok().flatten();

    if let Some(value) = cached {
        return value == "1";
    }

    // Cache miss - fallback to database (fail-secure)
    let is_elevated = check_elevated_in_db(db, user_id).await;

    // Cache the result (60s TTL to balance freshness and load)
    if is_elevated {
        cache_elevated_status(redis, user_id, true, 60).await;
    }

    is_elevated
}

/// Check elevated session status directly in the database.
async fn check_elevated_in_db(db: &PgPool, user_id: Uuid) -> bool {
    sqlx::query_scalar!(
        r#"SELECT EXISTS(
            SELECT 1 FROM elevated_sessions
            WHERE user_id = $1 AND expires_at > NOW()
        ) as "exists!""#,
        user_id
    )
    .fetch_one(db)
    .await
    .unwrap_or(false)
}

/// Cache elevated admin status in Redis (called after elevation).
pub async fn cache_elevated_status(
    redis: &Client,
    user_id: Uuid,
    is_elevated: bool,
    ttl_secs: i64,
) {
    let cache_key = format!("admin:elevated:{user_id}");
    let value = if is_elevated { "1" } else { "0" };

    let _: Result<(), _> = redis
        .set(
            &cache_key,
            value,
            Some(Expiration::EX(ttl_secs)),
            None,
            false,
        )
        .await;
}

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
            post(handlers::ban_user).delete(handlers::unban_user),
        )
        .route("/users/{id}/unban", post(handlers::unban_user))
        .route("/users/bulk-ban", post(handlers::bulk_ban_users))
        .route("/users/{id}", delete(handlers::delete_user))
        .route(
            "/guilds/{id}/suspend",
            post(handlers::suspend_guild).delete(handlers::unsuspend_guild),
        )
        .route("/guilds/{id}/unsuspend", post(handlers::unsuspend_guild))
        .route("/guilds/bulk-suspend", post(handlers::bulk_suspend_guilds))
        .route("/guilds/{id}", delete(handlers::delete_guild))
        .route("/announcements", post(handlers::create_announcement))
        // Auth settings (OIDC provider management)
        .route(
            "/auth-settings",
            get(handlers::get_auth_settings).put(handlers::update_auth_settings),
        )
        .route(
            "/oidc-providers",
            get(handlers::list_oidc_providers).post(handlers::create_oidc_provider),
        )
        .route(
            "/oidc-providers/{id}",
            put(handlers::update_oidc_provider).delete(handlers::delete_oidc_provider),
        )
        .layer(from_fn_with_state(state.clone(), require_elevated));

    // Non-elevated admin routes (require system admin)
    let admin_routes = Router::new()
        .route("/health", get(|| async { "admin ok" }))
        .route("/stats", get(handlers::get_admin_stats))
        .route("/users", get(handlers::list_users))
        .route("/users/export", get(handlers::export_users_csv))
        .route("/users/{id}/details", get(handlers::get_user_details))
        .route("/guilds", get(handlers::list_guilds))
        .route("/guilds/export", get(handlers::export_guilds_csv))
        .route("/guilds/{id}/details", get(handlers::get_guild_details))
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
