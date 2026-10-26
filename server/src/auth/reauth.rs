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

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use chrono::Utc;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;
use validator::Validate;

use super::error::{AuthError, AuthResult};
use super::login::{oidc_hand_back, start_oidc_flow};
use super::mfa_crypto::decrypt_mfa_secret;
use super::middleware::AuthUser;
use super::oidc::{client_return_url, OidcFlowPurpose, OidcFlowState, OidcReauthBinding};
use super::password::verify_password;
use super::types::{OidcReauthRequest, ReauthMethodsResponse, ReauthOidcProvider};
use crate::api::AppState;
use crate::db::{find_user_by_id, find_user_id_by_identity, get_auth_methods_allowed};
use crate::ratelimit::{extract_client_ip, normalize_ip, RateLimitCategory};

/// Maximum age of the provider's `auth_time` for an OIDC re-authentication.
const OIDC_MAX_AUTH_AGE_SECS: i64 = 300;
/// Clock skew tolerated between this server and the provider.
const OIDC_CLOCK_SKEW_SECS: i64 = 30;

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

impl ReauthFactors {
    /// Audit label of the verified factors, prefixed by `base` when given
    /// (e.g. `oidc+totp`).
    pub fn method(self, base: Option<&str>) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(base) = base {
            parts.push(base);
        }
        if self.password {
            parts.push("password");
        }
        if self.totp {
            parts.push("totp");
        }
        parts.join("+")
    }
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

/// Linked providers that can confirm a fresh sign-in for this session.
///
/// A provider qualifies when it is enabled, uses OIDC discovery (so the code
/// exchange yields a verified ID token with `auth_time`), OIDC sign-in is
/// allowed on this server, and the identity is eligible for this session:
/// linked no later than the session was first authenticated, or linked from
/// an elevated session. A session without a live row has no providers.
pub async fn linked_reauth_providers(
    state: &AppState,
    user_id: Uuid,
    session_id: Option<Uuid>,
) -> AuthResult<Vec<ReauthOidcProvider>> {
    let Some(ref manager) = state.oidc_manager else {
        return Ok(Vec::new());
    };
    let Some(session_id) = session_id else {
        return Ok(Vec::new());
    };
    if !get_auth_methods_allowed(&state.db).await?.oidc {
        return Ok(Vec::new());
    }
    let slugs: Vec<String> = sqlx::query_scalar(
        r"SELECT i.provider_slug
          FROM user_identities i
          JOIN sessions s ON s.id = $2 AND s.user_id = i.user_id
          WHERE i.user_id = $1
            AND (i.created_at <= s.created_at OR i.linked_while_elevated)
          ORDER BY i.created_at",
    )
    .bind(user_id)
    .bind(session_id)
    .fetch_all(&state.db)
    .await?;

    let mut providers = Vec::new();
    for slug in slugs {
        let Some(row) = manager.get_provider_row(&slug).await else {
            continue;
        };
        if row.enabled && manager.issues_id_tokens(&row.slug).await {
            providers.push(ReauthOidcProvider {
                provider_id: row.slug,
                display_name: row.display_name,
            });
        }
    }
    Ok(providers)
}

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
    let user = find_user_by_id(&state.db, auth_user.id)
        .await
        .map_err(AuthError::Database)?
        .ok_or(AuthError::UserNotFound)?;
    Ok(Json(ReauthMethodsResponse {
        password: user.password_hash.is_some(),
        mfa_required: user.mfa_secret.is_some(),
        oidc: linked_reauth_providers(&state, user.id, auth_user.session_id).await?,
    }))
}

