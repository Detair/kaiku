//! Re-authentication for sensitive actions (C24).
//!
//! Admin elevation and backup-code regeneration ask the caller to prove the
//! account again: the password and, when MFA is on, a current TOTP code
//! (backup codes are not accepted). A password change checks the current
//! password with the same helpers.
//!
//! Admin elevation can instead be confirmed with a fresh sign-in at a linked
//! OIDC provider (`GET /auth/reauth/oidc/authorize/{provider}`); the shared
//! OIDC callback hands that flow to [`complete_oidc_reauth`].
//! `GET /auth/reauth/methods` reports which methods the caller has.
//!
//! Every failure is recorded twice with `RateLimiter::record_failed_auth`:
//! - per client IP, which the `check_ip_not_blocked` layer on these routes (and on `/auth/login`)
//!   enforces;
//! - per account (`reauth:{user_id}`), enforced here, so failures spread over several addresses
//!   still lock the account's re-authentication.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use chrono::Utc;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::login::{oidc_hand_back, start_oidc_flow, OidcFlowPurpose};
use super::mfa_crypto::decrypt_mfa_secret;
use super::middleware::AuthUser;
use super::oidc::{client_return_url, OidcFlowState, OidcReauthBinding, OIDC_PURPOSE_REAUTH};
use super::password::verify_password;
use super::types::{OidcReauthQuery, ReauthMethodsResponse, ReauthOidcProvider};
use crate::api::AppState;
use crate::db::{find_user_by_id, find_user_id_by_identity, get_auth_methods_allowed};
use crate::ratelimit::{extract_client_ip, normalize_ip};

/// Maximum age of the provider's `auth_time` for an OIDC re-authentication.
const OIDC_MAX_AUTH_AGE_SECS: i64 = 300;
/// Clock skew tolerated between this server and the provider.
const OIDC_CLOCK_SKEW_SECS: i64 = 30;

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
        // Fails closed, like recording a failure.
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
pub async fn record_failure(state: &AppState, user_id: Uuid, client_ip: IpAddr) -> AuthResult<()> {
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
pub async fn password_matches(password: &str, hash: &str) -> AuthResult<bool> {
    let password = password.to_owned();
    let hash = hash.to_owned();
    tokio::task::spawn_blocking(move || verify_password(&password, &hash).unwrap_or(false))
        .await
        .map_err(|e| AuthError::Internal(format!("Password check failed: {e}")))
}

/// Check a TOTP code against the account's encrypted MFA secret.
pub fn totp_matches(
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
/// - `require_password`: the account password must be given and match; an account without a local
///   password gets `REAUTH_UNAVAILABLE`.
/// - When the account has MFA on, `mfa_code` must be a current TOTP code (`MFA_CODE_REQUIRED` when
///   missing). Backup codes are not accepted.
/// - A wrong password or code answers `INVALID_CREDENTIALS` without saying which one was wrong, and
///   is recorded as a failed attempt.
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
        let Some(hash) = user.password_hash.as_deref() else {
            // No password: point to a linked provider when there is one.
            return Err(
                if linked_reauth_providers(state, user_id).await?.is_empty() {
                    AuthError::ReauthUnavailable
                } else {
                    AuthError::ReauthOidcRequired
                },
            );
        };
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

// ============================================================================
// Methods and OIDC re-authentication
// ============================================================================

/// Linked providers that can confirm a fresh sign-in for this user: enabled,
/// using OIDC discovery (so the code exchange yields a verified ID token with
/// `auth_time`), and OIDC sign-in allowed on this server.
async fn linked_reauth_providers(
    state: &AppState,
    user_id: Uuid,
) -> AuthResult<Vec<ReauthOidcProvider>> {
    let Some(ref manager) = state.oidc_manager else {
        return Ok(Vec::new());
    };
    if !get_auth_methods_allowed(&state.db).await?.oidc {
        return Ok(Vec::new());
    }
    let mut providers = Vec::new();
    for identity in crate::db::list_user_identities(&state.db, user_id).await? {
        let Some(row) = manager.get_provider_row(&identity.provider_slug).await else {
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
        oidc: linked_reauth_providers(&state, user.id).await?,
    }))
}

/// Start an OIDC re-authentication that elevates the caller's session.
///
/// The provider is asked for a fresh login (`prompt=login`, `max_age=0`). The
/// flow state (single use, 5-minute TTL) is bound to the caller, the access
/// token's session and the purpose `reauth`; the shared OIDC callback then
/// elevates that session only.
///
/// GET /auth/reauth/oidc/authorize/{provider}
#[utoipa::path(
    get,
    path = "/auth/reauth/oidc/authorize/{provider}",
    tag = "auth",
    params(
        ("provider" = String, Path, description = "Linked provider (`provider_id` from the methods list)"),
        ("redirect_uri" = Option<String>, Query, description = "Optional localhost redirect (desktop)"),
        ("response" = Option<String>, Query, description = "`json` returns {\"url\": ...} instead of a redirect"),
        ("reason" = Option<String>, Query, description = "Elevation reason for the audit log"),
    ),
    responses(
        (status = 307, description = "Redirect to the provider"),
        (status = 400, description = "REAUTH_PROVIDER_UNAVAILABLE or VALIDATION_ERROR"),
        (status = 403, description = "Not a system admin"),
        (status = 429, description = "Too many attempts"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, query), fields(provider = %provider, user_id = %auth_user.id))]
