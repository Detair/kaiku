//! Self-service recovery (F6) and the fresh-OIDC request factor (L1b
//! Task 27).

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::core::StartedRequest;
use super::types::StartRequest;
use crate::api::AppState;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};

/// A fresh OIDC sign-in usable as a request factor.
#[derive(Debug, Clone)]
pub struct OidcFactor {
    pub user_id: Uuid,
    pub provider_slug: String,
    pub auth_time: DateTime<Utc>,
}

/// Start a recovery request (Task 27).
pub async fn start_recovery(
    _state: &AppState,
    _ctx: &AuthCtx<'_>,
    _body: &StartRequest,
) -> AuthResult<StartedRequest> {
    Err(AuthError::InvalidCredentials)
}

/// Redeem a one-time request OIDC code (Task 27).
pub async fn redeem_oidc_code(
    _ctx: &AuthCtx<'_>,
    _code: &str,
    _verifier: Option<&str>,
) -> AuthResult<OidcFactor> {
    Err(AuthError::InvalidCredentials)
}
