//! Single-use passkey ceremonies in Valkey (`passkey:ceremony:{id}`).

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, PasskeyAuthentication, PasskeyRegistration,
};

use crate::auth::{AuthError, AuthResult};

/// A ceremony lives 5 minutes (the `WebAuthn` timeout).
pub const CEREMONY_TTL_SECS: i64 = 300;

/// A random v4 id (DC15; not v7, whose time prefix CC3 rules out).
#[must_use]
pub fn new_ceremony_id() -> Uuid {
    Uuid::new_v4()
}

/// What the ceremony is for, and whom it is bound to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CeremonyKind {
    /// `flow_id` is set for a desktop flow's ceremony: it finishes only there.
    SignIn {
        #[serde(default)]
        flow_id: Option<Uuid>,
    },
    /// Bound to the account, the session (`sid`) and one action + target.
    StepUp {
        user_id: Uuid,
        session_id: Uuid,
        action: crate::auth::step_up::StepUpAction,
        /// `StepUpTarget::canonical()`.
        target: String,
        /// Set for a desktop flow's ceremony (see `SignIn`).
        #[serde(default)]
        flow_id: Option<Uuid>,
    },
    /// Bound to the account and a session (L1b: request, ticket, enrolment).
    Register {
        user_id: Uuid,
        binding: super::register::RegistrationBinding,
        label: String,
    },
}

/// The `webauthn-rs` state of the ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CeremonyState {
    SignIn(DiscoverableAuthentication),
    StepUp(PasskeyAuthentication),
    Register(PasskeyRegistration),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyRecord {
    pub kind: CeremonyKind,
    pub state: CeremonyState,
    pub created_at: i64,
}

#[must_use]
pub fn key(id: Uuid) -> String {
    format!("passkey:ceremony:{id}")
}

/// Store a new ceremony (`SET NX EX 300`). A Valkey error fails closed.
pub async fn put_new(
    redis: &fred::clients::Client,
    id: Uuid,
    rec: &CeremonyRecord,
) -> AuthResult<()> {
    use fred::interfaces::KeysInterface;
    let json = serde_json::to_string(rec)
        .map_err(|e| AuthError::Internal(format!("ceremony state: {e}")))?;
    let set: Option<String> = redis
        .set(
            key(id),
            json,
            Some(fred::types::Expiration::EX(CEREMONY_TTL_SECS)),
            Some(fred::types::SetOptions::NX),
            false,
        )
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "passkey ceremony store unavailable");
            AuthError::ServiceUnavailable
        })?;
    if set.is_none() {
        // A v4 collision: never overwrite another ceremony.
        return Err(AuthError::Internal("ceremony id collision".to_string()));
    }
    Ok(())
}

/// Take a ceremony (`GETDEL`): it can be finished once.
pub async fn take(redis: &fred::clients::Client, id: Uuid) -> AuthResult<Option<CeremonyRecord>> {
    use fred::interfaces::KeysInterface;
    let raw: Option<String> = redis.getdel(key(id)).await.map_err(|e| {
        tracing::warn!(error = %e, "passkey ceremony store unavailable");
        AuthError::ServiceUnavailable
    })?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}
