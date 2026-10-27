//! Login, logout, refresh, password reset, and OIDC handlers.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Json};
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use fred::prelude::*;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::backup_codes::find_matching_backup_code;
use super::error::{AuthError, AuthResult};
use super::helpers::{extract_refresh_token, extract_user_agent, should_return_refresh_token};
use super::jwt::{generate_token_pair, generate_token_pair_for_session, validate_refresh_token};
use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use super::middleware::AuthUser;
use super::oidc::{
    append_collision_suffix, client_challenge_matches, client_return_url,
    generate_username_from_claims, handoff_key, is_valid_client_challenge, new_handoff_code,
    resolve_flow_redirects, FlowRedirectError, OidcFlowPurpose, OidcFlowState, OidcHandoffGrant,
    OidcUserInfo, OIDC_HANDOFF_TTL_SECS,
};
use super::password::{hash_password, verify_password};
use super::queries::{self, InsertSessionParams};
use super::types::{
    AuthResponse, ForgotPasswordRequest, LoginRequest, LogoutRequest, OidcAuthorizeQuery,
    OidcCallbackQuery, OidcHandoffExchangeRequest, RefreshRequest, ResetPasswordRequest,
};
use super::{cookies, geoip, hash_token};
use crate::api::AppState;
use crate::db::{
    self, create_password_reset_token, create_session, find_session_by_token_hash,
    find_user_by_email, find_user_by_id, find_user_by_username, find_user_id_by_identity,
    find_valid_reset_token, get_auth_methods_allowed, get_unused_mfa_backup_codes,
    invalidate_user_reset_tokens, is_setup_complete, mark_mfa_backup_code_used,
};
use crate::ratelimit::NormalizedIp;

/// Login with username/password.
///
/// POST /auth/login
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful", body = AuthResponse),
        (status = 401, description = "Invalid credentials"),
        (status = 403, description = "MFA verification required or auth method disabled"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, jar, headers, body, normalized_ip), fields(username = %body.username))]
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    normalized_ip: Option<Extension<NormalizedIp>>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    // Helper macro to record failed auth (if rate limiter is configured)
    // SECURITY: Fails request with 503 if Valkey is down (fail-closed pattern)
    macro_rules! record_failed_auth {
        () => {
            if let (Some(ref rl), Some(Extension(ref nip))) = (&state.rate_limiter, &normalized_ip)
            {
                if rl.record_failed_auth(&nip.0).await.is_err()
                    && !rl.allow_during_outage(crate::ratelimit::RateLimitCategory::FailedAuth)
                {
                    // Fail closed (503, logged and counted by
                    // `allow_during_outage`): otherwise a rate limiter failure
                    // would let attackers bypass failed-login blocking.
                    return Err(AuthError::ServiceUnavailable);
                }
            }
        };
    }

    // Look up the user, then ALWAYS run a password verification — against the
    // real hash if the account exists with a local password, otherwise against
    // a dummy hash of identical Argon2 cost. This keeps the response time the
    // same whether the username exists or not, closing the timing side-channel
    // that would otherwise reveal valid usernames (enumeration).
    let user_opt = find_user_by_username(&state.db, &body.username).await?;
    let stored_hash = user_opt
        .as_ref()
        .and_then(|u| u.password_hash.as_deref())
        .unwrap_or_else(|| super::password::DUMMY_PASSWORD_HASH.as_str());
    // A malformed stored hash (verify error) is treated as a non-match rather
    // than a distinct 500, so it cannot be used as an oracle either.
    let password_valid = verify_password(&body.password, stored_hash).unwrap_or(false);

    let user = match user_opt {
        // Local account whose password matched.
        Some(u) if u.password_hash.is_some() && password_valid => u,
        // Missing user, non-local (OIDC) account, or wrong password — all
        // indistinguishable to the caller.
        _ => {
            record_failed_auth!();
            crate::observability::metrics::record_auth_login_attempt(false);
            return Err(AuthError::InvalidCredentials);
        }
    };

    // A banned account cannot start a new session.
    if queries::is_user_globally_banned(&state.db, user.id).await? {
        crate::observability::metrics::record_auth_login_attempt(false);
        return Err(AuthError::AccountBanned);
    }

    // Check MFA if enabled
    if let Some(ref encrypted_secret) = user.mfa_secret {
        // MFA is enabled - code is required
        let mfa_code = body.mfa_code.as_ref().ok_or(AuthError::MfaRequired)?;

        // Try the TOTP code first. It is accepted once per 30-second step
        // (the same per-user replay protection as the re-authentication
        // paths) and only after the password matched; a Valkey error fails
        // closed with 503.
        let totp_valid =
            super::reauth::accept_totp(&state, user.id, encrypted_secret, &user.username, mfa_code)
                .await?;

        if !totp_valid {
            // TOTP failed — try backup code
            let backup_codes = get_unused_mfa_backup_codes(&state.db, user.id)
                .await
                .map_err(AuthError::Database)?;

            let hashes: Vec<String> = backup_codes.iter().map(|c| c.code_hash.clone()).collect();
            if let Some(matched_idx) = find_matching_backup_code(mfa_code, &hashes) {
                // Mark backup code as used
                let used_code_id = backup_codes[matched_idx].id;
                mark_mfa_backup_code_used(&state.db, used_code_id)
                    .await
                    .map_err(AuthError::Database)?;
                tracing::info!(
                    user_id = %user.id,
                    code_id = %used_code_id,
                    "MFA backup code used for login"
                );
            } else {
                record_failed_auth!();
                crate::observability::metrics::record_auth_login_attempt(false);
                return Err(AuthError::InvalidMfaCode);
            }
        }
    }

    // Generate tokens
    let tokens = generate_token_pair(
        user.id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;

    // Store refresh token session
    let token_hash = hash_token(&tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    let user_agent = extract_user_agent(&headers);

    let geo =
        geoip::resolve_location(&state.http_client, &state.config.geoip_api_url, &addr.ip()).await;
    let city = geo.as_ref().and_then(|g| g.city.as_deref());
    let country = geo.as_ref().and_then(|g| g.country.as_deref());

    create_session(
        &state.db,
        tokens.refresh_token_id,
        user.id,
        &token_hash,
        expires_at,
        Some(&addr.ip().to_string()),
        user_agent.as_deref(),
        city,
        country,
    )
    .await?;

    // Clear failed auth counter on successful login
    if let (Some(ref rl), Some(Extension(ref nip))) = (&state.rate_limiter, &normalized_ip) {
        let _ = rl.clear_failed_auth(&nip.0).await;
    }

    // Check if setup is complete
    let setup_complete = is_setup_complete(&state.db).await?;

    tracing::info!(user_id = %user.id, setup_required = !setup_complete, "User logged in");
    crate::observability::metrics::record_auth_login_attempt(true);

    let include_refresh_token = should_return_refresh_token(&headers);

    let jar = jar.add(cookies::build_refresh_cookie(
        &tokens.refresh_token,
        state.config.jwt_refresh_expiry,
        &state.config,
    ));

    Ok((
        jar,
        Json(AuthResponse {
            access_token: tokens.access_token,
            refresh_token: include_refresh_token.then_some(tokens.refresh_token),
            expires_in: tokens.access_expires_in,
            token_type: "Bearer".to_string(),
            setup_required: !setup_complete,
            password_change_required: user.must_change_password,
        }),
    ))
}

/// Refresh access token using refresh token.
///
/// POST /auth/refresh
///
/// Uses a database transaction with `FOR UPDATE` row locking to prevent
/// race conditions where concurrent refresh requests could both succeed,
/// defeating token rotation security.
#[utoipa::path(
    post,
    path = "/auth/refresh",
    tag = "auth",
    request_body(content = RefreshRequest, description = "Required for Tauri clients; browser clients use HttpOnly cookie instead"),
    responses(
        (status = 200, description = "Token refreshed successfully", body = AuthResponse),
        (status = 401, description = "Invalid or expired token"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, jar, headers, body))]
