//! MFA (TOTP) and QR login handlers.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::CookieJar;
use fred::prelude::*;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::error::{AuthError, AuthResult};
use super::helpers::should_return_refresh_token;
use super::mfa_crypto::encrypt_mfa_secret;
use super::middleware::AuthUser;
use super::reauth;
use super::types::{
    AuthResponse, MfaSetupResponse, MfaVerifyRequest, QrCreateResponse, QrRedeemRequest,
};
use crate::api::AppState;
use crate::db::{find_user_by_id, set_mfa_secret};

/// Setup MFA (TOTP).
///
/// POST /auth/mfa/setup
///
/// Stores the TOTP secret as pending in Redis (5-minute TTL).
/// MFA is only activated once the user verifies a valid TOTP code via
/// `POST /auth/mfa/verify`, which moves the secret to permanent DB storage.
#[utoipa::path(
    post,
    path = "/auth/mfa/setup",
    tag = "auth",
    responses(
        (status = 200, description = "MFA setup initiated", body = MfaSetupResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn mfa_setup(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<MfaSetupResponse>> {
    // Check if encryption key is configured
    let encryption_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;

    // Decode encryption key from hex
    let key_bytes = hex::decode(encryption_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;

    if key_bytes.len() != 32 {
        return Err(AuthError::Internal(
            "MFA encryption key must be 32 bytes".to_string(),
        ));
    }

    // Generate a new TOTP secret (20 bytes = 160 bits, standard for TOTP)
    let secret = Secret::default();
    let secret_str = secret.to_encoded().to_string();

    // Encrypt the secret before storing
    let encrypted_secret = encrypt_mfa_secret(&secret_str, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to encrypt MFA secret: {e}")))?;

    // Store as pending in Redis (5-minute TTL) instead of directly in DB.
    // The secret is only persisted to the DB after successful TOTP verification.
    let redis_key = format!("mfa:pending:{}", auth_user.id);
    state
        .redis
        .set::<(), _, _>(
            &redis_key,
            &encrypted_secret,
            Some(Expiration::EX(300)),
            None,
            false,
        )
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to store pending MFA secret: {e}")))?;

    // Create TOTP instance for QR code
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|_| AuthError::Internal("Invalid TOTP secret encoding".into()))?,
        Some("Kaiku".to_string()),
        auth_user.username.clone(),
    )
    .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;

    // Generate QR code URI (otpauth://)
    let qr_code_url = totp.get_url();

    Ok(Json(MfaSetupResponse {
        secret: secret_str,
        qr_code_url,
    }))
}

/// Verify MFA code (TOTP).
///
/// POST /auth/mfa/verify
#[utoipa::path(
    post,
    path = "/auth/mfa/verify",
    tag = "auth",
    request_body = MfaVerifyRequest,
    responses(
        (status = 200, description = "MFA verification successful"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers, request), fields(user_id = %auth_user.id))]
pub async fn mfa_verify(
    State(state): State<AppState>,
    auth_user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<MfaVerifyRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    reauth::ensure_not_locked(&state, auth_user.id).await?;

    // Get user to retrieve encrypted MFA secret (the TOTP check decrypts it)
    let user = find_user_by_id(&state.db, auth_user.id)
        .await
        .map_err(|_| AuthError::Internal("Database error".to_string()))?
        .ok_or(AuthError::UserNotFound)?;

    // Check if MFA is enabled — either permanently (DB) or pending setup (Redis).
    let (encrypted_secret, is_pending_setup) = if let Some(secret) = user.mfa_secret {
        (secret, false)
    } else {
        // Check for pending MFA secret in Redis (from mfa_setup)
        let redis_key = format!("mfa:pending:{}", auth_user.id);
        let pending: Option<String> =
            state.redis.get(&redis_key).await.map_err(|e| {
                AuthError::Internal(format!("Failed to check pending MFA secret: {e}"))
            })?;
        match pending {
            Some(secret) => (secret, true),
            None => return Err(AuthError::Validation("MFA not enabled".to_string())),
        }
    };

    // Try TOTP code first (accepted once per step: replay protection)
    let totp_valid = reauth::accept_totp(
        &state,
        auth_user.id,
        &encrypted_secret,
        &user.username,
        &request.code,
    )
    .await?;

    if !totp_valid {
        let client_ip = reauth::client_ip(&state, &headers, addr);
        reauth::record_failure(&state, auth_user.id, client_ip).await?;
        return Err(AuthError::InvalidMfaCode);
    }

    // If this was a pending setup, persist the secret to DB and clean up Redis.
    if is_pending_setup {
        set_mfa_secret(&state.db, auth_user.id, Some(&encrypted_secret))
            .await
            .map_err(|e| AuthError::Internal(format!("Failed to persist MFA secret: {e}")))?;

        let redis_key = format!("mfa:pending:{}", auth_user.id);
        let _ = state.redis.del::<(), _>(&redis_key).await;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA verification successful"
    })))
}

