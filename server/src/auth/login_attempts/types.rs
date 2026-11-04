//! Wire types of the login attempts (C27 "Login attempts").

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `LoginAttempt.qr`: present only in a response that minted a new token.
#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct LoginQr {
    /// `kaiku://login/v1?o=<percent-encoded origin>&t=<b64u token>`.
    pub payload: String,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for LoginQr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginQr")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// `LoginAttempt` (C27).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct LoginAttempt {
    pub attempt_id: Uuid,
    /// `qr`, `password` or `oidc`.
    pub kind: String,
    /// `open`, `claimed`, `approved`, `consumed`, `denied`, `reported`,
    /// `cancelled` or `expired`.
    pub state: String,
    pub expires_at: DateTime<Utc>,
    pub qr: Option<LoginQr>,
    /// "Phone not at hand?": the account has an active TOTP (F3/F4 only).
    pub totp_available: bool,
}

/// `LoginStep` (C27): the answer to a correct first factor.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct LoginStep {
    /// `confirmation_required` or `totp_required`.
    pub status: String,
    pub attempt: LoginAttempt,
}

/// `POST /auth/login-attempts` body.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct CreateAttemptRequest {
    /// Native clients: S256 of their verifier (43 b64u characters).
    #[serde(default)]
    pub client_challenge: Option<String>,
}

/// Body of the PC-side attempt routes (`poll`, `collect`, `cancel`, `totp`).
#[derive(Default, Deserialize, utoipa::ToSchema)]
pub struct AttemptRequest {
    /// Native clients: the verifier of the attempt's `client_challenge`.
    #[serde(default)]
    pub verifier: Option<String>,
    /// `poll`: wait up to this many seconds (0..25) for a change.
    #[serde(default)]
    pub wait: Option<u8>,
    /// `totp`: the authenticator code.
    #[serde(default)]
    pub code: Option<String>,
}

impl std::fmt::Debug for AttemptRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttemptRequest")
            .field("wait", &self.wait)
            .finish_non_exhaustive()
    }
}

/// `POST /api/approvals/claim` body: b64u of the 32 scanned QR bytes.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct ClaimRequest {
    pub token: String,
}

impl std::fmt::Debug for ClaimRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClaimRequest { .. }")
    }
}