/// Start an OIDC re-authentication that elevates the caller's session.
///
/// The provider is asked for a fresh login (`prompt=login`, `max_age=0`). The
/// flow state (single use, 5-minute TTL) is bound to the caller, the access
/// token's session and the purpose `reauth`; the shared OIDC callback then
/// elevates that session only. When the account has MFA on, `mfa_code` (a
/// current TOTP code) is verified here, with the same lockout and replay
/// protection as the password path, and the result is bound into the state.
///
/// POST /auth/reauth/oidc/authorize/{provider}
#[utoipa::path(
    post,
    path = "/auth/reauth/oidc/authorize/{provider}",
    tag = "auth",
    params(
        ("provider" = String, Path, description = "Linked provider (`provider_id` from the methods list)"),
    ),
    request_body = OidcReauthRequest,
    responses(
        (status = 200, description = "Provider authorization URL: `{\"url\": string}`"),
        (status = 400, description = "MFA_CODE_REQUIRED, REAUTH_PROVIDER_UNAVAILABLE or VALIDATION_ERROR"),
        (status = 401, description = "INVALID_CREDENTIALS (wrong TOTP code)"),
        (status = 403, description = "Not a system admin"),
        (status = 429, description = "Too many attempts"),
        (status = 503, description = "SERVICE_UNAVAILABLE"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers, body), fields(provider = %provider, user_id = %auth_user.id))]
pub async fn oidc_reauth_authorize(
    State(state): State<AppState>,
    auth_user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    body: Result<Json<OidcReauthRequest>, JsonRejection>,
) -> Result<Response, AuthError> {
    let Json(body) = body.map_err(|e| AuthError::Validation(e.body_text()))?;
    body.validate()
        .map_err(|e| AuthError::Validation(crate::validation::format_validation_errors(&e)))?;
    let session_id = active_session(&state, &auth_user).await?;
    ensure_not_locked(&state, auth_user.id).await?;
    // Elevation is the only action this flow completes.
    if crate::permissions::queries::get_system_admin(&state.db, auth_user.id)
        .await?
        .is_none()
    {
        return Err(AuthError::Forbidden);
    }
    if !linked_reauth_providers(&state, auth_user.id, Some(session_id))
        .await?
        .iter()
        .any(|p| p.provider_id == provider)
    {
        return Err(AuthError::ReauthProviderUnavailable);
    }

    // MFA on: the TOTP code is part of this re-authentication.
    let mfa_verified = if auth_user.mfa_enabled || has_mfa(&state, auth_user.id).await? {
        let client_ip = client_ip(&state, &headers, addr);
        verify_reauth(
            &state,
            auth_user.id,
            None,
            body.mfa_code.as_deref(),
            false,
            client_ip,
        )
        .await?
        .totp
    } else {
        false
    };

    let binding = OidcReauthBinding {
        user_id: auth_user.id,
        session_id,
        reason: body.reason.clone(),
        mfa_verified,
    };
    start_oidc_flow(
        &state,
        &provider,
        body.redirect_uri.as_deref(),
        OidcFlowPurpose::Reauth(binding),
        // Always `{"url"}`: a bearer-authenticated call is never a navigation.
        true,
    )
    .await
}

/// Whether the account has MFA on (read from the DB, not the token).
async fn has_mfa(state: &AppState, user_id: Uuid) -> AuthResult<bool> {
    Ok(find_user_by_id(&state.db, user_id)
        .await
        .map_err(AuthError::Database)?
        .is_some_and(|u| u.mfa_secret.is_some()))
}

/// The access token's session, if it is a live session row of the user.
async fn active_session(state: &AppState, auth_user: &AuthUser) -> AuthResult<Uuid> {
    let session_id = auth_user
        .session_id
        .ok_or_else(|| AuthError::Validation("No active session found".to_string()))?;
    let active = crate::admin::queries::is_active_session(&state.db, auth_user.id, session_id)
        .await
        .map_err(|e| AuthError::Internal(format!("Session lookup failed: {e}")))?;
    if active {
        Ok(session_id)
    } else {
        Err(AuthError::Validation("No active session found".to_string()))
    }
}

