//! List, rename and remove passkeys (C29 `GET`/`PATCH`/`DELETE
//! /auth/passkeys`).

use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use super::queries::{self, PasskeyState};
use super::register::clean_label;
use super::rp_of;
use super::types::{PasskeyListResponse, PasskeyRemovedResponse, PasskeyView, RenameRequest};
use crate::api::AppState;
use crate::approvals::secrets::b64u_encode;
use crate::auth::ctx::AuthCtx;
use crate::auth::guards::require_full;
use crate::auth::step_up::{self, StepUpAction, StepUpTarget};
use crate::auth::strong::{self, AuthenticatorRef};
use crate::auth::{AuthError, AuthResult, AuthUser};
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// The caller's `pending` and `active` passkeys, with the RP ID and user
/// handle the client needs for `signalAllAcceptedCredentials`.
#[utoipa::path(
    get,
    path = "/auth/passkeys",
    tag = "passkeys",
    responses((status = 200, body = PasskeyListResponse)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<PasskeyListResponse>> {
    let rp = rp_of(&state)?;
    let handle = queries::user_handle(&state.db, auth.id).await?;
    let passkeys = queries::list_live(&state.db, auth.id)
        .await?
        .iter()
        .map(PasskeyView::from)
        .collect();
    Ok(Json(PasskeyListResponse {
        rp_id: rp.rp_id().to_string(),
        user_handle: b64u_encode(handle.as_bytes()),
        passkeys,
    }))
}

/// Rename one of the caller's live passkeys (any session).
#[utoipa::path(
    patch,
    path = "/auth/passkeys/{id}",
    tag = "passkeys",
    params(("id" = Uuid, Path, description = "Passkey id")),
    request_body = RenameRequest,
    responses(
        (status = 200, body = PasskeyView),
        (status = 400, description = "VALIDATION_ERROR"),
        (status = 404, description = "Not the caller's live passkey"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, passkey_id = %id))]
pub async fn rename(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameRequest>,
) -> AuthResult<Json<PasskeyView>> {
    rp_of(&state)?;
    let label = clean_label(Some(&body.label))?;
    let mut tx = state.db.begin().await?;
    let row = queries::rename(&mut *tx, auth.id, id, &label)
        .await?
        .ok_or_else(|| AuthError::NotFound("passkey".into()))?;
    crate::permissions::write_audit_log(
        &mut *tx,
        auth.id,
        "auth.passkey.renamed",
        Some("passkey"),
        Some(id),
        Some(serde_json::json!({"label": label})),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(PasskeyView::from(&row)))
}

/// Remove one of the caller's active passkeys: a full session and a
/// `passkey.remove` grant for that passkey. Never the last strong
/// authenticator. Its sessions end and its grants are voided.
#[utoipa::path(
    delete,
    path = "/auth/passkeys/{id}",
    tag = "passkeys",
    params(("id" = Uuid, Path, description = "Passkey id")),
    responses(
        (status = 200, body = PasskeyRemovedResponse),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
        (status = 404, description = "Not the caller's active passkey"),
        (status = 409, description = "LAST_STRONG_AUTHENTICATOR"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, passkey_id = %id))]
pub async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> AuthResult<Json<PasskeyRemovedResponse>> {
    let rp = rp_of(&state)?;
    require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    // Before the account lock: setting a missing handle writes the user row.
    let handle = queries::user_handle(&state.db, auth.id).await?;
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    let row = queries::find_own(&mut *tx, auth.id, id)
        .await?
        .filter(|r| r.state == PasskeyState::Active)
        .ok_or_else(|| AuthError::NotFound("passkey".into()))?;
    // Under the lock and before the grant: a refusal leaves the grant.
    strong::ensure_not_last_strong(&mut tx, auth.id, AuthenticatorRef::passkey(id)).await?;
    step_up::consume_tx(
        &mut tx,
        &auth,
        StepUpAction::PasskeyRemove,
        &StepUpTarget::Id(id),
    )
    .await?;
    queries::revoke(&mut tx, &ctx, auth.id, id, "removed_by_owner")
        .await?
        .ok_or_else(|| AuthError::NotFound("passkey".into()))?;
    let mut removed = NewSecurityEvent::new(
        auth.id,
        EventType::AuthenticatorRemoved,
        Actor::User(auth.id),
        serde_json::json!({"type": "passkey", "label": row.label, "by": "self"}),
    );
    removed.authenticator = Some(AuthenticatorRef::passkey(id));
    let mut events = vec![record(&mut tx, removed).await?];
    events.push(
        record(
            &mut tx,
            NewSecurityEvent::new(
                auth.id,
                EventType::StepUpAction,
                Actor::User(auth.id),
                serde_json::json!({
                    "action": StepUpAction::PasskeyRemove.as_str(),
                    "target_label": row.label,
                }),
            ),
        )
        .await?,
    );
    let remaining = queries::list_live(&mut *tx, auth.id)
        .await?
        .iter()
        .map(|r| b64u_encode(&r.credential_id))
        .collect();
    tx.commit().await?;
    crate::security_events::spawn_delivery(&state, events);
    tracing::info!("passkey removed");
    Ok(Json(PasskeyRemovedResponse {
        rp_id: rp.rp_id().to_string(),
        user_handle: b64u_encode(handle.as_bytes()),
        credential_id: b64u_encode(&row.credential_id),
        remaining_credential_ids: remaining,
    }))
}
