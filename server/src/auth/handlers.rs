//! Authentication HTTP Handlers

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Multipart, Path, State};
use axum::http::header::USER_AGENT;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Json};
use chrono::{Duration, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;
use validator::Validate;

use super::backup_codes::{find_matching_backup_code, generate_backup_codes, BACKUP_CODE_COUNT};
use super::error::{AuthError, AuthResult};
use super::jwt::{generate_token_pair, validate_refresh_token};
use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use super::middleware::AuthUser;
use super::oidc::{append_collision_suffix, generate_username_from_claims, OidcFlowState};
use super::password::{hash_password, verify_password};
use crate::api::AppState;
use crate::db::{
    self, count_all_mfa_backup_codes, count_unused_mfa_backup_codes, create_password_reset_token,
    create_session, delete_mfa_backup_codes, delete_session_by_token_hash, email_exists,
    find_session_by_token_hash, find_user_by_email, find_user_by_external_id, find_user_by_id,
    find_user_by_username, find_valid_reset_token, get_auth_methods_allowed,
    get_unused_mfa_backup_codes, invalidate_user_reset_tokens, is_setup_complete,
    mark_mfa_backup_code_used, set_mfa_secret, store_mfa_backup_codes, update_user_avatar,
    update_user_profile, username_exists, Session,
};
use crate::ratelimit::NormalizedIp;
use crate::util::format_file_size;
use crate::ws::broadcast_user_patch;

// ============================================================================
// Request/Response Types
// ============================================================================

/// Registration request.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RegisterRequest {
    /// Username (3-32 lowercase alphanumeric + underscore).
    #[validate(length(min = 3, max = 32), regex(path = "USERNAME_REGEX"))]
    pub username: String,
    /// Email address (optional).
    #[validate(email)]
    pub email: Option<String>,
    /// Password (8-128 characters).
    #[validate(length(min = 8, max = 128))]
    pub password: String,
    /// Display name (optional, defaults to username).
    #[validate(length(max = 64))]
    pub display_name: Option<String>,
}

/// Login request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// MFA code (required if MFA is enabled).
    pub mfa_code: Option<String>,
}

/// Token refresh request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RefreshRequest {
    /// Refresh token.
    pub refresh_token: String,
}

/// Logout request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LogoutRequest {
    /// Refresh token to invalidate.
    pub refresh_token: String,
}

/// Authentication response with tokens.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuthResponse {
    /// Access token (short-lived).
    pub access_token: String,
    /// Refresh token (long-lived).
    pub refresh_token: String,
    /// Access token expiry in seconds.
    pub expires_in: i64,
    /// Token type (always "Bearer").
    pub token_type: String,
    /// Whether server setup is required.
    pub setup_required: bool,
}

/// User profile response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserProfile {
    /// User ID.
    pub id: String,
    /// Username.
    pub username: String,
    /// Display name.
    pub display_name: String,
    /// Email (if set).
    pub email: Option<String>,
    /// Avatar URL (if set).
    pub avatar_url: Option<String>,
    /// Online status.
    pub status: String,
    /// Whether MFA is enabled.
    pub mfa_enabled: bool,
    /// When the account is scheduled for permanent deletion (if requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion_scheduled_at: Option<String>,
}

/// MFA setup response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MfaSetupResponse {
    /// TOTP secret (base32-encoded).
    pub secret: String,
    /// QR code URL for authenticator apps.
    pub qr_code_url: String,
}

/// MFA backup codes response (shown exactly once upon generation).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MfaBackupCodesResponse {
    /// Plaintext backup codes (shown to user once; store them securely).
    pub codes: Vec<String>,
}

/// MFA backup code count response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MfaBackupCodeCountResponse {
    /// Number of remaining unused backup codes.
    pub remaining: i64,
    /// Total number of codes originally generated.
    pub total: i64,
}

/// MFA verification request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MfaVerifyRequest {
    /// 6-digit TOTP code.
    pub code: String,
}

/// Update profile request.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdateProfileRequest {
    /// New display name (1-64 characters).
    #[validate(length(min = 1, max = 64))]
    pub display_name: Option<String>,
    /// New email address (optional, set to null to clear).
    #[validate(email)]
    pub email: Option<String>,
}

/// Update profile response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdateProfileResponse {
    /// Updated fields.
    pub updated: Vec<String>,
}

