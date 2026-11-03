//! Re-authentication for sensitive actions (C24).
//!
//! Admin elevation and backup-code regeneration ask the caller to prove the
//! account again: the password and, when MFA is on, a current TOTP code
//! (backup codes are not accepted). A password change checks the current
//! password with the same helpers.
//!
//! Admin elevation can instead be confirmed with a fresh sign-in at a linked
//! OIDC provider (`POST /auth/reauth/oidc/authorize/{provider}`); the shared
//! OIDC callback hands that flow to [`complete_oidc_reauth`].
//! `GET /auth/reauth/methods` reports which methods the caller has.
//!
//! Every failure is recorded twice with `RateLimiter::record_failed_auth`:
//! - per client IP, which the `check_ip_not_blocked` layer on these routes (and on `/auth/login`)
//!   enforces;
//! - per account (`reauth:{user_id}`), enforced here, so failures spread over several addresses
//!   still lock the account's re-authentication.

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
use super::password::verify_password;
use super::types::ReauthMethodsResponse;
use crate::api::AppState;
use crate::db::find_user_by_id;
use crate::ratelimit::{extract_client_ip, normalize_ip, RateLimitCategory};

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
pub async fn ensure_not_locked(state: &AppState, user_id: Uuid) -> AuthResult<()> {
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
                .unwrap_or_else(|| limiter.config().limits.failed_auth.block_duration_secs);
            Err(AuthError::TooManyAttempts { retry_after })
        }
        // Fails closed (503), like recording a failure and the other auth
        // limits (unless the operator set `RATE_LIMIT_FAIL_OPEN=true`).
        Err(e) => {
            tracing::debug!(error = %e, %user_id, "Cannot check re-authentication lock");
            if limiter.allow_during_outage(RateLimitCategory::FailedAuth) {
                Ok(())
            } else {
                Err(AuthError::ServiceUnavailable)
            }
        }
    }
}

/// Record a failed re-authentication for the client IP and the account.
///
/// Fails closed: if the failure cannot be recorded the request is refused
/// with 503 `SERVICE_UNAVAILABLE`, so a limiter outage cannot be used to
/// guess without limit (unless the operator set `RATE_LIMIT_FAIL_OPEN=true`).
pub async fn record_failure(state: &AppState, user_id: Uuid, client_ip: IpAddr) -> AuthResult<()> {
    let Some(ref limiter) = state.rate_limiter else {
        return Ok(());
    };
    for key in [normalize_ip(client_ip), account_key(user_id)] {
        if let Err(e) = limiter.record_failed_auth(&key).await {
            tracing::debug!(error = %e, %user_id, "Cannot record failed re-authentication");
            if !limiter.allow_during_outage(RateLimitCategory::FailedAuth) {
                return Err(AuthError::ServiceUnavailable);
            }
        }
    }
    Ok(())
}

/// Check a password against an Argon2 hash off the async runtime.
///
/// A malformed stored hash counts as a mismatch.
pub async fn password_matches(password: &str, hash: &str) -> AuthResult<bool> {
    let password = password.to_owned();
    let hash = hash.to_owned();
    tokio::task::spawn_blocking(move || verify_password(&password, &hash).unwrap_or(false))
        .await
        .map_err(|e| AuthError::Internal(format!("Password check failed: {e}")))
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

/// The factors a successful [`verify_reauth`] actually checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReauthFactors {
    /// The password was verified.
    pub password: bool,
    /// A TOTP code was verified.
    pub totp: bool,
}

