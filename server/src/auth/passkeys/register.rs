//! Adding a passkey (C29 `register/*`) and the registration core L1b's
//! request, ticket and enrolment variants call (`begin` / `complete`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;
use webauthn_rs::prelude::CreationChallengeResponse;

use super::ceremony::{self, CeremonyKind, CeremonyRecord, CeremonyState};
use super::queries::{self, InsertError, NewPasskey, PasskeyRow, PasskeyState};
use super::rp::VerifiedRegistration;
use super::types::{CeremonyFinishRequest, CeremonyStart, PasskeyView, RegisterStartRequest};
use super::{failure, rp_of, MAX_ACTIVE_PASSKEYS};
use crate::api::AppState;
use crate::auth::guards::require_full;
use crate::auth::step_up::{self, StepUpAction, StepUpTarget};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::{AuthError, AuthResult, AuthUser};
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// How the registration was authorised (audit `via`). L1b appends
/// `Request`, `Ticket` and `Enrollment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationVia {
    StepUp,
}

/// What the ceremony is bound to besides the account. L1b appends
/// `Request`, `Ticket` and `Enrollment`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegistrationBinding {
    Session { session_id: Uuid },
}

impl RegistrationVia {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StepUp => "step_up",
        }
    }
}

/// The label rules (C29): trimmed, 1..=64 characters; `None` → "Passkey".
pub fn clean_label(raw: Option<&str>) -> AuthResult<String> {
    let Some(raw) = raw else {
        return Ok("Passkey".to_string());
    };
    let label = raw.trim();
    let n = label.chars().count();
    if n == 0 || n > 64 || label.chars().any(char::is_control) {
        return Err(AuthError::Validation(
            "label must be 1 to 64 characters".into(),
        ));
    }
    Ok(label.to_string())
}

/// Start a registration for `user_id` bound to `binding`:
/// `excludeCredentials` = every live credential, the account's user
/// handle, its user name and display name.
pub async fn begin(
    state: &AppState,
    user_id: Uuid,
    binding: RegistrationBinding,
    label: String,
) -> AuthResult<(Uuid, CreationChallengeResponse)> {
    let rp = rp_of(state)?;
    let handle = queries::user_handle(&state.db, user_id).await?;
    let (username, display_name): (String, String) =
        sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&state.db)
            .await?;
    let exclude = queries::list_live(&state.db, user_id)
        .await?
        .into_iter()
        .map(|r| r.credential_id.into())
        .collect();
    let (ccr, st) = rp
        .start_registration(handle, &username, &display_name, exclude)
        .map_err(failure)?;
    let ceremony_id = ceremony::new_ceremony_id();
    ceremony::put_new(
        &state.redis,
        ceremony_id,
        &CeremonyRecord {
            kind: CeremonyKind::Register {
                user_id,
                binding,
                label,
            },
            state: CeremonyState::Register(st),
            created_at: Utc::now().timestamp(),
        },
    )
    .await?;
    Ok((ceremony_id, ccr))
}

/// Store a verified registration inside the caller's transaction.
///
/// The caller has already locked the account (B.7 rule 1). At most 10 active passkeys
/// (409 `PASSKEY_LIMIT`), a duplicate credential → 409
/// `PASSKEY_ALREADY_REGISTERED`; an `active` row marks the first activity;
/// records `authenticator_added` (high).
pub async fn complete(
    conn: &mut PgConnection,
    user_id: Uuid,
    label: &str,
    verified: &VerifiedRegistration,
    passkey_state: PasskeyState,
    ready_at: Option<DateTime<Utc>>,
    via: RegistrationVia,
) -> AuthResult<PasskeyRow> {
    complete_recorded(conn, user_id, label, verified, passkey_state, ready_at, via)
        .await
        .map(|(row, _)| row)
}

/// [`complete`], also returning the `authenticator_added` event id for
/// delivery after commit (`security_events::spawn_delivery`).
pub async fn complete_recorded(
    conn: &mut PgConnection,
    user_id: Uuid,
    label: &str,
    verified: &VerifiedRegistration,
    passkey_state: PasskeyState,
    ready_at: Option<DateTime<Utc>>,
    via: RegistrationVia,
) -> AuthResult<(PasskeyRow, Uuid)> {
    if queries::active_count(&mut *conn, user_id).await? >= MAX_ACTIVE_PASSKEYS as i64 {
        return Err(AuthError::PasskeyLimit);
    }
    let row = queries::insert(
        conn,
        &NewPasskey {
            user_id,
            verified,
            label,
            state: passkey_state,
            ready_at,
        },
    )
    .await
    .map_err(|e| match e {
        InsertError::Duplicate => AuthError::PasskeyAlreadyRegistered,
        InsertError::Db(e) => AuthError::Database(e),
    })?;
    let mut ev = NewSecurityEvent::new(
        user_id,
        EventType::AuthenticatorAdded,
        Actor::User(user_id),
        serde_json::json!({
            "type": "passkey",
            "label": row.label,
            "aaguid": row.aaguid,
            "backup_eligible": row.backup_eligible,
            "via": via.as_str(),
        }),
    );
    ev.authenticator = Some(AuthenticatorRef::passkey(row.id));
    let event = record(conn, ev).await?;
    Ok((row, event))
}