// ============================================================================
// Regex for validation
// ============================================================================

/// Username validation regex (matches DB constraint).
static USERNAME_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^[a-z0-9_]{3,32}$").expect("valid username regex")
});

// ============================================================================
// Helper Functions
// ============================================================================

// Re-use the public hash_token function from parent module
use super::hash_token;

/// Extract User-Agent from headers (sanitized and truncated to 512 chars for DB storage).
fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| {
            // Sanitize: remove control characters (except whitespace) to prevent
            // injection attacks and display issues in admin panels
            // Then truncate to 512 chars to prevent DoS and match DB constraint
            s.chars()
                .filter(|c| !c.is_control() || c.is_whitespace())
                .take(512)
                .collect()
        })
}

// ============================================================================
// Handlers
// ============================================================================

/// Register a new local user.
///
/// **First User Behavior:** The first user to register is automatically granted
/// system admin permissions within the registration transaction. This is serialized
/// by a FOR UPDATE lock on the `server_config.setup_complete` row to prevent race
/// conditions where multiple concurrent registrations both see `user_count=0`.
///
/// After the first user is created, subsequent registrations will not receive admin
/// permissions unless explicitly granted by an existing admin.
///
/// POST /auth/register
#[utoipa::path(
    post,
    path = "/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "User registered successfully", body = AuthResponse),
        (status = 400, description = "Bad request"),
        (status = 409, description = "Username or email already taken"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, body), fields(username = %body.username))]
pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> AuthResult<Json<AuthResponse>> {
    // Validate input first
    body.validate()
        .map_err(|e| AuthError::Validation(e.to_string()))?;

    // Check if local auth is allowed
    let auth_methods = get_auth_methods_allowed(&state.db).await?;
    if !auth_methods.local {
        return Err(AuthError::AuthMethodDisabled);
    }

    // Check registration policy (fail-closed: deny registration if DB is unreachable)
    let reg_policy_value = db::get_config_value(&state.db, "registration_policy")
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                "Failed to read registration_policy config - denying registration (fail-closed)"
            );
            AuthError::Database(e)
        })?;
    let reg_policy = reg_policy_value.as_str().ok_or_else(|| {
        tracing::error!(
            actual_value = ?reg_policy_value,
            "registration_policy config value is not a string"
        );
        AuthError::Internal("Server configuration error".to_string())
    })?;
    if reg_policy != "open" {
        // Both "closed" and "invite_only" reject direct registration
        return Err(AuthError::RegistrationDisabled);
    }

    // Check username uniqueness (outside transaction - UNIQUE constraint will catch races)
    if username_exists(&state.db, &body.username).await? {
        return Err(AuthError::UserAlreadyExists);
    }

    // Check email uniqueness (if provided)
    if let Some(ref email) = body.email {
        if email_exists(&state.db, email).await? {
            return Err(AuthError::UserAlreadyExists);
        }
    }

    // Hash password
    let password_hash = hash_password(&body.password).map_err(|_| AuthError::PasswordHash)?;

    // Set display name (default to username if not provided)
    let display_name = body.display_name.as_deref().unwrap_or(&body.username);

    // Start transaction for atomic first-user detection and admin grant
    let mut tx = state.db.begin().await.map_err(|e| {
        tracing::error!(
            error = %e,
            username = %body.username,
            "Failed to start registration transaction"
        );
        e
    })?;

    // FOR UPDATE on setup_complete serializes concurrent registrations by acquiring
    // a row-level lock. Multiple transactions can BEGIN concurrently, but they will
    // block at this SELECT FOR UPDATE until the first transaction COMMITS or ROLLS BACK.
    // The lock is held for the entire transaction duration, preventing the race condition
    // where two concurrent registrations both see user_count=0 and both grant admin.
    let _lock = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            username = %body.username,
            "Failed to lock setup_complete config during registration"
        );
        e
    })?;

    // Now safely count users (serialized by the lock above)
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                username = %body.username,
                "Failed to count users during registration"
            );
            e
        })?;
    let is_first_user = user_count == 0;

    // Create user (inline to use transaction)
    let user = sqlx::query_as::<_, crate::db::User>(
        "INSERT INTO users (username, display_name, email, password_hash, auth_method)
         VALUES ($1, $2, $3, $4, 'local')
         RETURNING *",
    )
    .bind(&body.username)
    .bind(display_name)
    .bind(&body.email)
    .bind(password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            username = %body.username,
            "Failed to create user during registration - transaction will rollback"
        );
        e
    })?;

    // Grant system admin to first user
    if is_first_user {
        sqlx::query!(
            "INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)",
            user.id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(
                error = %e,
                user_id = %user.id,
                username = %user.username,
                "Failed to grant system admin to first user - transaction will rollback"
            );
            e
        })?;

        tracing::info!(
            user_id = %user.id,
            username = %user.username,
            "First user registered and granted system admin"
        );
    }

    // Generate tokens
    let tokens = generate_token_pair(
        user.id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )
    .map_err(|e| {
        tracing::error!(
            error = %e,
            user_id = %user.id,
            "Failed to generate tokens - transaction will rollback"
        );
        e
    })?;

    // Store refresh token session (inline to use transaction)
    let token_hash = hash_token(&tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    let user_agent = extract_user_agent(&headers);

    let ip_str = Some(addr.ip().to_string());
    sqlx::query(
        r"INSERT INTO sessions (user_id, token_hash, expires_at, ip_address, user_agent)
          VALUES ($1, $2, $3, $4::inet, $5)",
    )
    .bind(user.id)
    .bind(&token_hash)
    .bind(expires_at)
    .bind(ip_str.as_deref())
    .bind(user_agent.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        tracing::error!(
            error = %e,
            user_id = %user.id,
            "Failed to create session - transaction will rollback"
        );
        e
    })?;

    // Commit transaction
    tx.commit().await.map_err(|e| {
        tracing::error!(
            error = %e,
            user_id = %user.id,
            username = %user.username,
            "Failed to commit registration transaction - user account rolled back"
        );
        e
    })?;

    // Check if setup is complete
    let setup_complete = is_setup_complete(&state.db).await?;

    if !is_first_user {
        tracing::info!(user_id = %user.id, username = %user.username, "User registered");
    }

    Ok(Json(AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.access_expires_in,
        token_type: "Bearer".to_string(),
        setup_required: !setup_complete,
    }))
}

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
#[tracing::instrument(skip(state, body, normalized_ip), fields(username = %body.username))]
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    normalized_ip: Option<Extension<NormalizedIp>>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> AuthResult<Json<AuthResponse>> {
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

    // Find user by username
    let user = if let Some(u) = find_user_by_username(&state.db, &body.username).await? {
        u
    } else {
        record_failed_auth!();
        return Err(AuthError::InvalidCredentials);
    };

    // Verify password (only for local auth)
    let password_hash = if let Some(h) = user.password_hash.as_ref() {
        h
    } else {
        record_failed_auth!();
        return Err(AuthError::InvalidCredentials);
    };

    let valid =
        verify_password(&body.password, password_hash).map_err(|_| AuthError::PasswordHash)?;

    if !valid {
        record_failed_auth!();
        return Err(AuthError::InvalidCredentials);
    }

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
            Some("VoiceChat".to_string()),
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

    create_session(
        &state.db,
        user.id,
        &token_hash,
        expires_at,
        Some(&addr.ip().to_string()),
        user_agent.as_deref(),
    )
    .await?;

    // Clear failed auth counter on successful login
    if let (Some(ref rl), Some(Extension(ref nip))) = (&state.rate_limiter, &normalized_ip) {
        let _ = rl.clear_failed_auth(&nip.0).await;
    }

    // Check if setup is complete
    let setup_complete = is_setup_complete(&state.db).await?;

    tracing::info!(user_id = %user.id, setup_required = !setup_complete, "User logged in");

    Ok(Json(AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.access_expires_in,
        token_type: "Bearer".to_string(),
        setup_required: !setup_complete,
    }))
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
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Token refreshed successfully", body = AuthResponse),
        (status = 401, description = "Invalid or expired token"),
    ),
    security(()),
)]
#[tracing::instrument(skip(state, body))]
pub async fn refresh_token(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<RefreshRequest>,
) -> AuthResult<Json<AuthResponse>> {
    // Validate the refresh token (JWT validation)
    let claims = validate_refresh_token(&body.refresh_token, &state.config.jwt_public_key)?;

    // Parse user ID
    let user_id: Uuid = claims.sub.parse().map_err(|_| AuthError::InvalidToken)?;

    let token_hash = hash_token(&body.refresh_token);

    // Wrap session lookup, deletion, and creation in a transaction with FOR UPDATE
    // to prevent race conditions in token rotation.
    let mut tx = state.db.begin().await?;

    // Lock the session row to prevent concurrent refresh
    let session: Option<Session> = sqlx::query_as(
        r"
        SELECT id, user_id, token_hash, expires_at, host(ip_address) as ip_address, user_agent, created_at
        FROM sessions
        WHERE token_hash = $1 AND expires_at > NOW()
        FOR UPDATE
        ",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;

    let session = session.ok_or(AuthError::InvalidToken)?;

    // Verify session belongs to the user in the token
    if session.user_id != user_id {
        return Err(AuthError::InvalidToken);
    }

    // Verify user still exists
    let _user = find_user_by_id(&state.db, user_id)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    // Delete old session within the transaction
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(&token_hash)
        .execute(&mut *tx)
        .await?;

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
    let user_agent = extract_user_agent(&headers);

    sqlx::query(
        r"
        INSERT INTO sessions (user_id, token_hash, expires_at, ip_address, user_agent)
        VALUES ($1, $2, $3, $4::inet, $5)
        ",
    )
    .bind(user_id)
    .bind(&new_token_hash)
    .bind(expires_at)
    .bind(addr.ip().to_string())
    .bind(user_agent.as_deref())
    .execute(&mut *tx)
    .await?;

    // Commit the transaction — this is the atomic point
    tx.commit().await?;

    // Check if setup is complete
    let setup_complete = is_setup_complete(&state.db).await?;

    tracing::info!(user_id = %user_id, "Token refreshed");

    Ok(Json(AuthResponse {
        access_token: new_tokens.access_token,
        refresh_token: new_tokens.refresh_token,
        expires_in: new_tokens.access_expires_in,
        token_type: "Bearer".to_string(),
        setup_required: !setup_complete,
    }))
}

/// Logout and invalidate session.
///
/// POST /auth/logout
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    request_body = LogoutRequest,
    responses(
        (status = 200, description = "Logged out successfully"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body), fields(user_id = %auth_user.id))]