/// Disable MFA.
///
/// POST /auth/mfa/disable
#[utoipa::path(
    post,
    path = "/auth/mfa/disable",
    tag = "auth",
    request_body = MfaVerifyRequest,
    responses(
        (status = 200, description = "MFA disabled successfully"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, headers, request), fields(user_id = %auth_user.id))]
pub async fn mfa_disable(
    State(state): State<AppState>,
    auth_user: AuthUser,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<MfaVerifyRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    // Require MFA verification before disabling (security measure)
    // First verify the provided code is valid. A lock or a limiter outage
    // keeps its own answer; every other failure reads as a wrong code.
    let _verified = mfa_verify(
        State(state.clone()),
        auth_user.clone(),
        ConnectInfo(addr),
        headers,
        Json(request),
    )
    .await
    .map_err(|e| match e {
        AuthError::TooManyAttempts { .. }
        | AuthError::ServiceUnavailable
        | AuthError::Internal(_) => e,
        _ => AuthError::InvalidMfaCode,
    })?;

    // Clear MFA secret from database
    set_mfa_secret(&state.db, auth_user.id, None)
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to disable MFA: {e}")))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA disabled successfully"
    })))
}

// ============================================================================
// QR Login Handlers
// ============================================================================

/// Create a one-time QR login token for the authenticated user.
///
/// POST /auth/qr/create
///
/// The token is stored in Valkey with a 120-second TTL and can be redeemed
/// exactly once by an unauthenticated device (e.g. mobile app scanning a QR code).
#[tracing::instrument(skip(state))]
pub async fn qr_create(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<QrCreateResponse>> {
    let token = Uuid::now_v7().to_string();
    let redis_key = format!("qr_login:{token}");

    state
        .redis
        .set::<(), _, _>(
            &redis_key,
            auth.id.to_string(),
            Some(fred::types::Expiration::EX(120)),
            Some(fred::types::SetOptions::NX),
            false,
        )
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to store QR token: {e}")))?;

    tracing::info!(user_id = %auth.id, "Created QR login token");

    Ok(Json(QrCreateResponse {
        token,
        expires_in: 120,
    }))
}

/// Redeem a one-time QR login token for a full auth session.
///
/// POST /auth/qr/redeem
///
/// The token is consumed atomically via `GETDEL` so it cannot be reused.
/// Returns an access/refresh token pair identical to the login endpoint.
#[tracing::instrument(skip(state, jar, headers, body))]
pub async fn qr_redeem(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<QrRedeemRequest>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    let token_uuid: Uuid = body.token.parse().map_err(|_| {
        crate::observability::metrics::record_auth_login_attempt(false);
        AuthError::InvalidCredentials
    })?;
    let redis_key = format!("qr_login:{token_uuid}");

    // Atomic get-and-delete (one-use)
    let user_id_str: Option<String> = state
        .redis
        .getdel(&redis_key)
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to read QR token: {e}")))?;

    let user_id_str = user_id_str.ok_or_else(|| {
        crate::observability::metrics::record_auth_login_attempt(false);
        AuthError::InvalidCredentials
    })?;
    let user_id: Uuid = user_id_str
        .parse()
        .map_err(|_| AuthError::Internal("Invalid user ID in QR token".to_string()))?;

    // Verify user still exists (may have been deleted/banned during the token window)
    let user = find_user_by_id(&state.db, user_id).await?.ok_or_else(|| {
        crate::observability::metrics::record_auth_login_attempt(false);
        AuthError::InvalidCredentials
    })?;

    // A banned account cannot start a new session.
    if super::queries::is_user_globally_banned(&state.db, user_id).await? {
        return Err(AuthError::AccountBanned);
    }

    // Create the session (probation, CC4) with the redeeming client.
    let client = super::session_issue::SessionClient::from_request(&state, &headers, addr);
    let issued = super::session_issue::issue_session(
        &super::ctx::AuthCtx::from(&state),
        &super::session_issue::NewSession {
            user_id,
            method: super::session_issue::SignInMethod::LegacyQrRedeem,
            client: &client,
            client_class: super::client_class::ClientClass::from_headers(&headers),
            authenticator: None,
        },
    )
    .await?;
    crate::security_events::spawn_delivery(&state, issued.event_ids.clone());

    tracing::info!(user_id = %user_id, "QR login token redeemed");
    crate::observability::metrics::record_auth_login_attempt(true);

    let include_refresh_token = should_return_refresh_token(&headers);

    let jar = jar.add(issued.refresh_cookie(&state.config));

    Ok((
        jar,
        Json(issued.into_response(user.must_change_password, include_refresh_token)),
    ))
}
