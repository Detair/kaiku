//! Passkey step-up (F7, C29): a passkey assertion by the session's own
//! account produces one action-bound grant on L1a's `step_up_grants`.

use axum::extract::State;
use axum::Json;
use webauthn_rs::prelude::{AuthenticationResult, PasskeyAuthentication, PublicKeyCredential};

use super::ceremony::{self, CeremonyKind, CeremonyRecord, CeremonyState};
use super::queries::{self, PasskeyRow, PasskeyState};
use super::types::{CeremonyFinishRequest, CeremonyStart, StepUpStartRequest};
use super::{audit, failure, rp_of, MAX_ACTIVE_PASSKEYS};
use crate::api::AppState;
use crate::auth::guards::require_full;
use crate::auth::step_up::{self, StepUpAction, StepUpGrant, StepUpTarget, StepUpVia};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::{AuthError, AuthResult, AuthUser};

/// The `step-up/start` checks, shared with the desktop `step_up` start:
/// a full session, a registry action and target, an own active passkey as
/// a `passkey.remove` target, at least one active passkey.
pub(super) async fn check_start(
    state: &AppState,
    auth: &AuthUser,
    action: &str,
    target: Option<&str>,
) -> AuthResult<(StepUpAction, StepUpTarget, Vec<PasskeyRow>)> {
    require_full(auth)?;
    let action = StepUpAction::parse(action)
        .ok_or_else(|| AuthError::Validation("unknown step-up action".into()))?;
    let target = StepUpTarget::parse(action, target.unwrap_or(""))?;
    if let (StepUpAction::PasskeyRemove, StepUpTarget::Id(id)) = (action, &target) {
        let own = queries::find_own(&state.db, auth.id, *id).await?;
        if own.is_none_or(|r| r.state != PasskeyState::Active) {
            return Err(AuthError::NotFound("passkey".into()));
        }
    }
    let mut active = queries::list_active(&state.db, auth.id).await?;
    if active.is_empty() {
        return Err(AuthError::Validation(
            "this account has no active passkey".into(),
        ));
    }
    active.truncate(MAX_ACTIVE_PASSKEYS);
    Ok((action, target, active))
}