/// Outcome codes of an OIDC re-authentication, handed back to the client.
mod reauth_code {
    pub const PROVIDER_ERROR: &str = "OIDC_EXCHANGE_FAILED";
    pub const AUTH_TIME_MISSING: &str = "REAUTH_AUTH_TIME_MISSING";
    pub const STALE: &str = "REAUTH_STALE";
    pub const IDENTITY_MISMATCH: &str = "REAUTH_IDENTITY_MISMATCH";
    pub const SESSION_INVALID: &str = "REAUTH_SESSION_INVALID";
    pub const NOT_ADMIN: &str = "REAUTH_NOT_ALLOWED";
    pub const IDENTITY_NOT_ELIGIBLE: &str = "REAUTH_IDENTITY_NOT_ELIGIBLE";
    pub const MFA_REQUIRED: &str = "MFA_CODE_REQUIRED";
    pub const FAILED: &str = "REAUTH_FAILED";
}

/// Finish an OIDC re-authentication (called by the shared OIDC callback after
/// the single-use flow state was taken and decrypted).
///
/// Checks, in order: the purpose; the ID token (signature, issuer, audience,
/// expiry and nonce, during the code exchange); that it has `auth_time`
/// (no fallback); that `auth_time` is at most 5 minutes old (30 s skew);
/// that (provider, `sub`) is linked to the user who started the flow; that the
/// bound session is still live; that the user is still a system admin. Only
/// then is that session elevated, with the same record as the password path.
///
/// The result is handed back like the identity-link flow: a localhost
/// redirect with `reauth=elevated&expires_at=...` or `reauth_error=CODE`
/// (desktop), or a `postMessage` of `{"type": "oidc-reauth-callback",
/// "success", "error_code", "expires_at"}` (browser).
pub async fn complete_oidc_reauth(
    state: &AppState,
    flow_state: &OidcFlowState,
    binding: &OidcReauthBinding,
    code: &str,
    client_ip: IpAddr,
) -> Result<Response, AuthError> {
    let outcome = match check_oidc_reauth(state, flow_state, binding, code, client_ip).await {
        Ok(outcome) => outcome,
        // A server fault still completes the hand-back, with a generic code.
        Err(e) => {
            tracing::error!(error = %e, user_id = %binding.user_id, provider = %flow_state.slug, "OIDC re-authentication failed with a server error");
            Err(reauth_code::FAILED)
        }
    };
    match &outcome {
        Ok(_) => {
            tracing::info!(user_id = %binding.user_id, provider = %flow_state.slug, "Session elevated after OIDC re-authentication");
        }
        Err(code) => {
            tracing::info!(user_id = %binding.user_id, provider = %flow_state.slug, error_code = code, "OIDC re-authentication refused");
        }
    }

    let (query, payload) = match outcome {
        Ok(expires_at) => (
            vec![
                ("reauth", "elevated".to_string()),
                ("expires_at", expires_at.to_rfc3339()),
            ],
            serde_json::json!({
                "type": "oidc-reauth-callback",
                "success": true,
                "error_code": null,
                "expires_at": expires_at,
            }),
        ),
        Err(code) => return Ok(reauth_failure_response(flow_state, code)),
    };
    Ok(oidc_hand_back(
        client_return_url(flow_state),
        &query,
        &payload,
    ))
}

/// Hand a refused re-authentication back to its origin with a generic error
/// code: desktop `?reauth_error=CODE`, browser an `oidc-reauth-callback`
/// `postMessage` with `success: false`.
pub(super) fn reauth_failure_response(flow_state: &OidcFlowState, code: &str) -> Response {
    oidc_hand_back(
        client_return_url(flow_state),
        &[("reauth_error", code.to_string())],
        &serde_json::json!({
            "type": "oidc-reauth-callback",
            "success": false,
            "error_code": code,
            "expires_at": null,
        }),
    )
}