pub async fn logout(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(body): Json<LogoutRequest>,
) -> AuthResult<()> {
    // Verify the session belongs to the authenticated user before deleting
    let token_hash = hash_token(&body.refresh_token);
    let session = find_session_by_token_hash(&state.db, &token_hash)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    if session.user_id != auth_user.id {
        return Err(AuthError::InvalidToken);
    }

    delete_session_by_token_hash(&state.db, &token_hash).await?;

    tracing::info!(user_id = %auth_user.id, "User logged out");

    Ok(())
}

/// Get current user profile.
///
/// GET /auth/me
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Current user profile", body = UserProfile),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(fields(user_id = %auth_user.id))]
pub async fn get_profile(auth_user: AuthUser) -> Json<UserProfile> {
    Json(UserProfile {
        id: auth_user.id.to_string(),
        username: auth_user.username,
        display_name: auth_user.display_name,
        email: auth_user.email,
        avatar_url: auth_user.avatar_url,
        status: "online".to_string(),
        mfa_enabled: auth_user.mfa_enabled,
        deletion_scheduled_at: auth_user.deletion_scheduled_at.map(|dt| dt.to_rfc3339()),
    })
}

/// Upload user avatar.
///
/// POST /auth/me/avatar
#[utoipa::path(
    post,
    path = "/auth/me/avatar",
    tag = "auth",
    request_body(content = Vec<u8>, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Avatar uploaded successfully", body = UserProfile),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, multipart), fields(user_id = %auth_user.id))]
pub async fn upload_avatar(
    State(state): State<AppState>,
    auth_user: AuthUser,
    mut multipart: Multipart,
) -> AuthResult<Json<UserProfile>> {
    // Check if S3 is configured
    let s3 = state
        .s3
        .as_ref()
        .ok_or_else(|| AuthError::Internal("File storage not configured".to_string()))?;

    // Get the file from multipart
    let mut file_data = None;
    let mut filename = None;
    let mut content_type = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AuthError::Internal(format!("Multipart error: {e}")))?
    {
        if field.name() == Some("avatar") {
            filename = field.file_name().map(ToString::to_string);
            content_type = field.content_type().map(ToString::to_string);

            let data = field
                .bytes()
                .await
                .map_err(|e| AuthError::Internal(format!("Upload error: {e}")))?;

            file_data = Some(data);
            break; // Only process the first file
        }
    }

    let data = file_data.ok_or(AuthError::Validation("No avatar file provided".to_string()))?;

    // SECURITY: Validate file size before processing to prevent resource exhaustion
    if data.len() > state.config.max_avatar_size {
        return Err(AuthError::Validation(format!(
            "Avatar file too large ({}). Maximum size is {}",
            format_file_size(data.len()),
            format_file_size(state.config.max_avatar_size)
        )));
    }

    // Validate mime type from header
    let mime = content_type.unwrap_or_else(|| "application/octet-stream".to_string());

    if !mime.starts_with("image/") {
        return Err(AuthError::Validation("File must be an image".to_string()));
    }

    // Reject SVG files (potential XSS vector via embedded JavaScript)
    if mime.contains("svg") {
        return Err(AuthError::Validation(
            "SVG files are not allowed for avatars".to_string(),
        ));
    }

    // Validate actual file content using magic bytes (don't trust client-provided MIME type)
    let detected_format = image::guess_format(&data).map_err(|_| {
        AuthError::Validation(
            "Unable to detect image format. File may be corrupted or not a valid image."
                .to_string(),
        )
    })?;

    // Only allow safe raster formats
    match detected_format {
        image::ImageFormat::Png
        | image::ImageFormat::Jpeg
        | image::ImageFormat::Gif
        | image::ImageFormat::WebP => {}
        _ => {
            return Err(AuthError::Validation(format!(
                "Unsupported image format: {detected_format:?}. Only PNG, JPEG, GIF, and WebP are allowed."
            )));
        }
    }

    // Generate S3 key: avatars/{user_id}/{timestamp}_{filename}
    let timestamp = Utc::now().timestamp();
    let safe_filename = filename
        .unwrap_or_else(|| "avatar.png".to_string())
        .replace(|c: char| !c.is_alphanumeric() && c != '.', "_");

    let key = format!("avatars/{}/{}_{}", auth_user.id, timestamp, safe_filename);

    // Upload to S3
    s3.upload(&key, data.to_vec(), &mime)
        .await
        .map_err(|e| AuthError::Internal(format!("S3 upload failed: {e}")))?;

    // Construct public URL (assuming bucket is public or proxied)
    let bucket = &state.config.s3_bucket;
    let endpoint = &state.config.s3_endpoint;

    // Handle localhost vs cloud endpoint formatting
    let url = if let Some(ep) = endpoint
        .as_deref()
        .filter(|s| s.contains("localhost") || s.contains("127.0.0.1"))
    {
        // For RustFS/Local: endpoint/bucket/key
        format!("{ep}/{bucket}/{key}")
    } else if let Some(ep) = endpoint {
        // Custom endpoint (R2, etc): endpoint/bucket/key or bucket.endpoint/key
        // We'll stick to path style for safety if custom endpoint is used
        format!("{ep}/{bucket}/{key}")
    } else {
        // AWS S3 standard: https://bucket.s3.region.amazonaws.com/key
        // We assume standard path style for simplicity if no endpoint logic matches
        // or just construct a relative path if proxied
        format!("/{bucket}/{key}")
    };

    // Update user in DB
    let user = update_user_avatar(&state.db, auth_user.id, Some(&url))
        .await
        .map_err(|e| AuthError::Internal(format!("Database update failed: {e}")))?;

    // Convert status to string
    let status_str = match user.status {
        crate::db::UserStatus::Online => "online",
        crate::db::UserStatus::Away => "away",
        crate::db::UserStatus::Busy => "busy",
        crate::db::UserStatus::Offline => "offline",
    };

    Ok(Json(UserProfile {
        id: user.id.to_string(),
        username: user.username,
        display_name: user.display_name,
        email: user.email,
        avatar_url: user.avatar_url,
        status: status_str.to_string(),
        mfa_enabled: user.mfa_secret.is_some(),
        deletion_scheduled_at: user.deletion_scheduled_at.map(|dt| dt.to_rfc3339()),
    }))
}