pub async fn refresh_token(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    body: Option<Json<RefreshRequest>>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    // Read refresh token from JSON body (Tauri) or HttpOnly cookie (browser)
    let raw_token = extract_refresh_token(body.map(|b| b.0.refresh_token), &jar)?;

    // Validate the refresh token (JWT validation)
    let claims = validate_refresh_token(&raw_token, &state.config.jwt_public_key)?;

    // Parse user ID
    let user_id: Uuid = claims.sub.parse().map_err(|_| AuthError::InvalidToken)?;

    // A banned account cannot renew its session (its sessions were ended
    // when the ban was issued; this answers with the reason).
    if queries::is_user_globally_banned(&state.db, user_id).await? {
        crate::observability::metrics::record_token_refresh(false);
        return Err(AuthError::AccountBanned);
    }

    let token_hash = hash_token(&raw_token);

    // Resolve GeoIP before the transaction to avoid holding a connection during HTTP call
    let user_agent = extract_user_agent(&headers);
    let geo =
        geoip::resolve_location(&state.http_client, &state.config.geoip_api_url, &addr.ip()).await;

    // Wrap session lookup, deletion, and creation in a transaction with FOR UPDATE
    // to prevent race conditions in token rotation.
    let mut tx = state.db.begin().await?;

    // Lock the session row to prevent concurrent refresh
    let session = queries::lock_session_by_token_hash_tx(&mut tx, &token_hash).await?;

    let Some(session) = session else {
        crate::observability::metrics::record_token_refresh(false);
        return Err(AuthError::InvalidToken);
    };

    // Verify session belongs to the user in the token
    if session.user_id != user_id {
        crate::observability::metrics::record_token_refresh(false);
        return Err(AuthError::InvalidToken);
    }

    // A session whose id is denylisted has ended even if its row survived
    // (e.g. ending it in Valkey succeeded but a later step failed): refuse,
    // since every access token issued for it would be refused anyway. Fails
    // closed (503) when Valkey cannot answer, like the access-token check.
    if super::revocation::is_session_revoked(&state.redis, &session.id.to_string()).await? {
        crate::observability::metrics::record_token_refresh(false);
        return Err(AuthError::InvalidToken);
    }

    // Verify user still exists
    let user = find_user_by_id(&state.db, user_id)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    // Generate the new token pair for the same session: its id (the access
    // token's `sid`) stays stable, so revoking the session later also reaches
    // sockets opened before this refresh.
    let new_tokens = generate_token_pair_for_session(
        user_id,
        session.id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;

    // Rotate the refresh token in place within the transaction
    let new_token_hash = hash_token(&new_tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    let city = geo.as_ref().and_then(|g| g.city.as_deref());
    let country = geo.as_ref().and_then(|g| g.country.as_deref());

    let ip_str = addr.ip().to_string();
    queries::rotate_session_tx(
        &mut tx,
        session.id,
        &InsertSessionParams {
            id: session.id,
            user_id,
            token_hash: &new_token_hash,
            expires_at,
            ip_address: Some(&ip_str),
            user_agent: user_agent.as_deref(),
            city,
            country,
        },
    )
    .await?;

    // Commit the transaction — this is the atomic point
    tx.commit().await?;

    // Check if setup is complete
    let setup_complete = is_setup_complete(&state.db).await?;

    tracing::info!(user_id = %user_id, "Token refreshed");
    crate::observability::metrics::record_token_refresh(true);

    let include_refresh_token = should_return_refresh_token(&headers);

    let jar = jar.add(cookies::build_refresh_cookie(
        &new_tokens.refresh_token,
        state.config.jwt_refresh_expiry,
        &state.config,
    ));

    Ok((
        jar,
        Json(AuthResponse {
            access_token: new_tokens.access_token,
            refresh_token: include_refresh_token.then_some(new_tokens.refresh_token),
            expires_in: new_tokens.access_expires_in,
            token_type: "Bearer".to_string(),
            setup_required: !setup_complete,
            password_change_required: user.must_change_password,
        }),
    ))
}

/// Logout and invalidate session.
///
/// POST /auth/logout
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    request_body(content = LogoutRequest, description = "Optional; without a refresh token (body or HttpOnly cookie) the bearer token's session is ended"),
    responses(
        (status = 200, description = "Logged out successfully"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, jar, body), fields(user_id = %auth_user.id))]
pub async fn logout(
    State(state): State<AppState>,
    auth_user: AuthUser,
    jar: CookieJar,
    body: Option<Json<LogoutRequest>>,
) -> AuthResult<CookieJar> {
    // The session to end: the access token's own session, plus the one named
    // by a refresh token (JSON body for Tauri/Android, HttpOnly cookie for
    // browsers) when it belongs to the caller. Either alone is enough.
    let mut session_ids: Vec<Uuid> = auth_user.session_id.into_iter().collect();
    let body_token = body.and_then(|b| b.0.refresh_token);
    if let Ok(raw_token) = extract_refresh_token(body_token, &jar) {
        let token_hash = hash_token(&raw_token);
        if let Some(session) = find_session_by_token_hash(&state.db, &token_hash).await? {
            if session.user_id == auth_user.id && !session_ids.contains(&session.id) {
                session_ids.push(session.id);
            }
        }
    }
    if session_ids.is_empty() {
        return Err(AuthError::InvalidToken);
    }

    // Delete the sessions (no more refreshes) and commit first: signing out
    // never leaves a refreshable session. Then end them (their access tokens
    // and sockets stop now); a Valkey failure answers 500 and a retry is safe.
    let mut tx = state.db.begin().await?;
    let mut ended = Vec::with_capacity(session_ids.len());
    for session_id in session_ids {
        if let Some(id) = queries::delete_user_session_tx(&mut tx, auth_user.id, session_id).await?
        {
            ended.push(id);
        }
    }
    // The access token's session may already be gone (e.g. revoked
    // elsewhere); denylisting it again is harmless.
    if let Some(own) = auth_user.session_id {
        if !ended.contains(&own) {
            ended.push(own);
        }
    }
    tx.commit().await?;
    super::revocation::end_sessions(&state, auth_user.id, &ended).await?;

    tracing::info!(user_id = %auth_user.id, "User logged out");

    Ok(jar.add(cookies::build_clear_cookie(&state.config)))
}

// ============================================================================
// OIDC
// ============================================================================

/// Get available OIDC providers.
///
/// GET /auth/oidc/providers
#[utoipa::path(
    get,
    path = "/auth/oidc/providers",
    tag = "auth",
    responses(
        (status = 200, description = "List of available OIDC providers"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state))]
