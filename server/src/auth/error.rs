//! Authentication Error Types

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

/// Authentication error types.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Invalid credentials (wrong username/password).
    #[error("Invalid credentials")]
    InvalidCredentials,

    /// User not found.
    #[error("User not found")]
    UserNotFound,

    /// Resource not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Forbidden action.
    #[error("Forbidden")]
    Forbidden,

    /// User already exists (registration).
    #[error("Username or email already taken")]
    UserAlreadyExists,

    /// Email already taken by another user.
    #[error("Email already in use by another account")]
    EmailTaken,

    /// Invalid or expired token.
    #[error("Invalid or expired token")]
    InvalidToken,

    /// Token has expired.
    #[error("Token expired")]
    TokenExpired,

    /// Missing Authorization header.
    #[error("Missing authorization header")]
    MissingAuthHeader,

    /// Invalid authorization header format.
    #[error("Invalid authorization header format")]
    InvalidAuthHeader,

    /// MFA required but not provided.
    #[error("MFA verification required")]
    MfaRequired,

    /// Invalid MFA code.
    #[error("Invalid MFA code")]
    InvalidMfaCode,

    /// Email service is not available (SMTP not configured).
    #[error("Email service is not available")]
    EmailNotConfigured,

    /// Validation error.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Password hashing error.
    #[error("Password processing failed")]
    PasswordHash,

    /// Database error.
    #[error("Database error")]
    Database(#[from] sqlx::Error),

    /// JWT error.
    #[error("Token error")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    /// OIDC provider not found.
    #[error("OIDC provider not found")]
    OidcProviderNotFound,

    /// OIDC is not configured on this server.
    #[error("OIDC is not configured")]
    OidcNotConfigured,

    /// OIDC state parameter mismatch (CSRF protection).
    #[error("Invalid OIDC state parameter")]
    OidcStateMismatch,

    /// OIDC code exchange failed.
    #[error("OIDC authentication failed")]
    OidcCodeExchangeFailed(String),

    /// Desktop sign-in from an app too old for the hand-off code flow (no
    /// `client_challenge`); the app must be updated.
    #[error("Update the app to sign in with this provider")]
    OidcClientOutdated,

    /// Unknown, expired, already used or wrongly verified desktop hand-off code.
    #[error("Sign-in could not be completed. Please try again.")]
    OidcHandoffInvalid,

    /// Registration is disabled by server policy.
    #[error("Registration is disabled")]
    RegistrationDisabled,

    /// This authentication method is disabled.
    #[error("This authentication method is disabled")]
    AuthMethodDisabled,

    /// The account must set a new password before using this route.
    #[error("You must change your password before continuing")]
    PasswordChangeRequired,

    /// The account is banned from this server.
    #[error("This account has been banned")]
    AccountBanned,

    /// A system admin must use an elevated session for this action (same
    /// 403 `elevation_required` body as the admin middleware).
    #[error("This action requires an elevated session")]
    ElevationRequired,

    /// Refusing to unlink the account's only remaining login method.
    #[error("Cannot remove your only login method; set a password or link another account first")]
    CannotUnlinkLastIdentity,

    /// A dependency needed to authenticate (Valkey) is unavailable.
    #[error("Service temporarily unavailable")]
    ServiceUnavailable,

    /// Re-authentication needs a TOTP code because the account has MFA on.
    #[error("Your authenticator code is required")]
    MfaCodeRequired,

    /// Re-authentication is impossible: the account has no local password.
    #[error("This account has no password to confirm with")]
    ReauthUnavailable,

    /// The account has no password but can re-authenticate with a linked
    /// provider (`GET /auth/reauth/methods`).
    #[error("Confirm with your linked sign-in provider instead")]
    ReauthOidcRequired,

    /// The provider is not linked to this account or cannot confirm a fresh
    /// sign-in.
    #[error("This sign-in provider cannot be used to confirm your identity")]
    ReauthProviderUnavailable,

    /// Too many failed re-authentication attempts; try again later.
    #[error("Too many failed attempts. Try again later.")]
    TooManyAttempts {
        /// Seconds until attempts are accepted again.
        retry_after: u64,
    },

    /// Internal server error.
    #[error("Internal server error")]
    Internal(String),
}