pub async fn oidc_reauth_authorize(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Path(provider): Path<String>,
    axum::extract::Query(query): axum::extract::Query<OidcReauthQuery>,
) -> Result<Response, AuthError> {
    if query
        .reason
        .as_deref()
        .is_some_and(|r| r.chars().count() > 255)
    {
        return Err(AuthError::Validation(
            "reason: at most 255 characters".to_string(),
        ));
    }
    let session_id = active_session(&state, &auth_user).await?;
    ensure_not_locked(&state, auth_user.id).await?;
    // Elevation is the only action this flow completes.
    if crate::permissions::queries::get_system_admin(&state.db, auth_user.id)
        .await?
        .is_none()
    {
        return Err(AuthError::Forbidden);
    }
    if !linked_reauth_providers(&state, auth_user.id)
        .await?
        .iter()
        .any(|p| p.provider_id == provider)
    {
        return Err(AuthError::ReauthProviderUnavailable);
    }

    let binding = OidcReauthBinding {
        purpose: OIDC_PURPOSE_REAUTH.to_string(),
        user_id: auth_user.id,
        session_id,
        reason: query.reason.clone(),
    };
    start_oidc_flow(
        &state,
        &provider,
        query.redirect_uri.as_deref(),
        OidcFlowPurpose::Reauth(binding),
        query.response.as_deref() == Some("json"),
    )
    .await
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
    pub const PURPOSE_MISMATCH: &str = "OIDC_STATE_MISMATCH";
    pub const PROVIDER_ERROR: &str = "OIDC_EXCHANGE_FAILED";
    pub const AUTH_TIME_MISSING: &str = "REAUTH_AUTH_TIME_MISSING";
    pub const STALE: &str = "REAUTH_STALE";
    pub const IDENTITY_MISMATCH: &str = "REAUTH_IDENTITY_MISMATCH";
    pub const SESSION_INVALID: &str = "REAUTH_SESSION_INVALID";
    pub const NOT_ADMIN: &str = "REAUTH_NOT_ALLOWED";
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
    let outcome = check_oidc_reauth(state, flow_state, binding, code, client_ip).await?;
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
    if binding.purpose != OIDC_PURPOSE_REAUTH {
        return Ok(Err(reauth_code::PURPOSE_MISMATCH));
    }
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
    // Fails closed: an unreachable denylist is a server fault (503).
    let revoked =
        super::revocation::is_session_revoked(&state.redis, &binding.session_id.to_string())
            .await?;
    if !session_live || revoked {
        return Ok(Err(reauth_code::SESSION_INVALID));
    }

    if crate::permissions::queries::get_system_admin(&state.db, binding.user_id)
        .await?
        .is_none()
    {
        return Ok(Err(reauth_code::NOT_ADMIN));
    }

    let elevated = crate::admin::system::record_elevation(
        state,
        binding.user_id,
        binding.session_id,
        &client_ip.to_string(),
        binding.reason.as_deref(),
        "oidc",
    )
    .await
    .map_err(|e| AuthError::Internal(format!("Elevation failed: {e}")))?;
    Ok(Ok(elevated.expires_at))
}
