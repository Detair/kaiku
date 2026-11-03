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
