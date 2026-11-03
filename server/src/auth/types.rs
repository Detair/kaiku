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
pub(crate) static USERNAME_REGEX: std::sync::LazyLock<regex::Regex> =
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
    /// Password (optional, B.1; 8–128 characters and the blocklist when
    /// given).
    #[serde(default)]
    pub password: Option<String>,
    /// Display name (optional, defaults to username).
    #[validate(length(max = 64))]
    pub display_name: Option<String>,
}

/// `POST /auth/register` answer (C27): the account and its registration
/// ticket (native clients; browsers get the cookie), never a session.
#[derive(Serialize, utoipa::ToSchema)]
pub struct RegisterResponse {
    pub user_id: Uuid,
    pub username: String,
    pub ticket_expires_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_ticket: Option<String>,
}

impl std::fmt::Debug for RegisterResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisterResponse")
            .field("user_id", &self.user_id)
            .finish_non_exhaustive()
    }
}

/// Login request.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Gone in v1.0 (C27): sent → 400 `CLIENT_OUTDATED`.
    #[serde(default)]
    pub mfa_code: Option<String>,
    /// Native clients: S256 of their verifier (the attempt's collection
    /// secret); browsers get the attempt cookie instead.
    #[serde(default)]
    pub client_challenge: Option<String>,
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginRequest")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish_non_exhaustive()
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
    /// Refresh token of the session to end. Optional: without it the session
    /// of the bearer access token is ended.
    #[serde(default)]
    pub refresh_token: Option<String>,
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
    /// Whether the user must set a new password before using the rest of the
    /// API (admin-created account still on its temporary password).
    pub password_change_required: bool,
    /// The session (stable across refreshes; the access token's `sid`).
    pub session_id: Uuid,
    /// The session's assurance, fixed at creation (C27).
    pub assurance: super::assurance::Assurance,
    /// When the session ends at the latest (absolute cap, B.4).
    pub absolute_expires_at: chrono::DateTime<chrono::Utc>,
}

/// User profile response.
#[allow(clippy::struct_excessive_bools)]
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
    /// Whether the user must set a new password before using the rest of the API.
    pub password_change_required: bool,
    /// The "Kaiku" system account (C28); always false for a signed-in user.
    pub is_system: bool,
    /// The calling session (C27); absent only outside a session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<CurrentSession>,
    /// Whether this account's key devices may confirm sign-ins and add
    /// phones (B.1; only an admin changes it).
    pub may_approve: bool,
}

/// `GET /auth/me` `session` (C27).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CurrentSession {
    pub id: Uuid,
    pub assurance: super::assurance::Assurance,
    pub auth_methods: Vec<String>,
    pub auth_time: chrono::DateTime<chrono::Utc>,
    /// `web`, `desktop` or `android`.
    pub client_class: String,
    pub absolute_expires_at: chrono::DateTime<chrono::Utc>,
    /// The key device whose own session this is, if any.
    pub key_device_id: Option<Uuid>,
}

impl From<&super::SessionMeta> for CurrentSession {
    fn from(s: &super::SessionMeta) -> Self {
        Self {
            id: s.id,
            assurance: s.assurance,
            auth_methods: s.auth_methods.clone(),
            auth_time: s.auth_time,
            client_class: s.client_class.as_str().to_string(),
            absolute_expires_at: s.absolute_expires_at,
            key_device_id: s
                .authenticator
                .filter(|a| a.kind == super::strong::AuthenticatorKind::KeyDevice)
                .map(|a| a.id),
        }
    }
}

/// MFA setup response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct MfaSetupResponse {
    /// TOTP secret (base32-encoded).
    pub secret: String,
    /// QR code URL for authenticator apps.
    pub qr_code_url: String,
}

/// `POST /auth/recovery-codes` (C27): the new set, shown once.
#[derive(Serialize, utoipa::ToSchema)]
pub struct RecoveryCodesResponse {
    /// 10 codes, `ABCDE-FGHIJ-KLM`.
    pub recovery_codes: Vec<String>,
}

impl std::fmt::Debug for RecoveryCodesResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryCodesResponse")
            .field("recovery_codes", &"[REDACTED]")
            .finish()
    }
}

/// `GET /auth/recovery-codes/count` (C27).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RecoveryCodesCountResponse {
    /// Unused codes of the current set.
    pub left: i64,
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