/// Start a passkey step-up (`allowCredentials` = the caller's active
/// passkeys, DC3).
#[utoipa::path(
    post,
    path = "/auth/passkeys/step-up/start",
    tag = "passkeys",
    request_body = StepUpStartRequest,
    responses(
        (status = 200, body = CeremonyStart),
        (status = 400, description = "VALIDATION_ERROR"),
        (status = 403, description = "FULL_SESSION_REQUIRED"),
        (status = 404, description = "passkey.remove target is not an own active passkey"),
        (status = 503, description = "PASSKEYS_UNAVAILABLE or SERVICE_UNAVAILABLE"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<StepUpStartRequest>,
) -> AuthResult<Json<CeremonyStart>> {
    let rp = rp_of(&state)?;
    let (action, target, active) =
        check_start(&state, &auth, &body.action, body.target.as_deref()).await?;
    let creds: Vec<_> = active.iter().map(|r| r.credential.0.clone()).collect();
    let (rcr, st) = rp.start_step_up(&creds).map_err(failure)?;
    let ceremony_id = ceremony::new_ceremony_id();
    ceremony::put_new(
        &state.redis,
        ceremony_id,
        &CeremonyRecord {
            kind: CeremonyKind::StepUp {
                user_id: auth.id,
                session_id: auth.session_id.ok_or(AuthError::InvalidToken)?,
                action,
                target: target.canonical(),
            },
            state: CeremonyState::StepUp(st),
            created_at: chrono::Utc::now().timestamp(),
        },
    )
    .await?;
    Ok(Json(CeremonyStart {
        ceremony_id,
        options: serde_json::to_value(&rcr).map_err(|e| AuthError::Internal(e.to_string()))?,
    }))
}

/// Verify a step-up assertion for `user_id` (web and desktop): the user
/// handle and the credential must be the account's own; `pending` →
/// `PASSKEY_PENDING`; unknown or revoked → `PASSKEY_UNKNOWN`. Records the
/// use (counter, BS, last use).
pub(super) async fn verify(
    state: &AppState,
    user_id: uuid::Uuid,
    ceremony_state: &PasskeyAuthentication,
    credential: serde_json::Value,
) -> AuthResult<(PasskeyRow, AuthenticationResult)> {
    let rp = rp_of(state)?;
    let cred: PublicKeyCredential =
        serde_json::from_value(credential).map_err(|_| AuthError::PasskeyInvalid)?;
    if let Some(handle) = cred.get_user_unique_id() {
        let own = queries::user_handle(&state.db, user_id).await?;
        if handle != own.as_bytes() {
            return Err(AuthError::PasskeyUserMismatch);
        }
    }
    let row = queries::find_by_credential_id(&state.db, cred.get_credential_id())
        .await?
        .ok_or(AuthError::PasskeyUnknown)?;
    if row.user_id != user_id {
        return Err(AuthError::PasskeyUserMismatch);
    }
    match row.state {
        PasskeyState::Revoked => return Err(AuthError::PasskeyUnknown),
        PasskeyState::Pending => {
            return Err(AuthError::PasskeyPending {
                ready_at: row.ready_at.unwrap_or_else(chrono::Utc::now),
            })
        }
        PasskeyState::Active => {}
    }
    let result = match rp.finish_step_up(&cred, ceremony_state, row.backup_eligible) {
        Ok(r) => r,
        Err(f) => {
            if f == super::rp::PasskeyFailure::CounterRegressed {
                audit::counter_regression(state, &row).await;
            }
            return Err(failure(f));
        }
    };
    let mut passkey = row.credential.0.clone();
    passkey.update_credential(&result);
    queries::record_use(&state.db, row.id, &passkey, result.backup_state()).await?;
    Ok((row, result))
}

/// Grant the step-up: audit `auth.passkey.step_up` and L1a's `grant_tx`
/// (which locks and re-checks the session live and full).
pub(super) async fn grant(
    state: &AppState,
    auth: &AuthUser,
    action: StepUpAction,
    target: &StepUpTarget,
    passkey_id: uuid::Uuid,
) -> AuthResult<StepUpGrant> {
    let mut tx = state.db.begin().await?;
    crate::permissions::write_audit_log(
        &mut *tx,
        auth.id,
        "auth.passkey.step_up",
        Some("passkey"),
        Some(passkey_id),
        Some(serde_json::json!({"action": action.as_str(), "target": target.canonical()})),
        None,
    )
    .await?;
    let grant = step_up::grant_tx(
        &mut tx,
        auth,
        action,
        target,
        StepUpVia::Passkey,
        AuthenticatorRef::passkey(passkey_id),
    )
    .await?;
    tx.commit().await?;
    Ok(grant)
}

/// Finish a passkey step-up → 200 `StepUpGrant` (`via: "passkey"`).
#[utoipa::path(
    post,
    path = "/auth/passkeys/step-up/finish",
    tag = "passkeys",
    request_body = CeremonyFinishRequest,
    responses(
        (status = 200, body = StepUpGrant),
        (status = 400, description = "PASSKEY_CEREMONY_INVALID, PASSKEY_INVALID, PASSKEY_UNKNOWN or PASSKEY_USER_MISMATCH"),
        (status = 403, description = "FULL_SESSION_REQUIRED"),
        (status = 409, description = "PASSKEY_PENDING {ready_at}"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn finish(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CeremonyFinishRequest>,
) -> AuthResult<Json<StepUpGrant>> {
    rp_of(&state)?;
    require_full(&auth)?;
    // The ceremony is consumed whatever happens next.
    let rec = ceremony::take(&state.redis, body.ceremony_id)
        .await?
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (
        CeremonyKind::StepUp {
            user_id,
            session_id,
            action,
            target,
        },
        CeremonyState::StepUp(st),
    ) = (rec.kind, rec.state)
    else {
        return Err(AuthError::PasskeyCeremonyInvalid);
    };
    if user_id != auth.id || Some(session_id) != auth.session_id {
        return Err(AuthError::PasskeyCeremonyInvalid);
    }
    let target = StepUpTarget::parse(action, &target)?;
    let (row, _) = verify(&state, auth.id, &st, body.credential).await?;
    let grant = grant(&state, &auth, action, &target, row.id).await?;
    tracing::info!(action = action.as_str(), "passkey step-up granted");
    Ok(Json(grant))
}
