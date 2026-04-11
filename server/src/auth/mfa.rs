//! MFA (TOTP, backup codes) and QR login handlers.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::CookieJar;
use chrono::{Duration, Utc};
use fred::prelude::*;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;

use super::backup_codes::{find_matching_backup_code, generate_backup_codes, BACKUP_CODE_COUNT};
use super::error::{AuthError, AuthResult};
use super::helpers::{extract_user_agent, should_return_refresh_token};
use super::jwt::generate_token_pair;
use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use super::middleware::AuthUser;
use super::types::{
    AuthResponse, MfaBackupCodeCountResponse, MfaBackupCodesResponse, MfaSetupResponse,
    MfaVerifyRequest, QrCreateResponse, QrRedeemRequest,
};
use super::{cookies, hash_token};
use crate::api::AppState;
use crate::db::{
    count_all_mfa_backup_codes, count_unused_mfa_backup_codes, create_session,
    delete_mfa_backup_codes, find_user_by_id, get_unused_mfa_backup_codes, is_setup_complete,
    mark_mfa_backup_code_used, set_mfa_secret, store_mfa_backup_codes,
};

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

/// Verify MFA code (TOTP or backup code).
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
#[tracing::instrument(skip(state, request), fields(user_id = %auth_user.id))]
pub async fn mfa_verify(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<MfaVerifyRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    // Check if encryption key is configured
    let encryption_key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".to_string()))?;

    // Decode encryption key from hex
    let key_bytes = hex::decode(encryption_key)
        .map_err(|_| AuthError::Internal("Invalid MFA encryption key".to_string()))?;

    // Get user to retrieve encrypted MFA secret
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

    // Decrypt the secret
    let secret_str = decrypt_mfa_secret(&encrypted_secret, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to decrypt MFA secret: {e}")))?;

    // Count all backup codes (used + unused) BEFORE verification to detect first-time setup.
    // This must happen before any backup code is consumed, otherwise exhausting the last
    // code would be indistinguishable from never having had codes.
    let total_codes_before = count_all_mfa_backup_codes(&state.db, auth_user.id)
        .await
        .map_err(AuthError::Database)?;

    // Parse the secret
    let secret = Secret::Encoded(secret_str);

    // Create TOTP instance
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|_| AuthError::Internal("Invalid TOTP secret encoding".into()))?,
        Some("Kaiku".to_string()),
        user.username,
    )
    .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;

    // Try TOTP code first
    let totp_valid = totp
        .check_current(&request.code)
        .map_err(|e| AuthError::Internal(format!("Failed to verify TOTP code: {e}")))?;

    if !totp_valid {
        // TOTP failed — try backup code
        let backup_codes = get_unused_mfa_backup_codes(&state.db, auth_user.id)
            .await
            .map_err(AuthError::Database)?;

        let hashes: Vec<String> = backup_codes.iter().map(|c| c.code_hash.clone()).collect();
        if let Some(matched_idx) = find_matching_backup_code(&request.code, &hashes) {
            let used_code_id = backup_codes[matched_idx].id;
            mark_mfa_backup_code_used(&state.db, used_code_id)
                .await
                .map_err(AuthError::Database)?;
            tracing::info!(
                user_id = %auth_user.id,
                code_id = %used_code_id,
                "MFA backup code used for verification"
            );
        } else {
            return Err(AuthError::InvalidMfaCode);
        }
    }

    // If this was a pending setup, persist the secret to DB and clean up Redis.
    if is_pending_setup {
        set_mfa_secret(&state.db, auth_user.id, Some(&encrypted_secret))
            .await
            .map_err(|e| AuthError::Internal(format!("Failed to persist MFA secret: {e}")))?;

        let redis_key = format!("mfa:pending:{}", auth_user.id);
        let _ = state.redis.del::<(), _>(&redis_key).await;
    }

    // Auto-generate backup codes only on first-time setup completion.
    // We use total_codes_before (counted before verification) to distinguish
    // "never had codes" from "used the last code in this request".
    if total_codes_before == 0 {
        // First-time verify after setup — auto-generate backup codes
        let (plaintext_codes, hashes) = generate_backup_codes()
            .map_err(|e| AuthError::Internal(format!("Failed to generate backup codes: {e}")))?;

        store_mfa_backup_codes(&state.db, auth_user.id, &hashes)
            .await
            .map_err(AuthError::Database)?;

        tracing::info!(
            user_id = %auth_user.id,
            count = plaintext_codes.len(),
            "MFA backup codes auto-generated on first verify"
        );

        return Ok(Json(serde_json::json!({
            "success": true,
            "message": "MFA verification successful",
            "backup_codes": plaintext_codes
        })));
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
#[tracing::instrument(skip(state, request), fields(user_id = %auth_user.id))]
pub async fn mfa_disable(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(request): Json<MfaVerifyRequest>,
) -> AuthResult<Json<serde_json::Value>> {
    // Require MFA verification before disabling (security measure)
    // First verify the provided code is valid
    let verification_result =
        mfa_verify(State(state.clone()), auth_user.clone(), Json(request)).await;

    if verification_result.is_err() {
        return Err(AuthError::InvalidMfaCode);
    }

    // Clear MFA secret from database
    set_mfa_secret(&state.db, auth_user.id, None)
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to disable MFA: {e}")))?;

    // Delete all backup codes (no longer needed)
    delete_mfa_backup_codes(&state.db, auth_user.id)
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to delete backup codes: {e}")))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA disabled successfully"
    })))
}

