//! Admin authentication and authorization middleware.

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

use super::error::AdminError;
use super::queries;
use super::types::{ElevatedAdmin, SystemAdminUser};
use crate::api::AppState;
use crate::auth::AuthUser;
use crate::permissions::queries::get_system_admin;

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
///
/// The elevation must belong to the session of the request's access token
/// (`sid`); other sessions of the same admin are not elevated (C24).
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

    let session_id = request
        .extensions()
        .get::<AuthUser>()
        .and_then(|auth| auth.session_id)
        .ok_or(AdminError::ElevationRequired)?;

    let elevated = queries::find_elevated_session(&state.db, admin.user_id, session_id)
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
