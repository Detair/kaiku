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
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::backup_codes::find_matching_backup_code;
use super::error::{AuthError, AuthResult};
use super::helpers::{extract_refresh_token, extract_user_agent, should_return_refresh_token};
use super::jwt::{generate_token_pair, validate_refresh_token};
use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use super::middleware::AuthUser;
use super::oidc::{
    append_collision_suffix, generate_username_from_claims, OidcFlowState, OidcUserInfo,
};
use super::password::{hash_password, verify_password};
use super::queries::{self, InsertSessionParams};
use super::types::{
    AuthResponse, ForgotPasswordRequest, LoginRequest, LogoutRequest, OidcAuthorizeQuery,
    OidcCallbackQuery, RefreshRequest, ResetPasswordRequest,
};
use super::{cookies, geoip, hash_token};
use crate::api::AppState;
use crate::db::{
    self, create_password_reset_token, create_session, delete_session_by_token_hash,
    find_session_by_token_hash, find_user_by_email, find_user_by_id, find_user_by_username,
    find_user_id_by_identity, find_valid_reset_token, get_auth_methods_allowed,
    get_unused_mfa_backup_codes, invalidate_user_reset_tokens, is_setup_complete,
    mark_mfa_backup_code_used,
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
#[tracing::instrument(skip(state, jar, body, normalized_ip), fields(username = %body.username))]
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    normalized_ip: Option<Extension<NormalizedIp>>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<LoginRequest>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    // Helper macro to record failed auth (if rate limiter is configured)
    // SECURITY: Fails request if rate limiter is down (fail-closed pattern)
    macro_rules! record_failed_auth {
        () => {
            if let (Some(ref rl), Some(Extension(ref nip))) = (&state.rate_limiter, &normalized_ip)
            {
                if let Err(e) = rl.record_failed_auth(&nip.0).await {
                    tracing::error!(
                        error = %e,
                        ip = ?nip.0,
                        username = %body.username,
                        "SECURITY: Failed to record failed authentication - BLOCKING REQUEST to prevent rate limit bypass"
                    );
                    // Fail closed - deny request when rate limiter is unavailable
                    // This prevents attackers from bypassing rate limiting by triggering rate limiter failures
                    return Err(AuthError::Internal(
                        "Authentication service temporarily unavailable. Please try again later.".to_string()
                    ));
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

    // Check MFA if enabled
    if let Some(ref encrypted_secret) = user.mfa_secret {
        // MFA is enabled - code is required
        let mfa_code = body.mfa_code.as_ref().ok_or(AuthError::MfaRequired)?;

        // Get encryption key from config
        let encryption_key = state
            .config
            .mfa_encryption_key
            .as_ref()
            .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;

        // Decode encryption key from hex
        let key_bytes = hex::decode(encryption_key)
            .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;

        // Decrypt the secret
        let secret_str = decrypt_mfa_secret(encrypted_secret, &key_bytes)
            .map_err(|e| AuthError::Internal(format!("Failed to decrypt MFA secret: {e}")))?;

        // Parse the secret and create TOTP instance
        let secret = Secret::Encoded(secret_str);
        let totp = TOTP::new(
            Algorithm::SHA1,
            6,
            1,
            30,
            secret
                .to_bytes()
                .map_err(|_| AuthError::Internal("Invalid TOTP secret encoding".into()))?,
            Some("Kaiku".to_string()),
            user.username.clone(),
        )
        .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;

        // Try TOTP code first
        let totp_valid = totp
            .check_current(mfa_code)
            .map_err(|e| AuthError::Internal(format!("Failed to verify TOTP code: {e}")))?;

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
#[tracing::instrument(skip(state, jar, body))]
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

    // Verify user still exists
    let _user = find_user_by_id(&state.db, user_id)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    // Delete old session within the transaction
    queries::delete_session_by_token_hash_tx(&mut tx, &token_hash).await?;

    // Generate new token pair
    let new_tokens = generate_token_pair(
        user_id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;

    // Store new refresh token session within the transaction
    let new_token_hash = hash_token(&new_tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    let city = geo.as_ref().and_then(|g| g.city.as_deref());
    let country = geo.as_ref().and_then(|g| g.country.as_deref());

    let ip_str = addr.ip().to_string();
    queries::insert_session_tx(
        &mut tx,
        &InsertSessionParams {
            id: new_tokens.refresh_token_id,
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
    request_body(content = LogoutRequest, description = "Required for Tauri clients; browser clients use HttpOnly cookie instead"),
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
    // Read refresh token from JSON body (Tauri) or HttpOnly cookie (browser)
    let raw_token = extract_refresh_token(body.map(|b| b.0.refresh_token), &jar)?;

    // Verify the session belongs to the authenticated user before deleting
    let token_hash = hash_token(&raw_token);
    let session = find_session_by_token_hash(&state.db, &token_hash)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    if session.user_id != auth_user.id {
        return Err(AuthError::InvalidToken);
    }

    delete_session_by_token_hash(&state.db, &token_hash).await?;

    // Deleting the refresh session kills future refreshes, but the current
    // stateless access token would otherwise stay valid until it expires.
    // Add its session id (== the access token's `sid` claim) to the revocation
    // denylist for the access-token lifetime so it stops working immediately.
    if let Err(e) = super::revocation::revoke_session(
        &state.redis,
        &session.id.to_string(),
        state.config.jwt_access_expiry,
    )
    .await
    {
        tracing::warn!(error = %e, user_id = %auth_user.id, "Failed to denylist access token on logout");
    }

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
        ("redirect_uri" = Option<String>, Query, description = "Optional redirect URI override"),
    ),
    responses(
        (status = 307, description = "Redirect to provider authorization URL"),
        (status = 400, description = "OIDC not configured or auth method disabled"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, query), fields(provider = %provider))]
pub async fn oidc_authorize(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    axum::extract::Query(query): axum::extract::Query<OidcAuthorizeQuery>,
) -> Result<Response, AuthError> {
    let want_json = query.response.as_deref() == Some("json");
    start_oidc_flow(
        &state,
        &provider,
        query.redirect_uri.as_deref(),
        None,
        want_json,
    )
    .await
}

/// Begin an OIDC authorization flow and redirect to the provider.
///
/// Shared by the public login flow (`link_user_id = None`) and the
/// authenticated identity-link flow (`link_user_id = Some(current user)`); the
/// only difference is what gets persisted in the encrypted Redis flow state, so
/// the callback can tell a login apart from a link.
///
/// When `want_json` is set, the provider authorization URL is returned as a JSON
/// body (`{"url": ...}`) instead of a 307 redirect — for the browser link flow,
/// which fetches this as an authenticated XHR and opens the URL itself.
pub async fn start_oidc_flow(
    state: &AppState,
    provider: &str,
    redirect_uri: Option<&str>,
    link_user_id: Option<Uuid>,
    want_json: bool,
) -> Result<Response, AuthError> {
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

    // Determine callback URL
    let callback_base = if let Some(redirect_uri) = redirect_uri {
        // Tauri flow: validate the redirect URI is a localhost callback
        let parsed = openidconnect::url::Url::parse(redirect_uri)
            .map_err(|_| AuthError::Validation("Invalid redirect_uri".to_string()))?;
        if matches!(
            (parsed.scheme(), parsed.host_str()),
            ("http", Some("localhost" | "127.0.0.1"))
        ) {
            redirect_uri.to_string()
        } else {
            tracing::warn!(redirect_uri = %redirect_uri, "Rejected non-localhost redirect_uri");
            return Err(AuthError::Validation(
                "redirect_uri must be http://localhost or http://127.0.0.1".to_string(),
            ));
        }
    } else {
        // Browser flow: use the server's own callback endpoint
        let public_url = std::env::var("PUBLIC_URL").map_err(|_| {
            tracing::error!("PUBLIC_URL env var is not set; required for OIDC browser flow");
            AuthError::Internal("Server misconfiguration: PUBLIC_URL is not set".to_string())
        })?;
        format!("{public_url}/auth/oidc/callback")
    };

    let (auth_url, csrf_state, nonce, pkce_verifier) = oidc_manager
        .generate_auth_url(provider, &callback_base)
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
        redirect_uri: callback_base,
        created_at: Utc::now().timestamp(),
        link_user_id,
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
            Some(Expiration::EX(600)),
            None,
            false,
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to store OIDC state in Redis");
            AuthError::Internal("Failed to store OIDC state".to_string())
        })?;

    tracing::info!(provider = %provider, link = link_user_id.is_some(), json = want_json, "Starting OIDC provider flow");
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
        ("code" = String, Query, description = "Authorization code from provider"),
        ("state" = String, Query, description = "CSRF state parameter"),
    ),
    responses(
        (status = 307, description = "Redirect to client with auth token"),
        (status = 400, description = "Invalid callback parameters"),
    ),
)]
#[tracing::instrument(skip(state, jar, query))]
pub async fn oidc_callback(
    State(state): State<AppState>,
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

    // Exchange code for tokens (also verifies ID token nonce for OIDC providers)
    let (access_token, _id_token) = oidc_manager
        .exchange_code(
            &flow_state.slug,
            &query.code,
            &flow_state.pkce_verifier,
            &flow_state.redirect_uri,
            &flow_state.nonce,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, provider = %flow_state.slug, "OIDC code exchange failed");
            AuthError::OidcCodeExchangeFailed(e.to_string())
        })?;

    // Extract user info
    let user_info = oidc_manager
        .extract_user_info(
            &flow_state.slug,
            &access_token,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, provider = %flow_state.slug, "Failed to extract OIDC user info");
            AuthError::OidcCodeExchangeFailed(format!("Failed to extract user info: {e}"))
        })?;

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
    if let Some(link_user_id) = flow_state.link_user_id {
        return handle_identity_link(&state, &flow_state, &user_info, link_user_id).await;
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

    // Best-effort: record that this identity was just used to log in.
    if let Err(e) =
        db::touch_user_identity_last_used(&state.db, &flow_state.slug, &user_info.subject).await
    {
        tracing::warn!(error = %e, provider = %flow_state.slug, "Failed to update identity last_used_at");
    }

    // Generate JWT token pair
    let tokens = generate_token_pair(
        user.id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;

    // Store session
    let token_hash = hash_token(&tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    create_session(
        &state.db,
        tokens.refresh_token_id,
        user.id,
        &token_hash,
        expires_at,
        None,
        None,
        None,
        None,
    )
    .await?;

    let setup_complete = is_setup_complete(&state.db).await?;

    tracing::info!(user_id = %user.id, provider = %flow_state.slug, "User logged in via OIDC");

    // Check if redirect_uri is a localhost callback (Tauri flow)
    let parsed_redirect = openidconnect::url::Url::parse(&flow_state.redirect_uri)
        .map_err(|e| AuthError::Internal(format!("Invalid redirect URI: {e}")))?;
    let is_localhost = matches!(
        (parsed_redirect.scheme(), parsed_redirect.host_str()),
        ("http", Some("localhost" | "127.0.0.1"))
    );

    if is_localhost {
        // Tauri flow: redirect with tokens in query params
        let mut redirect_url = parsed_redirect;
        redirect_url
            .query_pairs_mut()
            .append_pair("access_token", &tokens.access_token)
            .append_pair("refresh_token", &tokens.refresh_token)
            .append_pair("expires_in", &tokens.access_expires_in.to_string())
            .append_pair("setup_required", &(!setup_complete).to_string());
        Ok(Redirect::temporary(redirect_url.as_str()).into_response())
    } else {
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
) -> Result<Response, AuthError> {
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
            Ok(_) => {
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

    if let Some(code) = error_code {
        tracing::info!(user_id = %link_user_id, provider = %flow_state.slug, error_code = code, "OIDC identity link rejected");
    }

    // Token-less result response (mirrors the login callback's Tauri-vs-browser
    // split, minus the session/cookie since the caller is already authenticated).
    // Always completes the handshake — success or a generic error code.
    let parsed_redirect = openidconnect::url::Url::parse(&flow_state.redirect_uri)
        .map_err(|e| AuthError::Internal(format!("Invalid redirect URI: {e}")))?;
    let is_localhost = matches!(
        (parsed_redirect.scheme(), parsed_redirect.host_str()),
        ("http", Some("localhost" | "127.0.0.1"))
    );

    if is_localhost {
        let mut redirect_url = parsed_redirect;
        match error_code {
            Some(code) => redirect_url
                .query_pairs_mut()
                .append_pair("link_error", code),
            None => redirect_url
                .query_pairs_mut()
                .append_pair("linked", &flow_state.slug),
        };
        Ok(Redirect::temporary(redirect_url.as_str()).into_response())
    } else {
        let payload = serde_json::json!({
            "type": "oidc-link-callback",
            "success": error_code.is_none(),
            "provider_slug": flow_state.slug,
            "error_code": error_code,
        });
        let html = format!(
            r#"<!DOCTYPE html>
<html><body><script>
if (window.opener) {{
    window.opener.postMessage({payload}, window.location.origin);
    window.close();
}} else {{
    document.body.innerText = "You can close this window.";
}}
</script></body></html>"#,
        );
        Ok(axum::response::Html(html).into_response())
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
    queries::delete_all_user_sessions_tx(&mut tx, reset_token.user_id)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, user_id = %reset_token.user_id, "Failed to delete sessions");
            e
        })?;

    tx.commit().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to commit password reset transaction");
        AuthError::Database(e)
    })?;

    tracing::info!(user_id = %reset_token.user_id, "Password reset successful, all sessions invalidated");

    Ok(Json(serde_json::json!({
        "message": "Password has been reset successfully. Please log in with your new password."
    })))
}