pub async fn oidc_providers(State(state): State<AppState>) -> AuthResult<Json<serde_json::Value>> {
    let auth_methods = get_auth_methods_allowed(&state.db).await?;

    if !auth_methods.oidc {
        return Ok(Json(serde_json::json!({ "providers": [] })));
    }

    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    let providers = oidc_manager.list_public().await;

    Ok(Json(serde_json::json!({ "providers": providers })))
}

/// Initiate OIDC authorization.
///
/// GET /auth/oidc/authorize/:provider
#[utoipa::path(
    get,
    path = "/auth/oidc/authorize/{provider}",
    tag = "auth",
    params(
        ("provider" = String, Path, description = "OIDC provider name"),
        ("redirect_uri" = Option<String>, Query, description = "Optional desktop loopback return address (Tauri): `http://{127.0.0.1|localhost}[:port]/callback`, optionally with one per-flow value `?flow=<base64url, 22-128 chars>` that is handed back unchanged; the provider always returns to the server callback"),
        ("client_challenge" = Option<String>, Query, description = "Required with `redirect_uri`, refused without it: S256 challenge (unpadded base64url SHA-256, 43 chars) of the desktop's verifier. The loopback then receives `?code=` (one-time, 60 s), redeemed at `POST /auth/oidc/desktop/exchange`"),
        ("response" = Option<String>, Query, description = "`json` returns `{\"url\": ...}` instead of a 307 (always JSON for a desktop start)"),
        ("X-Kaiku-Client" = Option<String>, Header, description = "Must be `desktop` for a desktop start (`redirect_uri` or `client_challenge` given); otherwise 400 `OIDC_CLIENT_OUTDATED`"),
    ),
    responses(
        (status = 200, description = "Desktop start: `{\"url\": provider authorization URL}`"),
        (status = 307, description = "Browser: redirect to provider authorization URL"),
        (status = 400, description = "Invalid redirect_uri or client_challenge (`VALIDATION_ERROR`), or a desktop start without the desktop header or client_challenge (`OIDC_CLIENT_OUTDATED`)"),
        (status = 403, description = "Auth method disabled"),
    ),
    security(()),
)]
#[tracing::instrument(
    skip(state, headers, query),
    fields(provider = %provider, has_client_header = is_desktop_client(&headers))
)]
pub async fn oidc_authorize(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<OidcAuthorizeQuery>,
) -> Result<Response, AuthError> {
    // A desktop start must come from the app itself: browsers cannot send the
    // header cross-site (not allowed by CORS) and a clicked link cannot set
    // it, so a crafted start link never reaches the provider. The app reads
    // the provider URL from JSON instead of following a redirect.
    let is_desktop_start = query.redirect_uri.is_some() || query.client_challenge.is_some();
    if is_desktop_start && !is_desktop_client(&headers) {
        return Err(AuthError::OidcClientOutdated);
    }
    let want_json = is_desktop_start || query.response.as_deref() == Some("json");
    start_oidc_flow(
        &state,
        &provider,
        query.redirect_uri.as_deref(),
        query.client_challenge.as_deref(),
        OidcFlowPurpose::Login,
        want_json,
    )
    .await
}

/// Header (`X-Kaiku-Client: desktop`) the desktop app sends on SSO starts.
pub const DESKTOP_CLIENT_HEADER: &str = "x-kaiku-client";

/// Whether the request carries `X-Kaiku-Client: desktop`.
fn is_desktop_client(headers: &HeaderMap) -> bool {
    headers
        .get(DESKTOP_CLIENT_HEADER)
        .is_some_and(|v| v.as_bytes() == b"desktop")
}

/// Flow-state TTL of a login or link flow.
const OIDC_STATE_TTL_SECS: i64 = 600;
/// Flow-state TTL of a re-authentication flow.
const OIDC_REAUTH_STATE_TTL_SECS: i64 = 300;