// ============================================================================
// Profile Update
// ============================================================================

/// Update current user profile.
///
/// POST /auth/me
///
/// Updates `display_name` and/or email, then broadcasts a patch event
/// to all subscribers so they see the changes in real-time.
#[utoipa::path(
    post,
    path = "/auth/me",
    tag = "auth",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated successfully", body = UpdateProfileResponse),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, body), fields(user_id = %auth_user.id))]
pub async fn update_profile(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(body): Json<UpdateProfileRequest>,
) -> AuthResult<Json<UpdateProfileResponse>> {
    // Validate request
    body.validate()
        .map_err(|e| AuthError::Validation(e.to_string()))?;

    // Check if there's anything to update
    if body.display_name.is_none() && body.email.is_none() {
        return Err(AuthError::Validation("No fields to update".to_string()));
    }

    // Check email uniqueness if changing email
    if let Some(ref email) = body.email {
        if email_exists(&state.db, email)
            .await
            .map_err(AuthError::Database)?
        {
            // Check if it's the same user's email
            let current_user = find_user_by_id(&state.db, auth_user.id)
                .await
                .map_err(AuthError::Database)?
                .ok_or_else(|| AuthError::NotFound("User".to_string()))?;

            if current_user.email.as_ref() != Some(email) {
                return Err(AuthError::EmailTaken);
            }
        }
    }

    // Build diff for patch event before update
    let mut diff = serde_json::Map::new();
    let mut updated_fields = Vec::new();

    if let Some(ref display_name) = body.display_name {
        diff.insert("display_name".to_string(), serde_json::json!(display_name));
        updated_fields.push("display_name".to_string());
    }
    if let Some(ref email) = body.email {
        diff.insert("email".to_string(), serde_json::json!(email));
        updated_fields.push("email".to_string());
    }

    // Update database
    let _updated_user = update_user_profile(
        &state.db,
        auth_user.id,
        body.display_name.as_deref(),
        body.email.as_ref().map(|e| Some(e.as_str())),
    )
    .await
    .map_err(AuthError::Database)?;

    // Broadcast patch event to subscribers
    if !diff.is_empty() {
        if let Err(e) = broadcast_user_patch(
            &state.redis,
            auth_user.id,
            serde_json::Value::Object(diff.clone()),
        )
        .await
        {
            tracing::error!(
                error = %e,
                user_id = %auth_user.id,
                changed_fields = ?updated_fields,
                diff = ?diff,
                "Failed to broadcast user profile update to Redis - other clients may see stale data. Consider implementing retry queue."
            );
            // Don't fail the request, update was successful
        }
    }

    Ok(Json(UpdateProfileResponse {
        updated: updated_fields,
    }))
}

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
        Some("VoiceChat".to_string()),
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
        Some("VoiceChat".to_string()),
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

