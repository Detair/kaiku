//! Authentication HTTP Handlers

use axum::{
    extract::{ConnectInfo, Multipart, Path, State},
    http::{header::USER_AGENT, HeaderMap},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use totp_rs::{Algorithm, Secret, TOTP};
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::db::{
    create_session, delete_session_by_token_hash, email_exists,
    find_session_by_token_hash, find_user_by_id, find_user_by_username, is_setup_complete,
    set_mfa_secret, update_user_avatar, update_user_profile, username_exists,
};
use crate::ws::broadcast_user_patch;
use crate::ratelimit::NormalizedIp;
use crate::util::format_file_size;

use super::error::{AuthError, AuthResult};
use super::jwt::{generate_token_pair, validate_refresh_token};
use super::mfa_crypto::{decrypt_mfa_secret, encrypt_mfa_secret};
use super::middleware::AuthUser;
use super::password::{hash_password, verify_password};

// ============================================================================
// Request/Response Types
// ============================================================================

/// Registration request.
#[derive(Debug, Deserialize, Validate)]
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
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// MFA code (required if MFA is enabled).
    pub mfa_code: Option<String>,
}

/// Token refresh request.
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    /// Refresh token.
    pub refresh_token: String,
}

/// Logout request.
#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    /// Refresh token to invalidate.
    pub refresh_token: String,
}

/// Authentication response with tokens.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
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
}

/// MFA setup response.
#[derive(Debug, Serialize)]
pub struct MfaSetupResponse {
    /// TOTP secret (base32-encoded).
    pub secret: String,
    /// QR code URL for authenticator apps.
    pub qr_code_url: String,
}

/// MFA verification request.
#[derive(Debug, Deserialize)]
pub struct MfaVerifyRequest {
    /// 6-digit TOTP code.
    pub code: String,
}

/// Update profile request.
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateProfileRequest {
    /// New display name (1-64 characters).
    #[validate(length(min = 1, max = 64))]
    pub display_name: Option<String>,
    /// New email address (optional, set to null to clear).
    #[validate(email)]
    pub email: Option<String>,
}

/// Update profile response.
#[derive(Debug, Serialize)]
pub struct UpdateProfileResponse {
    /// Updated fields.
    pub updated: Vec<String>,
}

// ============================================================================
// Regex for validation
// ============================================================================