/// Begin an OIDC authorization flow and redirect to the provider.
///
/// Shared by the public login flow, the authenticated identity-link flow and
/// the re-authentication flow; the only difference is what gets persisted in
/// the encrypted Redis flow state (see [`OidcFlowPurpose`]), so the callback
/// can tell them apart.
///
/// When `want_json` is set, the provider authorization URL is returned as a JSON
/// body (`{"url": ...}`) instead of a 307 redirect — for the browser link flow,
/// which fetches this as an authenticated XHR and opens the URL itself.
///
/// `client_challenge` is required for a desktop login (`redirect_uri` set) and
/// refused for every other flow; see [`OidcFlowState::client_challenge`].
pub async fn start_oidc_flow(
    state: &AppState,
    provider: &str,
    redirect_uri: Option<&str>,
    client_challenge: Option<&str>,
    purpose: OidcFlowPurpose,
    want_json: bool,
) -> Result<Response, AuthError> {
    let is_reauth = matches!(purpose, OidcFlowPurpose::Reauth(_));
    let auth_methods = get_auth_methods_allowed(&state.db).await?;
    if !auth_methods.oidc {
        return Err(AuthError::AuthMethodDisabled);
    }

    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    // Verify provider exists
    if oidc_manager.get_provider_row(provider).await.is_none() {
        return Err(AuthError::OidcProviderNotFound);
    }

    // Provider always returns to the server's callback; a desktop (Tauri)
    // loopback address is only where the server hands the result back.
    let redirects = resolve_flow_redirects(redirect_uri, state.config.public_url.as_deref())
        .map_err(|e| match e {
            FlowRedirectError::InvalidClientRedirect => {
                tracing::warn!(redirect_uri = ?redirect_uri, "Rejected non-localhost redirect_uri");
                AuthError::Validation(
                    "redirect_uri must be http://localhost or http://127.0.0.1".to_string(),
                )
            }
            FlowRedirectError::MissingPublicUrl => {
                tracing::error!("PUBLIC_URL env var is not set; required for OIDC");
                AuthError::Internal("Server misconfiguration: PUBLIC_URL is not set".to_string())
            }
        })?;

    // Desktop login hands back a one-time code bound to this challenge; an app
    // that sends none would expect tokens in the loopback URL and is refused.
    let is_desktop_login =
        matches!(purpose, OidcFlowPurpose::Login) && redirects.client_redirect_uri.is_some();
    let client_challenge = match (is_desktop_login, client_challenge) {
        (true, Some(challenge)) if is_valid_client_challenge(challenge) => {
            Some(challenge.to_string())
        }
        (true, Some(_)) => {
            return Err(AuthError::Validation(
                "client_challenge must be an unpadded base64url SHA-256 digest".to_string(),
            ))
        }
        (true, None) => return Err(AuthError::OidcClientOutdated),
        (false, Some(_)) => {
            return Err(AuthError::Validation(
                "client_challenge is only accepted for desktop sign-in".to_string(),
            ))
        }
        (false, None) => None,
    };

    let (auth_url, csrf_state, nonce, pkce_verifier) = oidc_manager
        .generate_auth_url_with(provider, &redirects.provider_redirect_uri, is_reauth)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, provider = %provider, "Failed to generate OIDC auth URL");
            AuthError::Internal(format!("Failed to generate auth URL: {e}"))
        })?;

    // Store OIDC state in Redis with 600s TTL
    let state_hash = hex::encode(Sha256::digest(csrf_state.as_bytes()));
    let redis_key = format!("oidc:state:{state_hash}");
    let flow_state = OidcFlowState {
        slug: provider.to_string(),
        pkce_verifier,
        nonce,
        redirect_uri: redirects.provider_redirect_uri,
        created_at: Utc::now().timestamp(),
        purpose,
        client_redirect_uri: redirects.client_redirect_uri,
        client_challenge,
    };
    let ttl = if is_reauth {
        OIDC_REAUTH_STATE_TTL_SECS
    } else {
        OIDC_STATE_TTL_SECS
    };

    let flow_json =
        serde_json::to_string(&flow_state).map_err(|e| AuthError::Internal(e.to_string()))?;

    // Encrypt the flow state before storing (protects PKCE verifier at rest)
    let enc_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;
    let enc_key_bytes = hex::decode(enc_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;
    let encrypted_flow = encrypt_mfa_secret(&flow_json, &enc_key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to encrypt OIDC state: {e}")))?;

    state
        .redis
        .set::<(), _, _>(
            &redis_key,
            encrypted_flow.as_str(),
            Some(Expiration::EX(ttl)),
            None,
            false,
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to store OIDC state in Redis");
            AuthError::Internal("Failed to store OIDC state".to_string())
        })?;

    tracing::info!(provider = %provider, purpose = flow_state.purpose.kind(), json = want_json, "Starting OIDC provider flow");
    if want_json {
        Ok(Json(serde_json::json!({ "url": auth_url })).into_response())
    } else {
        Ok(Redirect::temporary(&auth_url).into_response())
    }
}

