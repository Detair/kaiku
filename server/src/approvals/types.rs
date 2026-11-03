//! Wire types of the approvals routes (C26).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::step_up::StepUpGrant;

/// `ApprovalChallenge` (server → device).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ApprovalChallenge {
    pub challenge_id: Uuid,
    /// `login`, `enroll`, `device_login`, `device_stepup` or `enroll_pop`.
    pub kind: String,
    /// `qr`/`password`/`oidc` (login), `own`/`admin` (enroll), else null.
    pub variant: Option<String>,
    pub expires_at: DateTime<Utc>,
    /// b64u of M up to and including `context_hash`.
    pub signing_prefix: String,
    /// The canonical context the device displays and re-hashes.
    #[schema(value_type = Object)]
    pub context: serde_json::Value,
}

/// `POST /api/approvals/step-up` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StepUpChallengeRequest {
    pub action: String,
    #[serde(default)]
    pub target: String,
}

/// `POST /api/approvals/{id}/answer` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AnswerRequest {
    /// `approve` or `report`.
    pub decision: String,
    /// b64u of the response bytes (empty for proofs).
    #[serde(default)]
    pub response: String,
    /// b64u ASN.1 DER ECDSA signature over M.
    pub signature: String,
}

/// `POST /api/approvals/{id}/answer` result.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnswerResponse {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant: Option<StepUpGrant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<Object>)]
    pub enrollment: Option<serde_json::Value>,
}

/// `POST /auth/device-login/challenge` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeviceLoginChallengeRequest {
    pub user_id: Uuid,
    /// The app's per-install id.
    pub device_id: Uuid,
}

/// `POST /auth/device-login` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct DeviceLoginRequest {
    pub challenge_id: Uuid,
    /// b64u ASN.1 DER ECDSA signature over M (decision approve, empty response).
    pub signature: String,
}
