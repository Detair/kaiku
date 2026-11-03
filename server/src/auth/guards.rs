//! The two server guards (spec B.4, C27 common rules).
//!
//! *Full session* (here) and *step-up grant for this action*
//! (`step_up::consume_tx`). Both decide from committed rows: the session's
//! assurance comes from the row `require_auth` read (B.4 rev 5), never from
//! the token.

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use super::assurance::Assurance;
use super::error::{AuthError, AuthResult};
use super::middleware::AuthUser;

/// 403 `FULL_SESSION_REQUIRED` unless the caller's session is `full`.
pub fn require_full(auth: &AuthUser) -> AuthResult<()> {
    if auth.assurance() == Assurance::Full {
        Ok(())
    } else {
        Err(AuthError::FullSessionRequired)
    }
}

/// An `admin.*` step-up is only for a system admin with a full session.
///
/// Checked like `/api/admin/*` (`NOT_ADMIN` first, then
/// `FULL_SESSION_REQUIRED`) and before any target label is resolved, so a
/// non-admin never learns another account's authenticator labels or user
/// names this way. Other actions pass unchanged.
pub async fn require_admin_for_step_up(
    db: &sqlx::PgPool,
    auth: &AuthUser,
    action: super::step_up::StepUpAction,
) -> AuthResult<()> {
    if !action.is_admin() {
        return Ok(());
    }
    if !crate::permissions::queries::is_system_admin(db, auth.id).await? {
        return Err(AuthError::NotAdmin);
    }
    require_full(auth)
}

/// Route layer after `require_auth`: only full sessions pass.
pub async fn require_full_session(request: Request, next: Next) -> Result<Response, AuthError> {
    let auth = request
        .extensions()
        .get::<AuthUser>()
        .ok_or(AuthError::MissingAuthHeader)?;
    require_full(auth)?;
    Ok(next.run(request).await)
}