/// Handle OIDC callback.
///
/// GET /auth/oidc/callback
#[utoipa::path(
    get,
    path = "/auth/oidc/callback",
    tag = "auth",
    params(
        ("code" = Option<String>, Query, description = "Authorization code from provider (absent on provider error)"),
        ("state" = String, Query, description = "CSRF state parameter"),
        ("error" = Option<String>, Query, description = "Provider error; returned to the sign-in origin as OIDC_PROVIDER_ERROR"),
    ),
    responses(
        (status = 200, description = "Browser popup result page (postMessage to opener), success or generic error code"),
        (status = 307, description = "Redirect to the desktop loopback with the result or a generic error code"),
        (status = 400, description = "Invalid or unknown state"),
    ),
)]
#[tracing::instrument(skip(state, jar, headers, query))]
pub async fn oidc_callback(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    axum::extract::Query(query): axum::extract::Query<OidcCallbackQuery>,
) -> Result<Response, AuthError> {
    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    // Lookup and delete OIDC state from Redis (one-time use)
    let state_hash = hex::encode(Sha256::digest(query.state.as_bytes()));
    let redis_key = format!("oidc:state:{state_hash}");

    // Atomically get and delete the state (one-time use, prevents replay)
    let encrypted_flow: Option<String> = state.redis.getdel(&redis_key).await.map_err(|e| {
        tracing::error!(error = %e, "Failed to read OIDC state from Redis");
        AuthError::Internal("Failed to read OIDC state".to_string())
    })?;

    let encrypted_flow = encrypted_flow.ok_or(AuthError::OidcStateMismatch)?;

    // Decrypt the flow state (PKCE verifier protected at rest)
    let enc_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;
    let enc_key_bytes = hex::decode(enc_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;
    let flow_json = decrypt_mfa_secret(&encrypted_flow, &enc_key_bytes).map_err(|e| {
        tracing::error!(error = %e, "Failed to decrypt OIDC state");
        AuthError::OidcStateMismatch
    })?;

    let flow_state: OidcFlowState =
        serde_json::from_str(&flow_json).map_err(|e| AuthError::Internal(e.to_string()))?;

    // From here on the state is valid and consumed: failures go back to the
    // sign-in origin (desktop loopback / browser popup) with a generic code
    // instead of an HTTP error the origin never sees.
    let code = match (query.code.as_deref(), query.error.as_deref()) {
        (Some(code), None) => code,
        (_, provider_error) => {
            let provider_error: Option<String> =
                provider_error.map(|e| e.chars().take(64).collect());
            tracing::info!(provider = %flow_state.slug, provider_error = ?provider_error, "OIDC provider returned no authorization code");
            return Ok(flow_failure_response(&flow_state, OIDC_PROVIDER_ERROR));
        }
    };

    // Re-authentication flow: verify the fresh provider login and elevate the
    // bound session; never logs in or links.
    if let OidcFlowPurpose::Reauth(ref binding) = flow_state.purpose {
        let client_ip = super::reauth::client_ip(&state, &headers, addr);
        return super::reauth::complete_oidc_reauth(&state, &flow_state, binding, code, client_ip)
            .await;
    }

    // Exchange code for tokens (also verifies ID token nonce for OIDC providers)
    let access_token = match oidc_manager
        .exchange_code(
            &flow_state.slug,
            code,
            &flow_state.pkce_verifier,
            &flow_state.redirect_uri,
            &flow_state.nonce,
        )
        .await
    {
        Ok((access_token, _id_token)) => access_token,
        Err(e) => {
            tracing::warn!(error = %e, provider = %flow_state.slug, "OIDC code exchange failed");
            return Ok(flow_failure_response(&flow_state, OIDC_EXCHANGE_FAILED));
        }
    };

    // Extract user info
    let user_info = match oidc_manager
        .extract_user_info(&flow_state.slug, &access_token)
        .await
    {
        Ok(user_info) => user_info,
        Err(e) => {
            tracing::warn!(error = %e, provider = %flow_state.slug, "Failed to extract OIDC user info");
            return Ok(flow_failure_response(&flow_state, OIDC_EXCHANGE_FAILED));
        }
    };

    // Composite external_id: "{provider_slug}:{subject}". Retained as the
    // user's primary-identity marker; the authoritative lookup is the
    // user_identities table keyed by (provider_slug, subject).
    //
    // Accounts can now attach additional identities (see handle_identity_link),
    // so users.external_id only ever holds the *first* identity — it is no
    // longer the identity-set uniqueness guarantee (user_identities is), and
    // it carries no UNIQUE constraint (demoted in TD-31).
    let external_id = format!("{}:{}", flow_state.slug, user_info.subject);

    // Link flow: attach this identity to the already-authenticated user instead
    // of logging in or creating an account.
    if let OidcFlowPurpose::Link {
        user_id: link_user_id,
        session_id: link_session_id,
    } = flow_state.purpose
    {
        return handle_identity_link(
            &state,
            &flow_state,
            &user_info,
            link_user_id,
            link_session_id,
        )
        .await;
    }

    // User resolution: resolve the external identity to an account.
    let existing_user =
        match find_user_id_by_identity(&state.db, &flow_state.slug, &user_info.subject).await? {
            Some(user_id) => find_user_by_id(&state.db, user_id).await?,
            None => None,
        };

    let user = if let Some(existing) = existing_user {
        // Existing user — login
        existing
    } else {
        // New user — check registration policy (fail-closed: deny if DB unreachable)
        let reg_policy_value = db::get_config_value(&state.db, "registration_policy")
            .await
            .map_err(|e| {
                tracing::error!(
                    error = %e,
                    provider = %flow_state.slug,
                    "Failed to read registration_policy config - denying OIDC registration (fail-closed)"
                );
                AuthError::Database(e)
            })?;
        let reg_policy = reg_policy_value.as_str().ok_or_else(|| {
            tracing::error!(
                actual_value = ?reg_policy_value,
                provider = %flow_state.slug,
                "registration_policy config value is not a string"
            );
            AuthError::Internal("Server configuration error".to_string())
        })?;
        if reg_policy != "open" {
            // Both "closed" and "invite_only" reject OIDC registration
            // (no mechanism to carry invite tokens through OIDC flow)
            return Err(AuthError::RegistrationDisabled);
        }

        // Generate username from claims
        let base_username = generate_username_from_claims(&user_info);

        let display_name = user_info
            .name
            .clone()
            .unwrap_or_else(|| base_username.clone());

        // Use a transaction for atomic first-user detection + user creation.
        // Retry on username collision (UNIQUE constraint violation).
        let mut username = base_username;
        let mut new_user = None;
        for attempt in 0..5u8 {
            if attempt > 0 {
                username = append_collision_suffix(&username);
            }

            let mut tx = state.db.begin().await.map_err(|e| {
                tracing::error!(error = %e, "Failed to start OIDC registration transaction");
                AuthError::Database(e)
            })?;

            // Lock setup_complete to serialize first-user detection (same pattern as local
            // register)
            queries::lock_setup_complete(&mut tx).await.map_err(|e| {
                tracing::error!(error = ?e, "Failed to lock setup_complete during OIDC registration");
                e
            })?;

            let user_count = queries::count_users(&mut tx).await?;
            let is_first_user = user_count == 0;

            match queries::insert_oidc_user(
                &mut tx,
                &username,
                &display_name,
                user_info.email.as_deref(),
                &external_id,
                user_info.avatar_url.as_deref(),
            )
            .await?
            {
                Some(user) => {
                    // Record the external identity (authoritative login key) in
                    // the same transaction as the user row.
                    match db::insert_user_identity(
                        &mut *tx,
                        user.id,
                        &flow_state.slug,
                        &user_info.subject,
                        user_info.email.as_deref(),
                    )
                    .await
                    {
                        Ok(_) => {}
                        // Lost a concurrent first-login race: another request
                        // registered this identity between our pre-loop lookup
                        // and here. Roll back and fall through to the post-loop
                        // re-resolve, which logs that account in.
                        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => {
                            drop(tx);
                            break;
                        }
                        Err(e) => {
                            tracing::error!(error = %e, user_id = %user.id, provider = %flow_state.slug, "Failed to record OIDC identity");
                            return Err(AuthError::Database(e));
                        }
                    }

                    // Grant admin to first user
                    if is_first_user {
                        queries::grant_first_user_admin(&mut tx, user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!(error = ?e, user_id = %user.id, "Failed to grant admin to first OIDC user");
                                e
                            })?;
                        tracing::info!(user_id = %user.id, "First user registered via OIDC and granted system admin");
                    }

                    tx.commit().await.map_err(AuthError::Database)?;

                    tracing::info!(
                        user_id = %user.id,
                        username = %user.username,
                        provider = %flow_state.slug,
                        "New user registered via OIDC"
                    );
                    new_user = Some(user);
                    break;
                }
                None => {
                    // Username collision — tx dropped (implicit rollback), retry with suffix
                    tracing::debug!(username = %username, "Username collision during OIDC registration, retrying");
                }
            }
        }

        match new_user {
            Some(user) => user,
            None => {
                // Either the username-retry loop exhausted, or we lost a
                // concurrent first-login race for this same identity (the
                // insert_user_identity unique violation broke out of the loop
                // above). Re-resolve the identity; if it now exists, log that
                // account in instead of failing.
                if let Some(user_id) =
                    find_user_id_by_identity(&state.db, &flow_state.slug, &user_info.subject)
                        .await?
                {
                    find_user_by_id(&state.db, user_id).await?.ok_or_else(|| {
                        AuthError::Internal("Identity resolved to a missing user".to_string())
                    })?
                } else {
                    tracing::error!(external_id = %external_id, "Failed to create OIDC user after 5 collision retries");
                    return Err(AuthError::Internal(
                        "Username generation failed after retries".to_string(),
                    ));
                }
            }
        }
    };

    // A banned account cannot start a new session.
    if queries::is_user_globally_banned(&state.db, user.id).await? {
        return Err(AuthError::AccountBanned);
    }

    // Best-effort: record that this identity was just used to log in.
    if let Err(e) =
        db::touch_user_identity_last_used(&state.db, &flow_state.slug, &user_info.subject).await
    {
        tracing::warn!(error = %e, provider = %flow_state.slug, "Failed to update identity last_used_at");
    }

    if let Some(redirect_url) = client_return_url(&flow_state) {
        // Desktop: hand back only a one-time code; the desktop redeems it with
        // its verifier at `POST /auth/oidc/desktop/exchange`.
        return Ok(desktop_login_hand_back(&state, &flow_state, redirect_url, user.id).await);
    }

    let (tokens, setup_complete) =
        issue_oidc_session(&state, user.id, &SessionClient::default()).await?;

    tracing::info!(user_id = %user.id, provider = %flow_state.slug, "User logged in via OIDC");

    // Browser flow: set HttpOnly refresh cookie + return HTML with postMessage
    let jar = jar.add(cookies::build_refresh_cookie(
        &tokens.refresh_token,
        state.config.jwt_refresh_expiry,
        &state.config,
    ));

    // JSON-encode tokens to prevent any injection via token values
    let payload = serde_json::json!({
        "type": "oidc-callback",
        "access_token": tokens.access_token,
        "expires_in": tokens.access_expires_in,
        "setup_required": !setup_complete,
    });
    let html = format!(
        r#"<!DOCTYPE html>
<html><body><script>
if (window.opener) {{
    window.opener.postMessage({payload}, window.location.origin);
    window.close();
}} else {{
    document.body.innerText = "Login successful. You can close this window.";
}}
</script></body></html>"#,
    );
    Ok((jar, axum::response::Html(html)).into_response())
}