impl From<crate::db::ValkeyUnavailable> for AuthError {
    fn from(_: crate::db::ValkeyUnavailable) -> Self {
        Self::ServiceUnavailable
    }
}

/// Error response body for JSON responses.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ErrorResponse {
    /// Machine-readable error code.
    pub error: String,
    /// Human-readable error message.
    pub message: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            Self::UserNotFound => (StatusCode::NOT_FOUND, "USER_NOT_FOUND"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::UserAlreadyExists => (StatusCode::CONFLICT, "USER_EXISTS"),
            Self::EmailTaken => (StatusCode::CONFLICT, "EMAIL_TAKEN"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "INVALID_TOKEN"),
            Self::TokenExpired => (StatusCode::UNAUTHORIZED, "TOKEN_EXPIRED"),
            Self::MissingAuthHeader => (StatusCode::UNAUTHORIZED, "MISSING_AUTH"),
            Self::InvalidAuthHeader => (StatusCode::UNAUTHORIZED, "INVALID_AUTH_HEADER"),
            Self::MfaRequired => (StatusCode::FORBIDDEN, "MFA_REQUIRED"),
            Self::InvalidMfaCode => (StatusCode::UNAUTHORIZED, "INVALID_MFA"),
            Self::EmailNotConfigured => (StatusCode::SERVICE_UNAVAILABLE, "EMAIL_NOT_CONFIGURED"),
            Self::Validation(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            Self::PasswordHash => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            Self::Jwt(_) => (StatusCode::UNAUTHORIZED, "TOKEN_ERROR"),
            Self::OidcProviderNotFound => (StatusCode::NOT_FOUND, "OIDC_PROVIDER_NOT_FOUND"),
            Self::OidcNotConfigured => (StatusCode::SERVICE_UNAVAILABLE, "OIDC_NOT_CONFIGURED"),
            Self::OidcStateMismatch => (StatusCode::BAD_REQUEST, "OIDC_STATE_MISMATCH"),
            Self::OidcCodeExchangeFailed(_) => (StatusCode::BAD_GATEWAY, "OIDC_EXCHANGE_FAILED"),
            Self::OidcClientOutdated => (StatusCode::BAD_REQUEST, "OIDC_CLIENT_OUTDATED"),
            Self::OidcHandoffInvalid => (StatusCode::BAD_REQUEST, "OIDC_HANDOFF_INVALID"),
            Self::RegistrationDisabled => (StatusCode::FORBIDDEN, "REGISTRATION_DISABLED"),
            Self::AuthMethodDisabled => (StatusCode::FORBIDDEN, "AUTH_METHOD_DISABLED"),
            Self::PasswordChangeRequired => (StatusCode::FORBIDDEN, "PASSWORD_CHANGE_REQUIRED"),
            Self::AccountBanned => (StatusCode::FORBIDDEN, "ACCOUNT_BANNED"),
            Self::ElevationRequired => (StatusCode::FORBIDDEN, "elevation_required"),
            Self::CannotUnlinkLastIdentity => (StatusCode::CONFLICT, "CANNOT_UNLINK_LAST_IDENTITY"),
            Self::ServiceUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"),
            Self::MfaCodeRequired => (StatusCode::BAD_REQUEST, "MFA_CODE_REQUIRED"),
            Self::ReauthUnavailable => (StatusCode::BAD_REQUEST, "REAUTH_UNAVAILABLE"),
            Self::ReauthOidcRequired => (StatusCode::BAD_REQUEST, "REAUTH_OIDC_REQUIRED"),
            Self::ReauthProviderUnavailable => {
                (StatusCode::BAD_REQUEST, "REAUTH_PROVIDER_UNAVAILABLE")
            }
            Self::TooManyAttempts { .. } => (StatusCode::TOO_MANY_REQUESTS, "TOO_MANY_ATTEMPTS"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };

        let body = Json(ErrorResponse {
            error: code.to_string(),
            message: self.to_string(),
        });

        let mut response = (status, body).into_response();
        if let Self::TooManyAttempts { retry_after } = self {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, retry_after.into());
        }
        response
    }
}

/// Result type for auth operations.
pub type AuthResult<T> = Result<T, AuthError>;
