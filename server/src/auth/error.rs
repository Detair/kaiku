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

    /// The caller is not a system admin (same 403 `NOT_ADMIN` body as the
    /// admin middleware); e.g. an `admin.*` step-up.
    #[error("System admin privileges required")]
    NotAdmin,

    /// A system admin must use an elevated session for this action (same
    /// 403 `ELEVATION_REQUIRED` body as the admin middleware).
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
    /// The action needs a full session (B.4).
    #[error("Sign in with a passkey or your phone to do this")]
    FullSessionRequired,

    /// The action needs a step-up grant for this action and target (F7).
    #[error("Confirm this action first")]
    StepUpRequired,

    /// A user cannot remove their own last strong authenticator (SM4).
    #[error("This is your last way to sign in securely; add another first")]
    LastStrongAuthenticator,

    /// A refresh token was used twice: the session has ended (B.4).
    #[error("This session was ended because its key was used twice")]
    SessionReused,

    /// The session lapsed (inactivity) or reached its absolute cap (B.4).
    #[error("This session has expired; sign in again")]
    SessionExpired,

    /// Passkeys are not configured on this server (test configs only).
    #[error("Passkeys are not available on this server")]
    PasskeysUnavailable,

    /// Unknown, used, expired or foreign passkey ceremony (C29).
    #[error("This passkey request has expired; please try again")]
    PasskeyCeremonyInvalid,

    /// Origin, RP ID, UV, BE, challenge or signature check failed (C29).
    #[error("The passkey could not be verified")]
    PasskeyInvalid,

    /// The credential is not (or no longer) registered (C29).
    #[error("This passkey was removed from Kaiku")]
    PasskeyUnknown,

    /// The passkey is `pending` until `ready_at` (DC11; never "unknown").
    #[error("This passkey is not ready yet")]
    PasskeyPending {
        ready_at: chrono::DateTime<chrono::Utc>,
    },

    /// The passkey belongs to another account than the one asked for.
    #[error("This passkey belongs to another account")]
    PasskeyUserMismatch,

    /// `credProps.rk == false`: the credential is not discoverable (DC9).
    #[error("This passkey cannot be used to sign in without a user name")]
    PasskeyNotDiscoverable,

    /// The credential is already registered.
    #[error("This passkey is already registered")]
    PasskeyAlreadyRegistered,

    /// At most 10 active passkeys per account (DC6).
    #[error("This account already has the maximum number of passkeys")]
    PasskeyLimit,

    /// A desktop hand-off redemption failed (C29 step 6).
    #[error("The sign-in from the browser could not be completed; please try again")]
    PasskeyHandoffInvalid,

    /// A desktop route was called without `X-Kaiku-Client: desktop`.
    #[error("Please update the Kaiku app")]
    ClientOutdated,

    /// A new password failed the policy (B.1): 400 `PASSWORD_REJECTED
    /// {reason}` (C27).
    #[error("Choose another password")]
    PasswordRejected(crate::auth::password_policy::PasswordRejected),

    /// F3's neutral answer: a wrong password, an unknown user and a closed
    /// password path look the same (401 `INVALID_CREDENTIALS`).
    #[error("Wrong username or password, or password sign-in is not available for this account. Use a passkey or 'Sign in with phone', or ask an admin.")]
    NeutralSignIn,

    /// TOTP is not set up, throttled or compromised (403 `TOTP_UNAVAILABLE`).
    #[error("Your authenticator code cannot be used for this account")]
    TotpUnavailable,

    /// An admin, the account itself or the system account as recovery
    /// contact (400 `RECOVERY_CONTACT_INVALID`, C28).
    #[error("This person cannot be a recovery contact")]
    RecoveryContactInvalid,

    /// An immediate admin action on another admin's account (403
    /// `ADMIN_TARGET_IS_ADMIN`, F6 rev 4).
    #[error("This cannot be done to another admin's account")]
    AdminTargetIsAdmin,

    /// Unknown or expired registration ticket (404
    /// `REGISTRATION_TICKET_INVALID`; not in C27's list, appended).
    #[error("This sign-up has expired; please register again")]
    TicketInvalid,

    /// Unknown attempt or wrong collection secret (C27).
    #[error("This sign-in is not known")]
    LoginAttemptUnknown,
    /// The attempt cannot be collected yet (not approved).
    #[error("This sign-in has not been confirmed yet")]
    LoginAttemptNotReady,
    /// The attempt has ended: 410 `LOGIN_ATTEMPT_ENDED {state}`.
    #[error("This sign-in has ended")]
    LoginAttemptEnded { state: String },

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