/// OIDC callback query parameters.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OidcCallbackQuery {
    pub code: String,
    pub state: String,
}

/// OIDC authorize query parameters.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OidcAuthorizeQuery {
    /// Optional redirect URI override (for Tauri localhost callback).
    pub redirect_uri: Option<String>,
}

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
    let auth_methods = get_auth_methods_allowed(&state.db).await?;
    if !auth_methods.oidc {
        return Err(AuthError::AuthMethodDisabled);
    }

    let oidc_manager = state
        .oidc_manager
        .as_ref()
        .ok_or(AuthError::OidcNotConfigured)?;

    // Verify provider exists
    if oidc_manager.get_provider_row(&provider).await.is_none() {
        return Err(AuthError::OidcProviderNotFound);
    }

    // Determine callback URL
    let callback_base = if let Some(ref redirect_uri) = query.redirect_uri {
        // Tauri flow: validate the redirect URI is a localhost callback
        let parsed = openidconnect::url::Url::parse(redirect_uri)
            .map_err(|_| AuthError::Validation("Invalid redirect_uri".to_string()))?;
        if matches!(
            (parsed.scheme(), parsed.host_str()),
            ("http", Some("localhost" | "127.0.0.1"))
        ) {
            redirect_uri.clone()
        } else {
            tracing::warn!(redirect_uri = %redirect_uri, "Rejected non-localhost redirect_uri");
            return Err(AuthError::Validation(
                "redirect_uri must be http://localhost or http://127.0.0.1".to_string(),
            ));
        }
    } else {
        // Browser flow: use the server's own callback endpoint
        format!(
            "{}/auth/oidc/callback",
            std::env::var("PUBLIC_URL").unwrap_or_else(|_| String::new())
        )
    };

    let (auth_url, csrf_state, nonce, pkce_verifier) = oidc_manager
        .generate_auth_url(&provider, &callback_base)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, provider = %provider, "Failed to generate OIDC auth URL");
            AuthError::Internal(format!("Failed to generate auth URL: {e}"))
        })?;

    // Store OIDC state in Redis with 600s TTL
    let state_hash = hex::encode(Sha256::digest(csrf_state.as_bytes()));
    let redis_key = format!("oidc:state:{state_hash}");
    let flow_state = OidcFlowState {
        slug: provider.clone(),
        pkce_verifier,
        nonce,
        redirect_uri: callback_base,
        created_at: Utc::now().timestamp(),
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

    tracing::info!(provider = %provider, "Redirecting to OIDC provider");
    Ok(Redirect::temporary(&auth_url).into_response())
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
#[tracing::instrument(skip(state, query))]
pub async fn oidc_callback(
    State(state): State<AppState>,
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

    // Composite external_id: "{provider_slug}:{subject}"
    let external_id = format!("{}:{}", flow_state.slug, user_info.subject);

    // User resolution
    let user = if let Some(existing) = find_user_by_external_id(&state.db, &external_id).await? {
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
            let _ = sqlx::query_scalar::<_, serde_json::Value>(
                "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE"
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to lock setup_complete during OIDC registration");
                AuthError::Database(e)
            })?;

            let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
                .fetch_one(&mut *tx)
                .await
                .map_err(AuthError::Database)?;
            let is_first_user = user_count == 0;

            let insert_result = sqlx::query_as::<_, crate::db::User>(
                "INSERT INTO users (username, display_name, email, auth_method, external_id, avatar_url)
                 VALUES ($1, $2, $3, 'oidc', $4, $5)
                 RETURNING *",
            )
            .bind(&username)
            .bind(&display_name)
            .bind(&user_info.email)
            .bind(&external_id)
            .bind(&user_info.avatar_url)
            .fetch_one(&mut *tx)
            .await;

            match insert_result {
                Ok(user) => {
                    // Grant admin to first user
                    if is_first_user {
                        sqlx::query!(
                            "INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)",
                            user.id
                        )
                        .execute(&mut *tx)
                        .await
                        .map_err(|e| {
                            tracing::error!(error = %e, user_id = %user.id, "Failed to grant admin to first OIDC user");
                            AuthError::Database(e)
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
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    // Username collision — tx dropped (implicit rollback), retry with suffix
                    tracing::debug!(username = %username, "Username collision during OIDC registration, retrying");
                }
                Err(e) => {
                    tracing::error!(error = %e, external_id = %external_id, "Failed to create OIDC user");
                    return Err(AuthError::Database(e));
                }
            }
        }

        new_user.ok_or_else(|| {
            tracing::error!(external_id = %external_id, "Failed to create OIDC user after 5 collision retries");
            AuthError::Internal("Username generation failed after retries".to_string())
        })?
    };

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
    create_session(&state.db, user.id, &token_hash, expires_at, None, None).await?;

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
        // Browser flow: return HTML with postMessage to opener
        // JSON-encode tokens to prevent any injection via token values
        let payload = serde_json::json!({
            "type": "oidc-callback",
            "access_token": tokens.access_token,
            "refresh_token": tokens.refresh_token,
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
        Ok((StatusCode::OK, axum::response::Html(html)).into_response())
    }
}

// ============================================================================
// Password Reset
// ============================================================================

/// Forgot password request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ForgotPasswordRequest {
    /// Email address of the account.
    pub email: String,
}

/// Reset password request.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ResetPasswordRequest {
    /// The reset token (raw, as received via email).
    pub token: String,
    /// The new password (8-128 characters).
    pub new_password: String,
}

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
    sqlx::query("UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1")
        .bind(reset_token.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, token_id = %reset_token.id, "Failed to mark reset token used");
            AuthError::Database(e)
        })?;

    // Update password
    sqlx::query("UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2")
        .bind(&password_hash)
        .bind(reset_token.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %reset_token.user_id, "Failed to update password");
            AuthError::Database(e)
        })?;

    // Delete all user sessions (force re-login everywhere)
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(reset_token.user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, user_id = %reset_token.user_id, "Failed to delete sessions");
            AuthError::Database(e)
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