/// Generate MFA backup codes (one-time display).
///
/// Generates 10 random 8-character alphanumeric backup codes, hashes them with
/// Argon2id, and stores only the hashes. Returns the plaintext codes to the
/// user exactly once — they must save them now.
///
/// Calling this endpoint again regenerates all codes, invalidating any previously
/// generated codes.
///
/// Requires MFA to be enabled on the account.
///
/// POST /auth/mfa/backup-codes
#[utoipa::path(
    post,
    path = "/auth/mfa/backup-codes",
    tag = "auth",
    responses(
        (status = 200, description = "Backup codes generated", body = MfaBackupCodesResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn mfa_generate_backup_codes(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<MfaBackupCodesResponse>> {
    // Verify MFA is enabled before generating backup codes
    let user = find_user_by_id(&state.db, auth_user.id)
        .await
        .map_err(AuthError::Database)?
        .ok_or(AuthError::UserNotFound)?;

    if user.mfa_secret.is_none() {
        return Err(AuthError::Internal(
            "MFA must be enabled before generating backup codes".to_string(),
        ));
    }

    // Generate 10 random backup codes and their Argon2id hashes
    let (plaintext_codes, hashes) = generate_backup_codes()
        .map_err(|e| AuthError::Internal(format!("Failed to generate backup codes: {e}")))?;

    // Store hashes (replaces any existing codes)
    store_mfa_backup_codes(&state.db, auth_user.id, &hashes)
        .await
        .map_err(AuthError::Database)?;

    tracing::info!(
        user_id = %auth_user.id,
        count = plaintext_codes.len(),
        "MFA backup codes generated"
    );

    Ok(Json(MfaBackupCodesResponse {
        codes: plaintext_codes,
    }))
}

/// Get remaining MFA backup code count.
///
/// GET /auth/mfa/backup-codes/count
#[utoipa::path(
    get,
    path = "/auth/mfa/backup-codes/count",
    tag = "auth",
    responses(
        (status = 200, description = "Backup code count", body = MfaBackupCodeCountResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state), fields(user_id = %auth_user.id))]
pub async fn mfa_backup_code_count(
    State(state): State<AppState>,
    auth_user: AuthUser,
) -> AuthResult<Json<MfaBackupCodeCountResponse>> {
    // Verify MFA is enabled
    let user = find_user_by_id(&state.db, auth_user.id)
        .await
        .map_err(AuthError::Database)?
        .ok_or(AuthError::UserNotFound)?;

    if user.mfa_secret.is_none() {
        return Err(AuthError::Validation("MFA is not enabled".to_string()));
    }

    let remaining = count_unused_mfa_backup_codes(&state.db, auth_user.id)
        .await
        .map_err(AuthError::Database)?;

    Ok(Json(MfaBackupCodeCountResponse {
        remaining,
        total: BACKUP_CODE_COUNT as i64,
    }))
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
#[tracing::instrument(skip(state, body))]
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
    let _user = find_user_by_id(&state.db, user_id).await?.ok_or_else(|| {
        crate::observability::metrics::record_auth_login_attempt(false);
        AuthError::InvalidCredentials
    })?;

    // Issue tokens
    let tokens = generate_token_pair(
        user_id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;

    // Compute refresh token hash for session tracking
    let token_hash = hash_token(&tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);

    // Create session with IP and user agent from the redeeming client
    let user_agent = extract_user_agent(&headers);
    create_session(
        &state.db,
        user_id,
        &token_hash,
        expires_at,
        Some(&addr.ip().to_string()),
        user_agent.as_deref(),
        None,
        None,
    )
    .await?;

    let setup_complete = is_setup_complete(&state.db).await?;

    tracing::info!(user_id = %user_id, "QR login token redeemed");
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
