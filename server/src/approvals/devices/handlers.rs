//! `/api/approval-devices` (C26).

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;
use uuid::Uuid;

use super::{
    after_revoke, find, key_device_session, revoke_tx, KeyDevice, KeyDeviceRow, RevokeReason,
};
use crate::api::AppState;
use crate::approvals::error::ApprovalError;
use crate::auth::ctx::AuthCtx;
use crate::auth::step_up::{consume_tx, StepUpAction, StepUpTarget};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::AuthUser;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// `GET /api/approval-devices` answer.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct KeyDeviceList {
    pub devices: Vec<KeyDevice>,
}

fn current(auth: &AuthUser) -> Option<AuthenticatorRef> {
    auth.session.as_ref().and_then(|s| s.authenticator)
}

/// The caller's key devices (any session, probation included).
#[utoipa::path(
    get,
    path = "/api/approval-devices",
    tag = "approvals",
    responses((status = 200, body = KeyDeviceList)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<KeyDeviceList>, ApprovalError> {
    let rows: Vec<KeyDeviceRow> = sqlx::query_as(
        "SELECT * FROM approval_devices WHERE user_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(auth.id)
    .fetch_all(&state.db)
    .await?;
    let me = current(&auth);
    Ok(Json(KeyDeviceList {
        devices: rows.iter().map(|r| r.to_wire(me)).collect(),
    }))
}

/// Remove a key device: full session + `key_device.remove`/id grant; never
/// the account's last strong authenticator. The own id is "Sign out and
/// remove this phone": its session ends with the cascade.
#[utoipa::path(
    delete,
    path = "/api/approval-devices/{id}",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Key device id")),
    responses(
        (status = 200, body = KeyDevice),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
        (status = 404, description = "KEY_DEVICE_UNKNOWN"),
        (status = 409, description = "LAST_STRONG_AUTHENTICATOR"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn remove(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<KeyDevice>, ApprovalError> {
    crate::auth::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    let device = find(&mut tx, id)
        .await?
        .filter(|d| d.user_id == auth.id)
        .ok_or(ApprovalError::KeyDeviceUnknown)?;
    consume_tx(
        &mut tx,
        &auth,
        StepUpAction::KeyDeviceRemove,
        &StepUpTarget::Id(id),
    )
    .await?;
    if device.state == "revoked" {
        return Err(ApprovalError::KeyDeviceRevoked);
    }
    crate::auth::strong::ensure_not_last_strong(&mut tx, auth.id, AuthenticatorRef::key_device(id))
        .await?;
    let was_current = current(&auth) == Some(AuthenticatorRef::key_device(id));
    let (mut revoked, mut events) =
        revoke_tx(&mut tx, &ctx, id, RevokeReason::Owner, Actor::User(auth.id)).await?;
    events.push(
        record(
            &mut tx,
            NewSecurityEvent::new(
                auth.id,
                EventType::StepUpAction,
                Actor::User(auth.id),
                serde_json::json!({
                    "action": StepUpAction::KeyDeviceRemove.as_str(),
                    "target_label": device.label,
                }),
            ),
        )
        .await?,
    );
    tx.commit().await?;
    revoked.is_current = was_current;
    let st = state.clone();
    let device_id = device.device_id;
    tokio::spawn(async move { after_revoke(&st, auth.id, device_id, events).await });
    Ok(Json(revoked))
}

/// The app reports that Android invalidated its key: that device's own live
/// session, no grant (it only removes).
#[utoipa::path(
    post,
    path = "/api/approval-devices/{id}/invalidated",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Key device id")),
    responses((status = 200, body = KeyDevice), (status = 403, description = "KEY_DEVICE_REQUIRED")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn invalidated(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<KeyDevice>, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    let dev = key_device_session(&mut tx, &auth).await?;
    if dev.device.id != id {
        return Err(ApprovalError::KeyDeviceRequired);
    }
    let (mut revoked, events) = revoke_tx(
        &mut tx,
        &ctx,
        id,
        RevokeReason::KeyInvalidated,
        Actor::User(auth.id),
    )
    .await?;
    tx.commit().await?;
    // Only the device's own session reports it.
    revoked.is_current = true;
    let st = state.clone();
    let device_id = dev.device.device_id;
    tokio::spawn(async move { after_revoke(&st, auth.id, device_id, events).await });
    Ok(Json(revoked))
}