impl AuthError {
    /// The HTTP status and the machine-readable code of this error (the
    /// desktop passkey hand-off sends the code alone, C29 step 4).
    #[must_use]
    pub const fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
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
            Self::NotAdmin => (StatusCode::FORBIDDEN, "NOT_ADMIN"),
            Self::ElevationRequired => (StatusCode::FORBIDDEN, "ELEVATION_REQUIRED"),
            Self::CannotUnlinkLastIdentity => (StatusCode::CONFLICT, "CANNOT_UNLINK_LAST_IDENTITY"),
            Self::ServiceUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"),
            Self::MfaCodeRequired => (StatusCode::BAD_REQUEST, "MFA_CODE_REQUIRED"),
            Self::ReauthUnavailable => (StatusCode::BAD_REQUEST, "REAUTH_UNAVAILABLE"),
            Self::ReauthOidcRequired => (StatusCode::BAD_REQUEST, "REAUTH_OIDC_REQUIRED"),
            Self::ReauthProviderUnavailable => {
                (StatusCode::BAD_REQUEST, "REAUTH_PROVIDER_UNAVAILABLE")
            }
            Self::TooManyAttempts { .. } => (StatusCode::TOO_MANY_REQUESTS, "TOO_MANY_ATTEMPTS"),
            Self::FullSessionRequired => (StatusCode::FORBIDDEN, "FULL_SESSION_REQUIRED"),
            Self::StepUpRequired => (StatusCode::FORBIDDEN, "STEP_UP_REQUIRED"),
            Self::LastStrongAuthenticator => (StatusCode::CONFLICT, "LAST_STRONG_AUTHENTICATOR"),
            Self::SessionReused => (StatusCode::UNAUTHORIZED, "SESSION_REUSED"),
            Self::SessionExpired => (StatusCode::UNAUTHORIZED, "SESSION_EXPIRED"),
            Self::PasskeysUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "PASSKEYS_UNAVAILABLE"),
            Self::PasskeyCeremonyInvalid => (StatusCode::BAD_REQUEST, "PASSKEY_CEREMONY_INVALID"),
            Self::PasskeyInvalid => (StatusCode::BAD_REQUEST, "PASSKEY_INVALID"),
            Self::PasskeyUnknown => (StatusCode::BAD_REQUEST, "PASSKEY_UNKNOWN"),
            Self::PasskeyPending { .. } => (StatusCode::CONFLICT, "PASSKEY_PENDING"),
            Self::PasskeyUserMismatch => (StatusCode::BAD_REQUEST, "PASSKEY_USER_MISMATCH"),
            Self::PasskeyNotDiscoverable => (StatusCode::BAD_REQUEST, "PASSKEY_NOT_DISCOVERABLE"),
            Self::PasskeyAlreadyRegistered => (StatusCode::CONFLICT, "PASSKEY_ALREADY_REGISTERED"),
            Self::PasskeyLimit => (StatusCode::CONFLICT, "PASSKEY_LIMIT"),
            Self::PasskeyHandoffInvalid => (StatusCode::BAD_REQUEST, "PASSKEY_HANDOFF_INVALID"),
            Self::ClientOutdated => (StatusCode::BAD_REQUEST, "CLIENT_OUTDATED"),
            Self::PasswordRejected(_) => (StatusCode::BAD_REQUEST, "PASSWORD_REJECTED"),
            Self::NeutralSignIn => (StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS"),
            Self::TicketInvalid => (StatusCode::NOT_FOUND, "REGISTRATION_TICKET_INVALID"),
            Self::RecoveryContactInvalid => (StatusCode::BAD_REQUEST, "RECOVERY_CONTACT_INVALID"),
            Self::AdminTargetIsAdmin => (StatusCode::FORBIDDEN, "ADMIN_TARGET_IS_ADMIN"),
            Self::TotpUnavailable => (StatusCode::FORBIDDEN, "TOTP_UNAVAILABLE"),
            Self::LoginAttemptUnknown => (StatusCode::NOT_FOUND, "LOGIN_ATTEMPT_UNKNOWN"),
            Self::LoginAttemptNotReady => (StatusCode::CONFLICT, "LOGIN_ATTEMPT_NOT_READY"),
            Self::LoginAttemptEnded { .. } => (StatusCode::GONE, "LOGIN_ATTEMPT_ENDED"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();

        // C29: `PASSKEY_PENDING` carries `ready_at` next to `error`; C27:
        // `PASSWORD_REJECTED` carries `reason`.
        let mut response = if let Self::LoginAttemptEnded { state } = &self {
            (
                status,
                Json(serde_json::json!({
                    "error": code,
                    "message": self.to_string(),
                    "state": state,
                })),
            )
                .into_response()
        } else if let Self::PasswordRejected(reason) = &self {
            (
                status,
                Json(serde_json::json!({
                    "error": code,
                    "message": self.to_string(),
                    "reason": reason.as_str(),
                })),
            )
                .into_response()
        } else if let Self::PasskeyPending { ready_at } = &self {
            (
                status,
                Json(serde_json::json!({
                    "error": code,
                    "message": self.to_string(),
                    "ready_at": ready_at,
                })),
            )
                .into_response()
        } else {
            let body = Json(ErrorResponse {
                error: code.to_string(),
                message: self.to_string(),
            });
            (status, body).into_response()
        };
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
