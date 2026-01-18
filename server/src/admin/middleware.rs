//! Admin authentication and authorization middleware.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};

use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::queries::get_system_admin;

use super::types::{AdminError, ElevatedAdmin, SystemAdminUser};

/// Middleware that requires the user to be a system admin.
#[tracing::instrument(skip(state, request, next))]
pub async fn require_system_admin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AdminError> {
    let auth = request
        .extensions()
        .get::<AuthUser>()
        .cloned()
        .ok_or(AdminError::NotAdmin)?;

    let admin = get_system_admin(&state.db, auth.id)
        .await?
        .ok_or(AdminError::NotAdmin)?;

    let admin_user = SystemAdminUser {
        user_id: auth.id,
        username: auth.username,
        granted_at: admin.granted_at,
    };
    request.extensions_mut().insert(admin_user);

    Ok(next.run(request).await)
}

/// Middleware that requires an elevated admin session.
#[tracing::instrument(skip(state, request, next))]
pub async fn require_elevated(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AdminError> {
    let admin = request
        .extensions()
        .get::<SystemAdminUser>()
        .cloned()
        .ok_or(AdminError::NotAdmin)?;

    let elevated = sqlx::query!(
        r#"SELECT id, user_id, elevated_at, expires_at, reason
           FROM elevated_sessions
           WHERE user_id = $1 AND expires_at > NOW()
           ORDER BY elevated_at DESC
           LIMIT 1"#,
        admin.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(AdminError::ElevationRequired)?;

    let elevated_admin = ElevatedAdmin {
        user_id: elevated.user_id,
        elevated_at: elevated.elevated_at,
        expires_at: elevated.expires_at,
        reason: elevated.reason,
    };
    request.extensions_mut().insert(elevated_admin);

    Ok(next.run(request).await)
}
