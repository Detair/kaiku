//! Authentication HTTP Handlers

use axum::{
    extract::{ConnectInfo, Path, State},
    http::{header::USER_AGENT, HeaderMap},
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use uuid::Uuid;
use validator::Validate;

use crate::api::AppState;
use crate::db::{
    create_session, create_user, delete_session_by_token_hash, email_exists,
    find_session_by_token_hash, find_user_by_id, find_user_by_username, username_exists,
};

use super::error::{AuthError, AuthResult};
use super::jwt::{generate_token_pair, validate_refresh_token};
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

/// Hash a refresh token for storage (we don't store raw tokens).
fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Extract User-Agent from headers.
fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(USER_AGENT)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
}

// ============================================================================
// Handlers
// ============================================================================

/// Register a new local user.
///
/// POST /auth/register
#[tracing::instrument(skip(state, body), fields(username = %body.username))]
pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<RegisterRequest>,
) -> AuthResult<Json<AuthResponse>> {
    // Validate input
    body.validate()
        .map_err(|e| AuthError::Validation(e.to_string()))?;

    // Check username uniqueness
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

    // Create user
    let user = create_user(
        &state.db,
        &body.username,
        display_name,
        body.email.as_deref(),
        &password_hash,
    )
    .await?;

    // Generate tokens
    let tokens = generate_token_pair(
        user.id,
        &state.config.jwt_secret,
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

    tracing::info!(user_id = %user.id, "User registered");

    Ok(Json(AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.access_expires_in,
        token_type: "Bearer".to_string(),
    }))
}

/// Login with username/password.
///
/// POST /auth/login
#[tracing::instrument(skip(state, body), fields(username = %body.username))]
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> AuthResult<Json<AuthResponse>> {
    // Find user by username
    let user = find_user_by_username(&state.db, &body.username)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;

    // Verify password (only for local auth)
    let password_hash = user
        .password_hash
        .as_ref()
        .ok_or(AuthError::InvalidCredentials)?;

    let valid = verify_password(&body.password, password_hash).map_err(|_| AuthError::PasswordHash)?;

    if !valid {
        return Err(AuthError::InvalidCredentials);
    }

    // Check MFA if enabled
    if user.mfa_secret.is_some() {
        if body.mfa_code.is_none() {
            return Err(AuthError::MfaRequired);
        }
        // TODO: Verify MFA code (Phase 2)
        // For now, MFA verification is not implemented
    }

    // Generate tokens
    let tokens = generate_token_pair(
        user.id,
        &state.config.jwt_secret,
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

    tracing::info!(user_id = %user.id, "User logged in");

    Ok(Json(AuthResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.access_expires_in,
        token_type: "Bearer".to_string(),
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
    let claims = validate_refresh_token(&body.refresh_token, &state.config.jwt_secret)?;

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
        &state.config.jwt_secret,
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

    tracing::info!(user_id = %user_id, "Token refreshed");

    Ok(Json(AuthResponse {
        access_token: new_tokens.access_token,
        refresh_token: new_tokens.refresh_token,
        expires_in: new_tokens.access_expires_in,
        token_type: "Bearer".to_string(),
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
        avatar_url: None, // TODO: Load from User model
        status: "online".to_string(),
        mfa_enabled: auth_user.mfa_enabled,
    })
}

// ============================================================================
// Phase 2 Stubs (Not Yet Implemented)
// ============================================================================

/// Update current user profile.
///
/// POST /auth/me
pub async fn update_profile(State(_state): State<AppState>) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
}

/// Setup MFA (TOTP).
///
/// POST /auth/mfa/setup
pub async fn mfa_setup(State(_state): State<AppState>) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
}

/// Verify MFA code.
///
/// POST /auth/mfa/verify
pub async fn mfa_verify(State(_state): State<AppState>) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
}

/// Disable MFA.
///
/// POST /auth/mfa/disable
pub async fn mfa_disable(State(_state): State<AppState>) -> AuthResult<()> {
    Err(AuthError::Internal("Not implemented".to_string()))
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