/// Client address and user agent recorded with a new session.
#[derive(Default)]
struct SessionClient {
    ip: Option<String>,
    user_agent: Option<String>,
}

/// Start a session for an OIDC login; returns the token pair and whether
/// server setup is complete.
async fn issue_oidc_session(
    state: &AppState,
    user_id: Uuid,
    client: &SessionClient,
) -> AuthResult<(super::jwt::TokenPair, bool)> {
    let tokens = generate_token_pair(
        user_id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;
    let token_hash = hash_token(&tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    create_session(
        &state.db,
        tokens.refresh_token_id,
        user_id,
        &token_hash,
        expires_at,
        client.ip.as_deref(),
        client.user_agent.as_deref(),
        None,
        None,
    )
    .await?;
    let setup_complete = is_setup_complete(&state.db).await?;
    Ok((tokens, setup_complete))
}

/// Generic code: a desktop login state without a client challenge (started by
/// an older app or before the upgrade) cannot be handed back.
const OIDC_CLIENT_OUTDATED: &str = "OIDC_CLIENT_OUTDATED";

/// Finish a desktop login: store a one-time hand-off grant (60 s, no tokens)
/// and redirect the browser to the loopback with only `code` added to the
/// stored return address (which may carry the desktop's per-flow value).
async fn desktop_login_hand_back(
    state: &AppState,
    flow_state: &OidcFlowState,
    mut redirect_url: openidconnect::url::Url,
    user_id: Uuid,
) -> Response {
    let Some(client_challenge) = flow_state.client_challenge.clone() else {
        tracing::info!(provider = %flow_state.slug, "Desktop OIDC state without client challenge");
        return flow_failure_response(flow_state, OIDC_CLIENT_OUTDATED);
    };
    let code = new_handoff_code();
    let grant = OidcHandoffGrant {
        user_id,
        slug: flow_state.slug.clone(),
        client_challenge,
        created_at: Utc::now().timestamp(),
    };
    let stored = match serde_json::to_string(&grant) {
        Ok(json) => state
            .redis
            .set::<(), _, _>(
                handoff_key(&code),
                json.as_str(),
                Some(Expiration::EX(OIDC_HANDOFF_TTL_SECS)),
                None,
                false,
            )
            .await
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = stored {
        tracing::error!(error = %e, "Failed to store OIDC hand-off code");
        return flow_failure_response(flow_state, "SERVICE_UNAVAILABLE");
    }
    tracing::info!(user_id = %user_id, provider = %flow_state.slug, "OIDC desktop login handed off");
    redirect_url.query_pairs_mut().append_pair("code", &code);
    loopback_redirect(&redirect_url)
}

/// 307 to the desktop loopback: never cached, never sent on as a referrer.
fn loopback_redirect(url: &openidconnect::url::Url) -> Response {
    let mut response = Redirect::temporary(url.as_str()).into_response();
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

/// Redeem a desktop hand-off code.
///
/// POST /auth/oidc/desktop/exchange
///
/// The code is single use (consumed by the first attempt, also a failed one)
/// and valid for 60 s; `verifier` must hash (S256) to the `client_challenge`
/// of the authorize request. Returns the login token response with the
/// refresh token in the body (no cookie).
#[utoipa::path(
    post,
    path = "/auth/oidc/desktop/exchange",
    tag = "auth",
    request_body = OidcHandoffExchangeRequest,
    responses(
        (status = 200, description = "Signed in", body = AuthResponse),
        (status = 400, description = "Unknown, expired, used or wrongly verified code (`OIDC_HANDOFF_INVALID`)"),
        (status = 403, description = "Account banned (`ACCOUNT_BANNED`)"),
        (status = 429, description = "Rate limited"),
        (status = 503, description = "Valkey unavailable (`SERVICE_UNAVAILABLE`)"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, headers, body))]
pub async fn oidc_desktop_exchange(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<OidcHandoffExchangeRequest>,
) -> AuthResult<Json<AuthResponse>> {
    if body.code.is_empty() || body.code.len() > 128 {
        return Err(AuthError::OidcHandoffInvalid);
    }
    let raw: Option<String> = state
        .redis
        .getdel(handoff_key(&body.code))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to read OIDC hand-off code");
            AuthError::ServiceUnavailable
        })?;
    let grant: OidcHandoffGrant = raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .ok_or(AuthError::OidcHandoffInvalid)?;
    let age = Utc::now().timestamp() - grant.created_at;
    if !(0..=OIDC_HANDOFF_TTL_SECS).contains(&age)
        || !client_challenge_matches(&body.verifier, &grant.client_challenge)
    {
        tracing::info!(provider = %grant.slug, "OIDC hand-off code refused (expired or verifier mismatch)");
        return Err(AuthError::OidcHandoffInvalid);
    }

    let user = find_user_by_id(&state.db, grant.user_id)
        .await?
        .ok_or(AuthError::OidcHandoffInvalid)?;
    if queries::is_user_globally_banned(&state.db, user.id).await? {
        return Err(AuthError::AccountBanned);
    }

    let client = SessionClient {
        ip: Some(addr.ip().to_string()),
        user_agent: extract_user_agent(&headers),
    };
    let (tokens, setup_complete) = issue_oidc_session(&state, user.id, &client).await?;
    tracing::info!(user_id = %user.id, provider = %grant.slug, "User logged in via OIDC (desktop)");

    Ok(Json(AuthResponse {
        access_token: tokens.access_token,
        refresh_token: Some(tokens.refresh_token),
        expires_in: tokens.access_expires_in,
        token_type: "Bearer".to_string(),
        setup_required: !setup_complete,
        password_change_required: user.must_change_password,
    }))
}

/// Attach a freshly-authenticated external identity to an existing account
/// (the OIDC *link* flow). Unlike login, this issues no tokens.
///
/// Refuses to steal an identity already bound to a different account. A conflict
/// (already bound elsewhere, or the provider already linked to this account) is
/// reported back to the opener as a generic `IDENTITY_ALREADY_LINKED` code via
/// the same redirect/postMessage channel — not as an HTTP error.
async fn handle_identity_link(
    state: &AppState,
    flow_state: &OidcFlowState,
    user_info: &OidcUserInfo,
    link_user_id: Uuid,
    link_session_id: Option<Uuid>,
) -> Result<Response, AuthError> {
    // The session that started the flow must still be live, not revoked, of
    // an unbanned account and (for a system admin) elevated.
    if let Some(code) =
        super::identities::link_refusal(state, link_user_id, link_session_id).await?
    {
        tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, error_code = code, "OIDC identity link refused: starting session no longer valid");
        return Ok(flow_failure_response(flow_state, code));
    }

    // Determine the link outcome. Genuine server faults (DB errors) propagate as
    // AuthError; a *conflict* instead becomes a generic error code carried back to
    // the opener via the same redirect/postMessage channel, so the popup always
    // closes rather than hanging on an HTTP error page. The code is intentionally
    // ownerless — it never reveals which account holds the identity.
    let error_code: Option<&'static str> = match find_user_id_by_identity(
        &state.db,
        &flow_state.slug,
        &user_info.subject,
    )
    .await?
    {
        // Already linked to this account — idempotent success.
        Some(existing) if existing == link_user_id => {
            tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, "OIDC identity already linked to this account");
            None
        }
        // Bound to someone else — refuse.
        Some(_) => Some("IDENTITY_ALREADY_LINKED"),
        None => match db::insert_user_identity(
            &state.db,
            link_user_id,
            &flow_state.slug,
            &user_info.subject,
            user_info.email.as_deref(),
        )
        .await
        {
            Ok(identity) => {
                // Linked from an elevated session: may confirm re-authentication
                // right away (C24); otherwise only sessions started later can.
                if let Some(session_id) = link_session_id {
                    if crate::admin::queries::has_active_elevated_session(
                        &state.db,
                        link_user_id,
                        session_id,
                    )
                    .await
                    .map_err(|e| AuthError::Internal(format!("Elevation lookup failed: {e}")))?
                    {
                        sqlx::query(
                            "UPDATE user_identities SET linked_while_elevated = TRUE WHERE id = $1",
                        )
                        .bind(identity.id)
                        .execute(&state.db)
                        .await?;
                    }
                }
                tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, "Linked OIDC identity to account");
                None
            }
            // Lost a race, or the account already has an identity for this
            // provider (the (user_id, provider_slug) UNIQUE constraint).
            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                Some("IDENTITY_ALREADY_LINKED")
            }
            Err(e) => return Err(AuthError::Database(e)),
        },
    };

    // Token-less result response (mirrors the login callback's Tauri-vs-browser
    // split, minus the session/cookie since the caller is already authenticated).
    // Always completes the handshake — success or a generic error code.
    if let Some(code) = error_code {
        tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, error_code = code, "OIDC identity link rejected");
        return Ok(flow_failure_response(flow_state, code));
    }

    if let Some(mut redirect_url) = client_return_url(flow_state) {
        redirect_url
            .query_pairs_mut()
            .append_pair("linked", &flow_state.slug);
        Ok(loopback_redirect(&redirect_url))
    } else {
        let payload = serde_json::json!({
            "type": "oidc-link-callback",
            "success": true,
            "provider_slug": flow_state.slug,
            "error_code": null,
        });
        Ok(axum::response::Html(popup_result_page(&payload, true)).into_response())
    }
}