/// Confirm the account again before a sensitive action (C24).
///
/// - `require_password`: the account password must be given and match; an account without a local
///   password gets `REAUTH_UNAVAILABLE` (the elevate handler turns that into `REAUTH_OIDC_REQUIRED`
///   when a usable linked provider exists).
/// - When the account has MFA on, `mfa_code` must be a current TOTP code (`MFA_CODE_REQUIRED` when
///   missing). Backup codes are not accepted, and a code is accepted only once.
/// - A wrong password or code answers `INVALID_CREDENTIALS` without saying which one was wrong, and
///   is recorded as a failed attempt.
/// - While the account is locked after failures the answer is 429.
///
/// The missing-code check runs before any secret is verified, so that answer
/// says nothing about the password. Returns the factors that were verified.
pub async fn verify_reauth(
    state: &AppState,
    user_id: Uuid,
    password: Option<&str>,
    mfa_code: Option<&str>,
    require_password: bool,
    client_ip: IpAddr,
) -> Result<ReauthFactors, AuthError> {
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
        let given =
            password.ok_or_else(|| AuthError::Validation("password: is required".to_string()))?;
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
    let mut factors = ReauthFactors {
        password: false,
        totp: false,
    };
    if let Some((given, hash)) = password_check {
        let ok = password_matches(given, hash).await?;
        confirmed &= ok;
        factors.password = ok;
    }
    // The TOTP step is only consumed when the password was right.
    if let (Some(secret), Some(code)) = (mfa_secret, mfa_code) {
        let ok = confirmed && accept_totp(state, user_id, secret, &user.username, code).await?;
        confirmed &= ok;
        factors.totp = ok;
    }

    if confirmed {
        Ok(factors)
    } else {
        record_failure(state, user_id, client_ip).await?;
        tracing::info!(%user_id, "Re-authentication failed");
        Err(AuthError::InvalidCredentials)
    }
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
    // `device` means an active key device (not any strong authenticator: a
    // passkey-only account has no device step-up).
    let device: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE user_id = $1 AND state = 'active')",
    )
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;
    let passkey = super::passkeys::queries::active_count(&state.db, auth_user.id).await? > 0;
    Ok(Json(ReauthMethodsResponse { passkey, device }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::time::Duration;

    use axum::response::IntoResponse;
    use fred::prelude::{ClientLike, Config as ValkeyConfig, PerformanceConfig};
    use sqlx::PgPool;

    use super::*;
    use crate::api::AppStateConfig;
    use crate::config::Config;
    use crate::ratelimit::{RateLimitConfig, RateLimiter, RateLimits, TrustedProxies};

    /// App state whose rate limiter talks to a Valkey client that fails every
    /// command (connected, then closed; a short command timeout turns the wait
    /// into an error).
    async fn state_with_failing_limiter(pool: PgPool, fail_open: bool) -> AppState {
        let config = Config::default_for_test();
        let redis = crate::db::create_redis_client(&config.redis_url)
            .await
            .expect("Valkey");
        let failing = fred::clients::Client::new(
            ValkeyConfig::from_url(&config.redis_url).expect("Valkey URL"),
            Some(PerformanceConfig {
                default_command_timeout: Duration::from_millis(300),
                ..PerformanceConfig::default()
            }),
            None,
            None,
        );
        failing.connect();
        failing.wait_for_connect().await.expect("connect Valkey");
        failing.quit().await.expect("quit Valkey");
        let limiter = RateLimiter::new(
            failing,
            RateLimitConfig {
                enabled: true,
                redis_key_prefix: format!("test:rl:{}", Uuid::new_v4()),
                fail_open,
                trusted_proxies: TrustedProxies::default(),
                allowlist: HashSet::new(),
                limits: RateLimits::default(),
            },
        );
        AppState::new(AppStateConfig {
            db: pool,
            redis,
            config: config.clone(),
            s3: None,
            sfu: crate::voice::SfuServer::new(std::sync::Arc::new(config), None)
                .expect("SfuServer"),
            rate_limiter: Some(limiter),
            screen_share_limiter: None,
            email: None,
            oidc_manager: None,
            http_client: reqwest::Client::new(),
        })
    }

    fn status_of(err: AuthError) -> u16 {
        err.into_response().status().as_u16()
    }

    /// Re-auth lock checks and failure recording fail closed with 503
    /// `SERVICE_UNAVAILABLE` (not 500) when the limiter cannot reach Valkey.
    #[sqlx::test]
    async fn reauth_limiter_outage_answers_503(pool: PgPool) {
        let state = state_with_failing_limiter(pool, false).await;
        let user_id = Uuid::new_v4();
        let ip: IpAddr = "203.0.113.7".parse().expect("ip");

        let err = ensure_not_locked(&state, user_id)
            .await
            .expect_err("lock check fails closed");
        assert!(matches!(err, AuthError::ServiceUnavailable), "{err:?}");
        assert_eq!(status_of(err), 503);

        let err = record_failure(&state, user_id, ip)
            .await
            .expect_err("failure recording fails closed");
        assert!(matches!(err, AuthError::ServiceUnavailable), "{err:?}");
        assert_eq!(status_of(err), 503);
    }

    /// `RATE_LIMIT_FAIL_OPEN=true` (explicit operator opt-in) also opens the
    /// re-auth limiter during an outage, like the other auth limits.
    #[sqlx::test]
    async fn reauth_limiter_outage_respects_fail_open(pool: PgPool) {
        let state = state_with_failing_limiter(pool, true).await;
        let user_id = Uuid::new_v4();
        let ip: IpAddr = "203.0.113.7".parse().expect("ip");

        ensure_not_locked(&state, user_id)
            .await
            .expect("fail-open lets the check pass");
        record_failure(&state, user_id, ip)
            .await
            .expect("fail-open lets the request continue");
    }
}
