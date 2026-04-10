//! Authentication DTOs
//!
//! Request and response types exchanged over the `/auth/*` HTTP API.
//! These are the wire types referenced by handlers and by the `OpenAPI`
//! schema — they intentionally live apart from the handler logic so other
//! modules can depend on them without pulling in the full handler module.

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::Validate;

/// Username validation regex (matches DB constraint).
pub(super) static USERNAME_REGEX: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^[a-z0-9_]{3,32}$").expect("valid username regex")
    });

/// Helper for distinguishing "field omitted" from "field set to null" in
/// partial update requests (serde's standard `Option<T>` cannot represent
/// this distinction on its own).
#[allow(clippy::option_option)]
fn deserialize_double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

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
#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// MFA code (required if MFA is enabled).
    pub mfa_code: Option<String>,
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginRequest")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("mfa_code", &self.mfa_code.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
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
    /// Refresh token (long-lived). Omitted for browser clients that receive
    /// the refresh token via an `HttpOnly` cookie instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
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
    /// Custom status message via REST (unused — custom status is handled via WebSocket).
    #[allow(dead_code)]
    #[serde(default, deserialize_with = "deserialize_double_option")]
    #[allow(clippy::option_option)]
    pub status_message: Option<Option<String>>,
}

/// Update profile response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdateProfileResponse {
    /// Updated fields.
    pub updated: Vec<String>,
}

/// Update password request.
#[derive(Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdatePasswordRequest {
    /// Current password.
    pub current_password: String,
    /// New password (8-128 characters).
    #[validate(length(min = 8, max = 128))]
    pub new_password: String,
    /// Whether to revoke all other sessions after password change.
    #[serde(default)]
    pub revoke_others: bool,
}

impl std::fmt::Debug for UpdatePasswordRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdatePasswordRequest")
            .field("current_password", &"[REDACTED]")
            .field("new_password", &"[REDACTED]")
            .field("revoke_others", &self.revoke_others)
            .finish()
    }
}

/// Information about a single active auth session.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AuthSessionInfo)]
pub struct SessionInfo {
    /// Session ID.
    pub id: Uuid,
    /// Friendly device description (e.g. "Chrome on Linux").
    pub device: String,
    /// IP address of the client.
    pub ip_address: Option<String>,
    /// City from `GeoIP` lookup.
    pub city: Option<String>,
    /// Country from `GeoIP` lookup.
    pub country: Option<String>,
    /// When the session was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the session expires.
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Whether this is the currently active session.
    pub is_current: bool,
}

/// Response containing the list of active auth sessions.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AuthSessionListResponse)]
pub struct SessionListResponse {
    /// Active sessions for the authenticated user.
    pub sessions: Vec<SessionInfo>,
}

/// Response for revoking all other sessions.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RevokeAllResponse {
    /// Number of sessions that were revoked.
    pub revoked_count: i64,
}

/// QR login token redemption request.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct QrRedeemRequest {
    /// One-time QR login token.
    pub token: String,
}

/// QR login token creation response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct QrCreateResponse {
    /// One-time QR login token.
    pub token: String,
    /// Token lifetime in seconds.
    pub expires_in: u64,
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
