//! Passkey wire types (C29; `snake_case`, ids as UUID strings).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
