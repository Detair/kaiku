//! Shared TOTP acceptance and `GET /auth/reauth/methods` (C27).
//!
//! C24's password/TOTP and OIDC re-authentication is gone: sensitive actions
//! need a full session and an action-bound step-up grant (`step_up`, F7).
//! What remains here is the TOTP replay store ([`accept_totp`]) used by the
//! TOTP sign-in step and the TOTP setup, the client IP helper, and the list
//! of step-up methods the caller has.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::mfa_crypto::decrypt_mfa_secret;
use super::middleware::AuthUser;
use super::types::ReauthMethodsResponse;
use crate::api::AppState;
use crate::ratelimit::extract_client_ip;

/// The client IP as the rate limiter sees it (trusted proxies applied).
pub fn client_ip(state: &AppState, headers: &HeaderMap, addr: SocketAddr) -> IpAddr {
    match state.rate_limiter {
        Some(ref limiter) => extract_client_ip(
            headers,
            Some(&ConnectInfo(addr)),
            &limiter.config().trusted_proxies,
        ),
        None => addr.ip(),
    }
}

/// Lifetime of the last accepted TOTP step per user (covers the ±1 step
/// window).
const TOTP_STEP_TTL_SECS: i64 = 90;

/// Store `step` as the user's last accepted TOTP step unless an equal or later
/// step was already accepted. Returns 1 when stored, 0 for a replay.
const TOTP_ACCEPT_SCRIPT: &str = r"
local last = redis.call('GET', KEYS[1])
if last and tonumber(last) >= tonumber(ARGV[1]) then
    return 0
end
redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
return 1
";

/// Build the TOTP generator of an account (no skew; steps are checked one by
/// one so the matching step is known).
fn account_totp(state: &AppState, encrypted_secret: &str, username: &str) -> AuthResult<TOTP> {
    let encryption_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;
    let key_bytes = hex::decode(encryption_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;
    let secret_str = decrypt_mfa_secret(encrypted_secret, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to decrypt MFA secret: {e}")))?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        0,
        30,
        Secret::Encoded(secret_str)
            .to_bytes()
            .map_err(|_| AuthError::Internal("Invalid TOTP secret encoding".into()))?,
        Some("Kaiku".to_string()),
        username.to_string(),
    )
    .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))
}

/// Accept a TOTP code for this user at most once.
///
/// The code may match the current, previous or next 30-second step. The
/// matched step must be later than the last step accepted for the user
/// (stored in Valkey for 90 s), so a code cannot be replayed on the
/// re-authentication paths. Returns `false` for a wrong or replayed code.
/// Fails closed with 503 when Valkey cannot be used.
pub async fn accept_totp(
    state: &AppState,
    user_id: Uuid,
    encrypted_secret: &str,
    username: &str,
    code: &str,
) -> AuthResult<bool> {
    use fred::interfaces::LuaInterface;

    let totp = account_totp(state, encrypted_secret, username)?;
    let now = u64::try_from(Utc::now().timestamp()).unwrap_or(0);
    let current = now / 30;
    let matched = [current.saturating_sub(1), current, current + 1]
        .into_iter()
        .find(|step| totp.check(code, step * 30));
    let Some(step) = matched else {
        return Ok(false);
    };

    let stored: i64 = state
        .redis
        .eval(
            TOTP_ACCEPT_SCRIPT,
            vec![format!("mfa:totp_step:{user_id}")],
            vec![step.to_string(), TOTP_STEP_TTL_SECS.to_string()],
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, %user_id, "Cannot record the accepted TOTP step; refusing");
            AuthError::ServiceUnavailable
        })?;
    if stored == 0 {
        tracing::info!(%user_id, "Replayed TOTP code refused");
    }
    Ok(stored == 1)
}

// ============================================================================
// Methods and OIDC re-authentication
// ============================================================================

/// How the caller can re-authenticate (C24).
///
/// GET /auth/reauth/methods
#[utoipa::path(
    get,
    path = "/auth/reauth/methods",
    tag = "auth",
    responses((status = 200, description = "Available re-authentication methods", body = ReauthMethodsResponse)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn get_reauth_methods(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<ReauthMethodsResponse>> {
    // F7: a step-up is a passkey assertion (an active passkey) or a
    // `device_stepup` from an active key device; never a password, TOTP or
    // OIDC.
    // `device` counts key devices only: `strong::active_strong_count`
    // includes passkeys.
    let device: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE user_id = $1 AND state = 'active')",
    )
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;
    let passkey = super::passkeys::queries::active_count(&state.db, auth_user.id).await? > 0;
    Ok(Json(ReauthMethodsResponse { passkey, device }))
}
