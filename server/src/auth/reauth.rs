//! Re-authentication for sensitive actions (C24).
//!
//! Admin elevation and backup-code regeneration ask the caller to prove the
//! account again: the password and, when MFA is on, a current TOTP code
//! (backup codes are not accepted). A password change checks the current
//! password with the same helpers.
//!
//! Every failure is recorded twice with `RateLimiter::record_failed_auth`:
//! - per client IP, which the `check_ip_not_blocked` layer on these routes
//!   (and on `/auth/login`) enforces;
//! - per account (`reauth:{user_id}`), enforced here, so failures spread over
//!   several addresses still lock the account's re-authentication.

use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::mfa_crypto::decrypt_mfa_secret;
use super::password::verify_password;
use crate::api::AppState;
use crate::db::find_user_by_id;
use crate::ratelimit::{extract_client_ip, normalize_ip};

/// Message for a fail-closed rate limiter outage.
const LIMITER_UNAVAILABLE: &str =
    "Authentication service temporarily unavailable. Please try again later.";

/// Failed-attempt key of an account's re-authentication.
fn account_key(user_id: Uuid) -> String {
    format!("reauth:{user_id}")
}

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

/// Refuse while the account's re-authentication is locked after failures.
pub(crate) async fn ensure_not_locked(state: &AppState, user_id: Uuid) -> AuthResult<()> {
    let Some(ref limiter) = state.rate_limiter else {
        return Ok(());
    };
    let key = account_key(user_id);
    match limiter.is_blocked(&key).await {
        Ok(false) => Ok(()),
        Ok(true) => {
            let retry_after = limiter
                .get_block_ttl(&key)
                .await
                .unwrap_or(limiter.config().limits.failed_auth.block_duration_secs);
            Err(AuthError::TooManyAttempts { retry_after })
        }
        // Same policy as `check_ip_not_blocked`.
        Err(_) if limiter.config().fail_open => Ok(()),
        Err(e) => {
            tracing::error!(error = %e, %user_id, "Cannot check re-authentication lock");
            Err(AuthError::Internal(LIMITER_UNAVAILABLE.to_string()))
        }
    }
}

/// Record a failed re-authentication for the client IP and the account.
///
/// Fails closed: if the failure cannot be recorded the request is refused,
/// so a limiter outage cannot be used to guess without limit.
pub(crate) async fn record_failure(
    state: &AppState,
    user_id: Uuid,
    client_ip: IpAddr,
) -> AuthResult<()> {
    let Some(ref limiter) = state.rate_limiter else {
        return Ok(());
    };
    for key in [normalize_ip(client_ip), account_key(user_id)] {
        if let Err(e) = limiter.record_failed_auth(&key).await {
            tracing::error!(
                error = %e,
                %user_id,
                "Cannot record failed re-authentication; refusing the request"
            );
            return Err(AuthError::Internal(LIMITER_UNAVAILABLE.to_string()));
        }
    }
    Ok(())
}

/// Check a password against an Argon2 hash off the async runtime.
///
/// A malformed stored hash counts as a mismatch.
pub(crate) async fn password_matches(password: &str, hash: &str) -> AuthResult<bool> {
    let password = password.to_owned();
    let hash = hash.to_owned();
    tokio::task::spawn_blocking(move || verify_password(&password, &hash).unwrap_or(false))
        .await
        .map_err(|e| AuthError::Internal(format!("Password check failed: {e}")))
}

/// Check a TOTP code against the account's encrypted MFA secret.
pub(crate) fn totp_matches(
    state: &AppState,
    encrypted_secret: &str,
    username: &str,
    code: &str,
) -> AuthResult<bool> {
    let encryption_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;
    let key_bytes = hex::decode(encryption_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;
    let secret_str = decrypt_mfa_secret(encrypted_secret, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to decrypt MFA secret: {e}")))?;
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        Secret::Encoded(secret_str)
            .to_bytes()
            .map_err(|_| AuthError::Internal("Invalid TOTP secret encoding".into()))?,
        Some("Kaiku".to_string()),
        username.to_string(),
    )
    .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;
    totp.check_current(code)
        .map_err(|e| AuthError::Internal(format!("Failed to verify TOTP code: {e}")))
}

/// Confirm the account again before a sensitive action (C24).
///
/// - `require_password`: the account password must be given and match; an
///   account without a local password gets `REAUTH_UNAVAILABLE`.
/// - When the account has MFA on, `mfa_code` must be a current TOTP code
///   (`MFA_CODE_REQUIRED` when missing). Backup codes are not accepted.
/// - A wrong password or code answers `INVALID_CREDENTIALS` without saying
///   which one was wrong, and is recorded as a failed attempt.
/// - While the account is locked after failures the answer is 429.
///
/// The missing-code check runs before any secret is verified, so that answer
/// says nothing about the password.
pub async fn verify_reauth(
    state: &AppState,
    user_id: Uuid,
    password: Option<&str>,
    mfa_code: Option<&str>,
    require_password: bool,
    client_ip: IpAddr,
) -> Result<(), AuthError> {
    ensure_not_locked(state, user_id).await?;

    let user = find_user_by_id(&state.db, user_id)
        .await
        .map_err(AuthError::Database)?
        .ok_or(AuthError::UserNotFound)?;

    let password = password.filter(|p| !p.is_empty());
    let mfa_code = mfa_code.map(str::trim).filter(|c| !c.is_empty());

    let password_check = if require_password {
        let hash = user
            .password_hash
            .as_deref()
            .ok_or(AuthError::ReauthUnavailable)?;
        let given = password
            .ok_or_else(|| AuthError::Validation("password: is required".to_string()))?;
        Some((given, hash))
    } else {
        None
    };

    let mfa_secret = user.mfa_secret.as_deref();
    if mfa_secret.is_some() && mfa_code.is_none() {
        return Err(AuthError::MfaCodeRequired);
    }
    if password_check.is_none() && mfa_secret.is_none() {
        // Nothing to confirm the account with.
        return Err(AuthError::ReauthUnavailable);
    }

    let mut confirmed = true;
    if let Some((given, hash)) = password_check {
        confirmed &= password_matches(given, hash).await?;
    }
    if let (Some(secret), Some(code)) = (mfa_secret, mfa_code) {
        confirmed &= totp_matches(state, secret, &user.username, code)?;
    }

    if confirmed {
        Ok(())
    } else {
        record_failure(state, user_id, client_ip).await?;
        tracing::info!(%user_id, "Re-authentication failed");
        Err(AuthError::InvalidCredentials)
    }
}