/// Generic code: the provider returned an error instead of an authorization code.
const OIDC_PROVIDER_ERROR: &str = "OIDC_PROVIDER_ERROR";
/// Generic code: code exchange or user-info lookup with the provider failed.
const OIDC_EXCHANGE_FAILED: &str = "OIDC_EXCHANGE_FAILED";

/// Hand a failed flow back to its origin with a generic error code (never
/// provider text). Only used once the flow state has been validated, so the
/// return target is the stored, re-validated loopback — never callback input.
///
/// | flow  | desktop loopback       | browser popup (`postMessage`)                        |
/// |-------|------------------------|------------------------------------------------------|
/// | login | `?error=CODE`          | `{type: "oidc-callback", error}`                     |
/// | link  | `?link_error=CODE`     | `{type: "oidc-link-callback", success: false, error_code}` |
/// | reauth | `?reauth_error=CODE`  | `{type: "oidc-reauth-callback", success: false, error_code, expires_at: null}` |
fn flow_failure_response(flow_state: &OidcFlowState, code: &'static str) -> Response {
    let is_link = match flow_state.purpose {
        OidcFlowPurpose::Reauth(_) => {
            return super::reauth::reauth_failure_response(flow_state, code);
        }
        OidcFlowPurpose::Link { .. } => true,
        OidcFlowPurpose::Login => false,
    };
    if let Some(mut redirect_url) = client_return_url(flow_state) {
        let key = if is_link { "link_error" } else { "error" };
        redirect_url.query_pairs_mut().append_pair(key, code);
        return loopback_redirect(&redirect_url);
    }
    let payload = if is_link {
        serde_json::json!({
            "type": "oidc-link-callback",
            "success": false,
            "provider_slug": flow_state.slug,
            "error_code": code,
        })
    } else {
        serde_json::json!({ "type": "oidc-callback", "error": code })
    };
    // The link UI handles failures itself (closes the popup); the login page
    // only reacts to success, so a failed login keeps the popup open with a hint.
    axum::response::Html(popup_result_page(&payload, is_link)).into_response()
}