/// The checks of [`complete_oidc_reauth`]: `Ok(Ok(expires_at))` when the
/// session was elevated, `Ok(Err(code))` when refused, `Err` on server faults.
async fn check_oidc_reauth(
    state: &AppState,
    flow_state: &OidcFlowState,
    binding: &OidcReauthBinding,
    code: &str,
    client_ip: IpAddr,
) -> AuthResult<Result<chrono::DateTime<Utc>, &'static str>> {
    let manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    let exchanged = manager
        .exchange_code_verified(
            &flow_state.slug,
            code,
            &flow_state.pkce_verifier,
            &flow_state.redirect_uri,
            &flow_state.nonce,
        )
        .await;
    let claims = match exchanged {
        Ok((_, Some((_, claims)))) => claims,
        // A provider without an ID token cannot prove a fresh sign-in.
        Ok((_, None)) => return Ok(Err(reauth_code::AUTH_TIME_MISSING)),
        Err(e) => {
            tracing::warn!(error = %e, provider = %flow_state.slug, "OIDC re-authentication exchange failed");
            return Ok(Err(reauth_code::PROVIDER_ERROR));
        }
    };

    let Some(auth_time) = claims.auth_time else {
        return Ok(Err(reauth_code::AUTH_TIME_MISSING));
    };
    let age = (Utc::now() - auth_time).num_seconds();
    if !(-OIDC_CLOCK_SKEW_SECS..=OIDC_MAX_AUTH_AGE_SECS + OIDC_CLOCK_SKEW_SECS).contains(&age) {
        return Ok(Err(reauth_code::STALE));
    }

    let linked_user =
        find_user_id_by_identity(&state.db, &flow_state.slug, &claims.subject).await?;
    if linked_user != Some(binding.user_id) {
        return Ok(Err(reauth_code::IDENTITY_MISMATCH));
    }

    let session_live =
        crate::admin::queries::is_active_session(&state.db, binding.user_id, binding.session_id)
            .await
            .map_err(|e| AuthError::Internal(format!("Session lookup failed: {e}")))?;
    // Fails closed: an unreachable denylist is a server fault.
    let revoked =
        super::revocation::is_session_revoked(&state.redis, &binding.session_id.to_string())
            .await?;
    if !session_live || revoked {
        return Ok(Err(reauth_code::SESSION_INVALID));
    }

    // The identity must have been linked before this session began, or from
    // an elevated session (an identity linked later cannot unlock it).
    if !identity_eligible(state, binding, &flow_state.slug, &claims.subject).await? {
        return Ok(Err(reauth_code::IDENTITY_NOT_ELIGIBLE));
    }

    if crate::permissions::queries::get_system_admin(&state.db, binding.user_id)
        .await?
        .is_none()
    {
        return Ok(Err(reauth_code::NOT_ADMIN));
    }

    // MFA on: the TOTP code verified at the start is required.
    if has_mfa(state, binding.user_id).await? && !binding.mfa_verified {
        return Ok(Err(reauth_code::MFA_REQUIRED));
    }

    let factors = ReauthFactors {
        password: false,
        totp: binding.mfa_verified,
    };
    let elevated = crate::admin::system::record_elevation(
        state,
        binding.user_id,
        binding.session_id,
        &client_ip.to_string(),
        binding.reason.as_deref(),
        &factors.method(Some("oidc")),
    )
    .await
    .map_err(|e| AuthError::Internal(format!("Elevation failed: {e}")))?;
    Ok(Ok(elevated.expires_at))
}

/// Whether the (provider, `sub`) identity of the user may confirm the bound
/// session (see [`linked_reauth_providers`]).
async fn identity_eligible(
    state: &AppState,
    binding: &OidcReauthBinding,
    slug: &str,
    subject: &str,
) -> AuthResult<bool> {
    let eligible: bool = sqlx::query_scalar(
        r"SELECT EXISTS(
            SELECT 1 FROM user_identities i
            JOIN sessions s ON s.id = $4 AND s.user_id = i.user_id
            WHERE i.provider_slug = $1 AND i.subject = $2 AND i.user_id = $3
              AND (i.created_at <= s.created_at OR i.linked_while_elevated)
        )",
    )
    .bind(slug)
    .bind(subject)
    .bind(binding.user_id)
    .bind(binding.session_id)
    .fetch_one(&state.db)
    .await?;
    Ok(eligible)
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