lazy_static::lazy_static! {
    /// Username validation regex (matches DB constraint).
    static ref USERNAME_REGEX: regex::Regex = regex::Regex::new(r"^[a-z0-9_]{3,32}$").unwrap();
}

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
/// by a FOR UPDATE lock on the server_config.setup_complete row to prevent race
/// conditions where multiple concurrent registrations both see user_count=0.
///
/// After the first user is created, subsequent registrations will not receive admin
/// permissions unless explicitly granted by an existing admin.
///
/// POST /auth/register
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
        "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE"
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
    let user = sqlx::query_as!(
        crate::db::User,
        r#"INSERT INTO users (username, display_name, email, password_hash, auth_method)
           VALUES ($1, $2, $3, $4, 'local')
           RETURNING id, username, display_name, email, password_hash,
                     auth_method as "auth_method: _", external_id, avatar_url,
                     status as "status: _", mfa_secret, created_at, updated_at"#,
        body.username,
        display_name,
        body.email,
        password_hash
    )
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
          VALUES ($1, $2, $3, $4::inet, $5)"
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
            secret.to_bytes().unwrap(),
            Some("VoiceChat".to_string()),
            user.username.clone(),
        )
        .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;

        // Verify the code
        let is_valid = totp
            .check_current(mfa_code)
            .map_err(|e| AuthError::Internal(format!("Failed to verify TOTP code: {e}")))?;

        if !is_valid {
            record_failed_auth!();
            return Err(AuthError::InvalidMfaCode);
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
#[tracing::instrument(skip(state, body))]
pub async fn refresh_token(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<RefreshRequest>,
) -> AuthResult<Json<AuthResponse>> {
    // Validate the refresh token (JWT validation)
    let claims = validate_refresh_token(&body.refresh_token, &state.config.jwt_public_key)?;

    // Check if session exists in database (not revoked)
    let token_hash = hash_token(&body.refresh_token);
    let session = find_session_by_token_hash(&state.db, &token_hash)
        .await?
        .ok_or(AuthError::InvalidToken)?;

    // Parse user ID
    let user_id: Uuid = claims.sub.parse().map_err(|_| AuthError::InvalidToken)?;

    // Verify session belongs to the user in the token
    if session.user_id != user_id {
        return Err(AuthError::InvalidToken);
    }

    // Verify user still exists
    let _user = find_user_by_id(&state.db, user_id)
        .await?
        .ok_or(AuthError::UserNotFound)?;

    // Delete old session (token rotation)
    delete_session_by_token_hash(&state.db, &token_hash).await?;

    // Generate new token pair
    let new_tokens = generate_token_pair(
        user_id,
        &state.config.jwt_private_key,
        state.config.jwt_access_expiry,
        state.config.jwt_refresh_expiry,
    )?;

    // Store new refresh token session
    let new_token_hash = hash_token(&new_tokens.refresh_token);
    let expires_at = Utc::now() + Duration::seconds(state.config.jwt_refresh_expiry);
    let user_agent = extract_user_agent(&headers);

    create_session(
        &state.db,
        user_id,
        &new_token_hash,
        expires_at,
        Some(&addr.ip().to_string()),
        user_agent.as_deref(),
    )
    .await?;

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
#[tracing::instrument(skip(state, body), fields(user_id = %auth_user.id))]
pub async fn logout(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(body): Json<LogoutRequest>,
) -> AuthResult<()> {
    // Delete the session associated with the provided refresh token
    let token_hash = hash_token(&body.refresh_token);
    delete_session_by_token_hash(&state.db, &token_hash).await?;

    tracing::info!(user_id = %auth_user.id, "User logged out");

    Ok(())
}

/// Get current user profile.
///
/// GET /auth/me
pub async fn get_profile(auth_user: AuthUser) -> Json<UserProfile> {
    Json(UserProfile {
        id: auth_user.id.to_string(),
        username: auth_user.username,
        display_name: auth_user.display_name,
        email: auth_user.email,
        avatar_url: auth_user.avatar_url,
        status: "online".to_string(),
        mfa_enabled: auth_user.mfa_enabled,
    })
}

/// Upload user avatar.
///
/// POST /auth/me/avatar
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
    let mime = content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());

    if !mime.starts_with("image/") {
        return Err(AuthError::Validation("File must be an image".to_string()));
    }

    // Reject SVG files (potential XSS vector via embedded JavaScript)
    if mime.contains("svg") {
        return Err(AuthError::Validation("SVG files are not allowed for avatars".to_string()));
    }

    // Validate actual file content using magic bytes (don't trust client-provided MIME type)
    let detected_format = image::guess_format(&data)
        .map_err(|_| AuthError::Validation("Unable to detect image format. File may be corrupted or not a valid image.".to_string()))?;

    // Only allow safe raster formats
    match detected_format {
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::WebP => {}
        _ => {
            return Err(AuthError::Validation(format!(
                "Unsupported image format: {:?}. Only PNG, JPEG, GIF, and WebP are allowed.",
                detected_format
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
    let url = if endpoint.as_deref().map_or(false, |s| s.contains("localhost") || s.contains("127.0.0.1")) {
        // For MinIO/Local: endpoint/bucket/key
        // endpoint is Option, so unwrap safe because of check
        format!("{}/{}/{}", endpoint.as_ref().unwrap(), bucket, key)
    } else if let Some(ep) = endpoint {
        // Custom endpoint (R2, etc): endpoint/bucket/key or bucket.endpoint/key
        // We'll stick to path style for safety if custom endpoint is used
        format!("{}/{}/{}", ep, bucket, key)
    } else {
        // AWS S3 standard: https://bucket.s3.region.amazonaws.com/key
        // We assume standard path style for simplicity if no endpoint logic matches
        // or just construct a relative path if proxied
        format!("/{}/{}", bucket, key)
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
    }))
}

// ============================================================================
// Profile Update
// ============================================================================

/// Update current user profile.
///
/// POST /auth/me
///
/// Updates display_name and/or email, then broadcasts a patch event
/// to all subscribers so they see the changes in real-time.
pub async fn update_profile(
    State(state): State<AppState>,
    auth_user: AuthUser,
    Json(body): Json<UpdateProfileRequest>,
) -> AuthResult<Json<UpdateProfileResponse>> {
    // Validate request
    body.validate().map_err(|e| AuthError::Validation(e.to_string()))?;

    // Check if there's anything to update
    if body.display_name.is_none() && body.email.is_none() {
        return Err(AuthError::Validation("No fields to update".to_string()));
    }

    // Check email uniqueness if changing email
    if let Some(ref email) = body.email {
        if email_exists(&state.db, email).await.map_err(AuthError::Database)? {
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

    // Store encrypted secret in database
    set_mfa_secret(&state.db, auth_user.id, Some(&encrypted_secret))
        .await
        .map_err(|e| AuthError::Internal(format!("Failed to store MFA secret: {e}")))?;

    // Create TOTP instance for QR code
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_bytes().unwrap(),
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

/// Verify MFA code.
///
/// POST /auth/mfa/verify
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
        .ok_or_else(|| AuthError::UserNotFound)?;

    // Check if MFA is enabled
    let encrypted_secret = user
        .mfa_secret
        .ok_or_else(|| AuthError::Internal("MFA not enabled".to_string()))?;

    // Decrypt the secret
    let secret_str = decrypt_mfa_secret(&encrypted_secret, &key_bytes)
        .map_err(|e| AuthError::Internal(format!("Failed to decrypt MFA secret: {e}")))?;

    // Parse the secret
    let secret = Secret::Encoded(secret_str);

    // Create TOTP instance
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_bytes().unwrap(),
        Some("VoiceChat".to_string()),
        user.username,
    )
    .map_err(|e| AuthError::Internal(format!("Failed to create TOTP: {e}")))?;

    // Verify the code
    let is_valid = totp
        .check_current(&request.code)
        .map_err(|e| AuthError::Internal(format!("Failed to verify TOTP code: {e}")))?;

    if !is_valid {
        return Err(AuthError::InvalidMfaCode);
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA verification successful"
    })))
}

/// Disable MFA.
///
/// POST /auth/mfa/disable
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

    Ok(Json(serde_json::json!({
        "success": true,
        "message": "MFA disabled successfully"
    })))
}

/// Get available OIDC providers.
///
/// GET /auth/oidc/providers
pub async fn oidc_providers(State(_state): State<AppState>) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
}

/// Initiate OIDC authorization.
///
/// GET /auth/oidc/authorize/:provider
pub async fn oidc_authorize(
    State(_state): State<AppState>,
    Path(_provider): Path<String>,
) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
}

/// Handle OIDC callback.
///
/// GET /auth/oidc/callback
pub async fn oidc_callback(State(_state): State<AppState>) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
}
