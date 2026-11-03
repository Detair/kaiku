//! `/api/approvals/*` (C26): device step-up challenges and the answer,
//! deny and release routes (the `enroll` arms in Task 11, `login` in L1b).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use super::context::{CanonicalContext, DeviceStepupContext};
use super::core::{
    issue, lock_for_answer, set_state, verify, ChallengeState, NewChallenge, Requester,
};
use super::devices::{key_device_session, KeyDeviceSession};
use super::error::ApprovalError;
use super::secrets::b64u_decode;
use super::signing::{Decision, Kind};
use super::types::{AnswerRequest, AnswerResponse, ApprovalChallenge, StepUpChallengeRequest};
use crate::api::AppState;
use crate::auth::ctx::AuthCtx;
use crate::auth::step_up::{grant_tx, StepUpAction, StepUpTarget, StepUpVia};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::AuthUser;

/// `device_stepup` challenges live 2 minutes (C26).
const STEPUP_TTL_SECS: i64 = 120;

/// The server's display label for a step-up target (signed as
/// `target_label`, so the phone shows exactly what it confirms).
async fn target_label(
    conn: &mut sqlx::PgConnection,
    action: StepUpAction,
    target: &StepUpTarget,
) -> Result<String, ApprovalError> {
    let StepUpTarget::Id(id) = target else {
        return Ok(match target {
            StepUpTarget::Others => "all other sessions".to_string(),
            _ => String::new(),
        });
    };
    let label: Option<String> = match action {
        StepUpAction::KeyDeviceRemove | StepUpAction::AdminAuthenticatorRemove => {
            crate::auth::strong::authenticator_label(conn, AuthenticatorRef::key_device(*id))
                .await?
        }
        StepUpAction::PasskeyRemove => {
            crate::auth::strong::authenticator_label(conn, AuthenticatorRef::passkey(*id)).await?
        }
        StepUpAction::AdminEnrollmentIssue
        | StepUpAction::AdminMayApprove
        | StepUpAction::AdminFactorSetState
        | StepUpAction::AdminRecoveryContactSet
        | StepUpAction::RecoveryContactSet => {
            sqlx::query_scalar("SELECT username FROM users WHERE id = $1 AND NOT is_system")
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?
        }
        StepUpAction::SessionsEnd => {
            sqlx::query_scalar::<_, Option<String>>("SELECT user_agent FROM sessions WHERE id = $1")
                .bind(id)
                .fetch_optional(&mut *conn)
                .await?
                .flatten()
                .map(|ua| crate::auth::ua_parser::parse_device_name(&ua))
        }
        _ => None,
    };
    Ok(label.unwrap_or_default())
}

/// Ask for a `device_stepup` challenge from the key device's own full
/// session (`{action, target}` → 201 `ApprovalChallenge`).
#[utoipa::path(
    post,
    path = "/api/approvals/step-up",
    tag = "approvals",
    request_body = StepUpChallengeRequest,
    responses((status = 201, body = ApprovalChallenge), (status = 403, description = "KEY_DEVICE_REQUIRED")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth, body), fields(user_id = %auth.id))]
pub async fn step_up(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<StepUpChallengeRequest>,
) -> Result<(StatusCode, Json<ApprovalChallenge>), ApprovalError> {
    let action = StepUpAction::parse(&body.action)
        .ok_or_else(|| ApprovalError::Validation("unknown action".into()))?;
    let target = StepUpTarget::parse(action, &body.target)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let dev: KeyDeviceSession = key_device_session(&mut tx, &auth).await?;
    crate::auth::guards::require_full(&auth)?;
    let sid = auth.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let context = CanonicalContext::DeviceStepup(DeviceStepupContext {
        sid,
        action: action.as_str().to_string(),
        target: target.canonical(),
        target_label: target_label(&mut tx, action, &target).await?,
    });
    let challenge = issue(
        &mut tx,
        &ctx,
        NewChallenge {
            context,
            approver_id: auth.id,
            subject_id: auth.id,
            device_id: dev.device.device_id,
            requester: Requester::Session(sid),
            display: serde_json::json!({}),
            transfer_hash: None,
            ttl: chrono::Duration::seconds(STEPUP_TTL_SECS),
        },
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(challenge)))
}

fn decision(s: &str) -> Result<Decision, ApprovalError> {
    match s {
        "approve" => Ok(Decision::Approve),
        "report" => Ok(Decision::Report),
        _ => Err(ApprovalError::Validation(
            "decision must be approve or report".into(),
        )),
    }
}

