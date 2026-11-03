//! Passkey wire types (C29; `snake_case`, ids as UUID strings).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::queries::{PasskeyRow, PasskeyState};

/// A started ceremony: `options` = `{"publicKey": …}` (`WebAuthn` L3 JSON).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CeremonyStart {
    pub ceremony_id: Uuid,
    #[schema(value_type = Object)]
    pub options: serde_json::Value,
}

/// `POST /auth/passkeys/step-up/start`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StepUpStartRequest {
    /// A step-up action of the registry (C27 common rules).
    pub action: String,
    /// The canonical target: lowercase UUID, `others` or `""` (none).
    #[serde(default)]
    pub target: Option<String>,
}

/// A ceremony finish: `credential` = `PublicKeyCredential.toJSON()`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CeremonyFinishRequest {
    pub ceremony_id: Uuid,
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
}

/// `POST /auth/passkeys/register/start`.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct RegisterStartRequest {
    /// Trimmed, 1..=64 characters; `null` → "Passkey".
    #[serde(default)]
    pub label: Option<String>,
}

/// C29 `Passkey`. Never carries the credential or its public key.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[schema(as = Passkey)]
pub struct PasskeyView {
    pub id: Uuid,
    pub label: String,
    pub state: PasskeyState,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub ready_at: Option<DateTime<Utc>>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub transports: Vec<String>,
    pub aaguid: Option<Uuid>,
}

impl From<&PasskeyRow> for PasskeyView {
    fn from(r: &PasskeyRow) -> Self {
        Self {
            id: r.id,
            label: r.label.clone(),
            state: r.state,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
            ready_at: r.ready_at,
            backup_eligible: r.backup_eligible,
            backup_state: r.backup_state,
            transports: r.transports.clone(),
            aaguid: r.aaguid,
        }
    }
}

/// `GET /auth/passkeys`. `user_handle` is the b64u `WebAuthn` user id.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PasskeyListResponse {
    pub rp_id: String,
    pub user_handle: String,
    pub passkeys: Vec<PasskeyView>,
}

/// `PATCH /auth/passkeys/{id}`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RenameRequest {
    pub label: String,
}

/// `DELETE /auth/passkeys/{id}`: what the client needs for
/// `signalUnknownCredential` and `signalAllAcceptedCredentials` (b64u).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PasskeyRemovedResponse {
    pub rp_id: String,
    pub user_handle: String,
    pub credential_id: String,
    pub remaining_credential_ids: Vec<String>,
}