/// `POST /auth/me/password {new_password}` (C27). `current_password` and
/// `revoke_others` of older clients are ignored: the step-up grant
/// `password.set` authorises the change.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdatePasswordRequest {
    /// New password (8–128 characters, blocklist, B.1).
    pub new_password: String,
}

impl std::fmt::Debug for UpdatePasswordRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpdatePasswordRequest")
            .field("new_password", &"[REDACTED]")
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
    /// Assurance fixed at creation (C27).
    pub assurance: super::assurance::Assurance,
    /// `web`, `desktop` or `android`.
    pub client_class: String,
    /// How the session was created (C27).
    pub auth_methods: Vec<String>,
    /// The passkey or key device behind a full session (a key device's own
    /// session is shown as "Remove device").
    pub authenticator: Option<SessionAuthenticator>,
    /// Last sign-in or refresh.
    pub last_used_at: chrono::DateTime<chrono::Utc>,
    /// The absolute cap (B.4).
    pub absolute_expires_at: chrono::DateTime<chrono::Utc>,
}

/// `GET /auth/sessions` `authenticator` (C27).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SessionAuthenticator {
    pub id: Uuid,
    /// `passkey` or `key_device`.
    #[serde(rename = "type")]
    pub kind: String,
    pub label: Option<String>,
}

/// Response containing the list of active auth sessions.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[schema(as = AuthSessionListResponse)]
pub struct SessionListResponse {
    /// Active sessions for the authenticated user.
    pub sessions: Vec<SessionInfo>,
}

/// A single external (OIDC/OAuth) identity linked to the account.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IdentityInfo {
    /// Identity row ID (used to unlink).
    pub id: Uuid,
    /// Provider slug (e.g. "google").
    pub provider_slug: String,
    /// Human-readable provider name, resolved from the provider config.
    /// Falls back to the slug if the provider has since been removed.
    pub provider_name: String,
    /// Email reported by the provider at link time, if any.
    pub email: Option<String>,
    /// When the identity was linked.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// When the identity was last used to log in, if ever.
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Response containing the account's linked external identities.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct IdentityListResponse {
    /// External identities linked to the authenticated account.
    pub identities: Vec<IdentityInfo>,
}

/// Response for revoking all other sessions.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RevokeAllResponse {
    /// Number of sessions that were revoked.
    pub revoked_count: i64,
}

/// OIDC callback query parameters.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OidcCallbackQuery {
    /// Authorization code; absent when the provider reports an error.
    pub code: Option<String>,
    pub state: String,
    /// Provider error (e.g. `access_denied`). Logged only, never echoed.
    pub error: Option<String>,
}

/// OIDC authorize query parameters.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct OidcAuthorizeQuery {
    /// Optional desktop (Tauri) loopback address. The server hands the flow
    /// result back here after exchanging the code; it is never sent to the
    /// provider, which always redirects to `/auth/oidc/callback`.
    pub redirect_uri: Option<String>,
    /// Desktop login (with `redirect_uri`) only, and required there: S256
    /// challenge (unpadded base64url SHA-256, 43 characters) of a random
    /// verifier the desktop keeps. The loopback then receives a one-time
    /// `code` that only that verifier can redeem at
    /// `POST /auth/oidc/desktop/exchange`. Missing → 400 `OIDC_CLIENT_OUTDATED`.
    pub client_challenge: Option<String>,
    /// Desktop sign-in (C29 step 8): the loopback listener's port …
    pub loopback_port: Option<u16>,
    /// … and its per-flow path (43 b64u characters).
    pub callback_path: Option<String>,
    /// When `"json"`, return the provider authorization URL as a JSON body
    /// (`{"url": ...}`) instead of a 307 redirect. Used by the browser link
    /// flow, which calls this endpoint as an authenticated XHR (a top-level
    /// popup can't send the Bearer header) and then opens the URL itself.
    pub response: Option<String>,
}

/// Body of `POST /auth/oidc/desktop/exchange`.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct OidcHandoffExchangeRequest {
    /// One-time code from the desktop loopback hand-back (valid 60 s).
    pub code: String,
    /// The verifier whose S256 challenge was sent with the authorize request.
    pub verifier: String,
}

/// `GET /auth/reauth/methods`: how the caller can step up (C27, F7).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReauthMethodsResponse {
    /// A passkey can confirm the action (set by LP).
    pub passkey: bool,
    /// An active key device of the account can confirm it (`device_stepup`).
    pub device: bool,
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