/// Answer a challenge with a UV signature (C26 answer results).
#[utoipa::path(
    post,
    path = "/api/approvals/{id}/answer",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Challenge id")),
    request_body = AnswerRequest,
    responses(
        (status = 200, body = AnswerResponse),
        (status = 400, description = "SIGNATURE_INVALID, ENROLL_CODE_WRONG or VALIDATION_ERROR"),
        (status = 404, description = "APPROVAL_NOT_FOUND"),
        (status = 409, description = "APPROVAL_STATE"),
        (status = 410, description = "APPROVAL_EXPIRED or ENROLLMENT_FAILED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth, body), fields(user_id = %auth.id, challenge_id = %id))]
pub async fn answer(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AnswerRequest>,
) -> Result<Json<AnswerResponse>, ApprovalError> {
    let decision = decision(&body.decision)?;
    let response = b64u_decode(&body.response)
        .ok_or_else(|| ApprovalError::Validation("response must be base64url".into()))?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let dev = key_device_session(&mut tx, &auth).await?;
    let row = match lock_for_answer(&mut tx, id, dev.device.device_id).await {
        Ok(row) => row,
        Err(ApprovalError::Expired) => {
            tx.commit().await?;
            return Err(ApprovalError::Expired);
        }
        Err(e) => return Err(e),
    };
    match row.kind() {
        Some(Kind::DeviceStepup) => {
            // A proof: approve only, from the session that asked for it.
            if decision != Decision::Approve || !response.is_empty() {
                return Err(ApprovalError::Validation(
                    "a proof is approved with an empty response".into(),
                ));
            }
            if row.requester_kind != "session" || Some(row.requester_id) != auth.session_id {
                return Err(ApprovalError::NotFound);
            }
            verify(
                &row,
                ctx.origin(),
                &dev.device.public_key,
                decision,
                &response,
                &body.signature,
            )?;
            let parsed = CanonicalContext::from_json(Kind::DeviceStepup, &row.context)
                .map_err(|_| ApprovalError::NotFound)?;
            let CanonicalContext::DeviceStepup(c) = parsed else {
                return Err(ApprovalError::NotFound);
            };
            let action = StepUpAction::parse(&c.action).ok_or(ApprovalError::NotFound)?;
            let target = StepUpTarget::parse(action, &c.target)?;
            set_state(
                &mut tx,
                row.id,
                &[ChallengeState::Issued],
                ChallengeState::Consumed,
            )
            .await?;
            let grant = grant_tx(
                &mut tx,
                &auth,
                action,
                &target,
                StepUpVia::Device,
                AuthenticatorRef::key_device(dev.device.id),
            )
            .await?;
            sqlx::query("UPDATE approval_devices SET last_used_at = NOW() WHERE id = $1")
                .bind(dev.device.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(Json(AnswerResponse {
                state: "consumed".into(),
                grant: Some(grant),
                enrollment: None,
            }))
        }
        Some(Kind::Enroll) => {
            let enrollment = super::enrollments::answer_enroll_tx(
                &mut tx,
                &ctx,
                &dev,
                &row,
                decision,
                &response,
                &body.signature,
            )
            .await;
            match enrollment {
                Ok((enrollment, after)) => {
                    tx.commit().await?;
                    super::enrollments::after_done(&state, after).await;
                    Ok(Json(AnswerResponse {
                        state: "consumed".into(),
                        grant: None,
                        enrollment: Some(serde_json::to_value(enrollment).unwrap_or_default()),
                    }))
                }
                // A wrong code spends the challenge and counts a try: keep it.
                Err(
                    e @ (ApprovalError::EnrollCodeWrong { .. } | ApprovalError::EnrollmentFailed),
                ) => {
                    tx.commit().await?;
                    super::enrollments::notify_issuer_after_failure(&state, row.requester_id).await;
                    Err(e)
                }
                Err(e) => Err(e),
            }
        }
        // `login` arrives with L1b; proofs answered at their own routes.
        _ => Err(ApprovalError::NotFound),
    }
}

/// The unsigned "No" (C26): `enroll` cancels the enrolment its issuer
/// started; `login` arrives with L1b.
#[utoipa::path(
    post,
    path = "/api/approvals/{id}/deny",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Challenge id")),
    responses((status = 204), (status = 404, description = "APPROVAL_NOT_FOUND")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id, challenge_id = %id))]
pub async fn deny(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let dev = key_device_session(&mut tx, &auth).await?;
    let row = match lock_for_answer(&mut tx, id, dev.device.device_id).await {
        Ok(row) => row,
        Err(ApprovalError::Expired) => {
            tx.commit().await?;
            return Err(ApprovalError::Expired);
        }
        Err(e) => return Err(e),
    };
    // Only `enroll` and `login` (L1b) can be denied.
    match row.kind() {
        Some(Kind::Enroll) => {
            set_state(
                &mut tx,
                row.id,
                &[ChallengeState::Issued],
                ChallengeState::Denied,
            )
            .await?;
            super::enrollments::cancel_tx(
                &mut tx,
                &ctx,
                row.requester_id,
                super::enrollments::EnrollmentCanceller::Issuer,
            )
            .await?;
            tx.commit().await?;
            super::enrollments::notify_issuer_after_failure(&state, row.requester_id).await;
            Ok(StatusCode::NO_CONTENT)
        }
        _ => Err(ApprovalError::NotFound),
    }
}

/// Back / dismissed (C26): only `login` claims return to `open` (L1b).
#[utoipa::path(
    post,
    path = "/api/approvals/{id}/release",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Challenge id")),
    responses((status = 204), (status = 404, description = "APPROVAL_NOT_FOUND")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id, challenge_id = %id))]
pub async fn release(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApprovalError> {
    let mut tx = state.db.begin().await?;
    let dev = key_device_session(&mut tx, &auth).await?;
    let _ = lock_for_answer(&mut tx, id, dev.device.device_id).await?;
    Err(ApprovalError::NotFound)
}