/// Start adding a passkey: a full session and an open `passkey.add`
/// grant (checked, not consumed). Unknown body fields (an old client's
/// `reauth`) are ignored and never count as a step-up.
#[utoipa::path(
    post,
    path = "/auth/passkeys/register/start",
    tag = "passkeys",
    request_body = RegisterStartRequest,
    responses(
        (status = 200, body = CeremonyStart),
        (status = 400, description = "VALIDATION_ERROR (label)"),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
        (status = 409, description = "PASSKEY_LIMIT"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<RegisterStartRequest>,
) -> AuthResult<Json<CeremonyStart>> {
    rp_of(&state)?;
    require_full(&auth)?;
    let session_id = auth.session_id.ok_or(AuthError::InvalidToken)?;
    let mut tx = state.db.begin().await?;
    step_up::peek_tx(
        &mut tx,
        &auth,
        StepUpAction::PasskeyAdd,
        &StepUpTarget::None,
    )
    .await?;
    tx.commit().await?;
    if queries::active_count(&state.db, auth.id).await? >= MAX_ACTIVE_PASSKEYS as i64 {
        return Err(AuthError::PasskeyLimit);
    }
    let label = clean_label(body.label.as_deref())?;
    let (ceremony_id, ccr) = begin(
        &state,
        auth.id,
        RegistrationBinding::Session { session_id },
        label,
    )
    .await?;
    Ok(Json(CeremonyStart {
        ceremony_id,
        options: serde_json::to_value(&ccr).map_err(|e| AuthError::Internal(e.to_string()))?,
    }))
}

/// Finish adding a passkey → 201 `Passkey` (`active`); the `passkey.add`
/// grant is consumed here, under the account lock.
#[utoipa::path(
    post,
    path = "/auth/passkeys/register/finish",
    tag = "passkeys",
    request_body = CeremonyFinishRequest,
    responses(
        (status = 201, body = PasskeyView),
        (status = 400, description = "PASSKEY_CEREMONY_INVALID, PASSKEY_INVALID or PASSKEY_NOT_DISCOVERABLE"),
        (status = 403, description = "FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
        (status = 409, description = "PASSKEY_ALREADY_REGISTERED or PASSKEY_LIMIT"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn finish(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CeremonyFinishRequest>,
) -> AuthResult<(StatusCode, Json<PasskeyView>)> {
    let rp = rp_of(&state)?;
    require_full(&auth)?;
    let rec = ceremony::take(&state.redis, body.ceremony_id)
        .await?
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (
        CeremonyKind::Register {
            user_id,
            binding: RegistrationBinding::Session { session_id },
            label,
        },
        CeremonyState::Register(st),
    ) = (rec.kind, rec.state)
    else {
        return Err(AuthError::PasskeyCeremonyInvalid);
    };
    if user_id != auth.id || Some(session_id) != auth.session_id {
        return Err(AuthError::PasskeyCeremonyInvalid);
    }
    let reg = serde_json::from_value(body.credential).map_err(|_| AuthError::PasskeyInvalid)?;
    let verified = rp.finish_registration(&reg, &st).map_err(failure)?;

    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    step_up::consume_tx(
        &mut tx,
        &auth,
        StepUpAction::PasskeyAdd,
        &StepUpTarget::None,
    )
    .await?;
    let (row, added) = complete_recorded(
        &mut tx,
        auth.id,
        &label,
        &verified,
        PasskeyState::Active,
        None,
        RegistrationVia::StepUp,
    )
    .await?;
    let mut ids = vec![added];
    ids.push(
        record(
            &mut tx,
            NewSecurityEvent::new(
                auth.id,
                EventType::StepUpAction,
                Actor::User(auth.id),
                serde_json::json!({
                    "action": StepUpAction::PasskeyAdd.as_str(),
                    "target_label": row.label,
                }),
            ),
        )
        .await?,
    );
    tx.commit().await?;
    crate::security_events::spawn_delivery(&state, ids);
    tracing::info!(passkey_id = %row.id, "passkey added");
    Ok((StatusCode::CREATED, Json(PasskeyView::from(&row))))
}