/// Popup page that posts `payload` to the opener (same origin only). With
/// `close` it closes itself afterwards; otherwise it stays open showing that
/// sign-in failed.
fn popup_result_page(payload: &serde_json::Value, close: bool) -> String {
    let (close_js, fallback) = if close {
        ("window.close();", "You can close this window.")
    } else {
        (
            "",
            "Sign-in did not complete. Close this window and try again.",
        )
    };
    format!(
        r#"<!DOCTYPE html>
<html><body><script>
if (window.opener) {{
    window.opener.postMessage({payload}, window.location.origin);
    {close_js}
}}
document.body.innerText = "{fallback}";
</script></body></html>"#,
    )
}

/// Hand a token-less OIDC flow result back to the client that started it.
///
/// Desktop (`return_url`, see [`client_return_url`]): redirect there with
/// `query` appended. Browser: a popup page that posts `payload` to the opener
/// window and closes itself.
pub(super) fn oidc_hand_back(
    return_url: Option<openidconnect::url::Url>,
    query: &[(&str, String)],
    payload: &serde_json::Value,
) -> Response {
    if let Some(mut redirect_url) = return_url {
        {
            let mut pairs = redirect_url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        loopback_redirect(&redirect_url)
    } else {
        axum::response::Html(popup_result_page(payload, true)).into_response()
    }
}

// ============================================================================
// Password Reset
// ============================================================================

/// Request a password reset email.
///
/// Always returns 200 with a generic message to prevent user enumeration.
/// If SMTP is not configured, returns 503.
///
/// POST /auth/forgot-password
#[utoipa::path(
    post,
    path = "/auth/forgot-password",
    tag = "auth",
    request_body = ForgotPasswordRequest,
    responses(
        (status = 200, description = "Password reset email sent (returns success regardless of whether account exists)"),
        (status = 503, description = "Email service not configured"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, body))]
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    // Check if email service is configured
    let email_service = state.email.as_ref().ok_or(AuthError::EmailNotConfigured)?;

    // Basic email format validation
    if !body.email.contains('@') || body.email.len() < 5 {
        // Still return success to prevent enumeration
        return Ok(Json(serde_json::json!({
            "message": "If an account with that email exists, a reset code has been sent."
        })));
    }

    // Look up user by email — catch DB errors to prevent enumeration via 500 responses
    let user = match find_user_by_email(&state.db, &body.email).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            // User not found — return success silently (no enumeration)
            return Ok(Json(serde_json::json!({
                "message": "If an account with that email exists, a reset code has been sent."
            })));
        }
        Err(e) => {
            tracing::error!(error = %e, "Database error during password reset user lookup");
            return Ok(Json(serde_json::json!({
                "message": "If an account with that email exists, a reset code has been sent."
            })));
        }
    };

    // Only allow password reset for local auth users
    if user.auth_method != crate::db::AuthMethod::Local {
        return Ok(Json(serde_json::json!({
            "message": "If an account with that email exists, a reset code has been sent."
        })));
    }

    // Invalidate existing tokens for this user — abort if this fails to prevent token accumulation
    if let Err(e) = invalidate_user_reset_tokens(&state.db, user.id).await {
        tracing::error!(
            error = %e,
            user_id = %user.id,
            "Failed to invalidate existing reset tokens, aborting reset flow"
        );
        return Ok(Json(serde_json::json!({
            "message": "If an account with that email exists, a reset code has been sent."
        })));
    }

    // Generate 32 random bytes → base64url token
    use base64::Engine;
    use rand::RngCore;

    let mut token_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let raw_token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(token_bytes);

    // Hash for DB storage
    let token_hash = hash_token(&raw_token);
    let expires_at = Utc::now() + Duration::hours(1);

    // Insert token into DB — catch DB errors to prevent enumeration via 500 responses
    if let Err(e) = create_password_reset_token(&state.db, user.id, &token_hash, expires_at).await {
        tracing::error!(error = %e, user_id = %user.id, "Failed to create password reset token");
        return Ok(Json(serde_json::json!({
            "message": "If an account with that email exists, a reset code has been sent."
        })));
    }

    // Send email — log warning on failure, return same generic response to prevent enumeration
    match email_service
        .send_password_reset(&body.email, &user.username, &raw_token)
        .await
    {
        Ok(()) => {
            tracing::info!(user_id = %user.id, "Password reset email sent");
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                user_id = %user.id,
                "Failed to send password reset email"
            );
            // Clean up the orphaned token since the user never received it
            if let Err(cleanup_err) = invalidate_user_reset_tokens(&state.db, user.id).await {
                tracing::error!(
                    error = %cleanup_err,
                    user_id = %user.id,
                    "Failed to clean up orphaned password reset token after email failure"
                );
            }
        }
    }

    // Always return generic message to prevent user enumeration
    Ok(Json(serde_json::json!({
        "message": "If an account with that email exists, a reset code has been sent."
    })))
}

/// Reset password using a reset token.
///
/// POST /auth/reset-password
#[utoipa::path(
    post,
    path = "/auth/reset-password",
    tag = "auth",
    request_body = ResetPasswordRequest,
    responses(
        (status = 200, description = "Password reset successfully"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, body))]
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    // Validate password length
    if body.new_password.len() < 8 || body.new_password.len() > 128 {
        return Err(AuthError::Validation(
            "Password must be between 8 and 128 characters".to_string(),
        ));
    }

    // Hash the provided token and look it up
    let token_hash = hash_token(&body.token);
    let reset_token = find_valid_reset_token(&state.db, &token_hash)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    // Hash the new password
    let password_hash = hash_password(&body.new_password).map_err(|_| AuthError::PasswordHash)?;

    // Transaction: mark token used → update password → delete all sessions
    let mut tx = state.db.begin().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to start password reset transaction");
        AuthError::Database(e)
    })?;

    // Mark token as used
    queries::mark_reset_token_used_tx(&mut tx, reset_token.id)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, token_id = %reset_token.id, "Failed to mark reset token used");
            e
        })?;

    // Update password
    queries::update_password_hash_tx(&mut tx, reset_token.user_id, &password_hash)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, user_id = %reset_token.user_id, "Failed to update password");
            e
        })?;

    // Delete all user sessions (force re-login everywhere)
    let ended = queries::delete_user_sessions_except(&mut tx, reset_token.user_id, None)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, user_id = %reset_token.user_id, "Failed to delete sessions");
            AuthError::Database(e)
        })?;
    tx.commit().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to commit password reset transaction");
        AuthError::Database(e)
    })?;

    // End them now too: their access tokens and every open socket of the
    // user stop working.
    super::revocation::end_all_sessions(&state, reset_token.user_id, &ended).await?;

    tracing::info!(user_id = %reset_token.user_id, "Password reset successful, all sessions invalidated");

    Ok(Json(serde_json::json!({
        "message": "Password has been reset successfully. Please log in with your new password."
    })))
}
