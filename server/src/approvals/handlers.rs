//! `/api/approvals/*` (C26): device step-up challenges and the answer,
//! deny and release routes (the `enroll` arms in Task 11, `login` in L1b).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use super::context::{CanonicalContext, DeviceStepupContext};
use super::core::{
    head, issue, lock_for_answer, set_state, verify, ChallengeRow, ChallengeState, NewChallenge,
    Requester,
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
/// `target_label`, so the phone shows exactly what it confirms). Targets of
/// the caller's own account (its sessions and key devices) are read only
/// from that account; admin targets name any account, so `step_up` lets
/// only system admins get here with an `admin.*` action.
pub(crate) async fn target_label(
    conn: &mut sqlx::PgConnection,
    user_id: uuid::Uuid,
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
        StepUpAction::KeyDeviceRemove => {
            sqlx::query_scalar("SELECT label FROM approval_devices WHERE id = $1 AND user_id = $2")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&mut *conn)
                .await?
        }
        StepUpAction::AdminAuthenticatorRemove => {
            // A key device or a passkey id (C27 step-up table).
            match crate::auth::strong::authenticator_label(conn, AuthenticatorRef::key_device(*id))
                .await?
            {
                Some(label) => Some(label),
                None => {
                    crate::auth::strong::authenticator_label(conn, AuthenticatorRef::passkey(*id))
                        .await?
                }
            }
        }
        StepUpAction::PasskeyRemove => {
            sqlx::query_scalar(
                "SELECT label FROM passkeys WHERE id = $1 AND user_id = $2 AND state = 'active'",
            )
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?
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
        StepUpAction::SessionsEnd => sqlx::query_scalar::<_, Option<String>>(
            "SELECT user_agent FROM sessions WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?
        .flatten()
        .map(|ua| crate::auth::ua_parser::parse_device_name(&ua)),
        StepUpAction::IdentityUnlink => {
            sqlx::query_scalar(
                "SELECT provider_slug FROM user_identities WHERE id = $1 AND user_id = $2",
            )
            .bind(id)
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?
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
    responses(
        (status = 201, body = ApprovalChallenge),
        (status = 403, description = "KEY_DEVICE_REQUIRED, or NOT_ADMIN for an admin.* action"),
    ),
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
    // NOT_ADMIN before anything else for an admin.* action (as on
    // /api/admin/*), also before the key-device lookup.
    crate::auth::guards::require_admin_for_step_up(&state.db, &auth, action).await?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let dev: KeyDeviceSession = key_device_session(&mut tx, &auth).await?;
    crate::auth::guards::require_full(&auth)?;
    let sid = auth.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let context = CanonicalContext::DeviceStepup(DeviceStepupContext {
        sid,
        action: action.as_str().to_string(),
        target: target.canonical(),
        target_label: target_label(&mut tx, auth.id, action, &target).await?,
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

/// B.7 rule 1 for answer, deny and release: learn the challenge's accounts
/// without a lock, lock the caller's and the subject's accounts in one
/// statement, then the device and session rows (`key_device_session`), then
/// the challenge, bound to this device row's account and install id.
/// Revocations take the same order (accounts → device → sessions →
/// challenges), so neither side can wait for the other in a cycle.
async fn lock_challenge(
    conn: &mut sqlx::PgConnection,
    auth: &AuthUser,
    id: Uuid,
) -> Result<(KeyDeviceSession, ChallengeRow), ApprovalError> {
    super::devices::require_key_device_snapshot(auth)?;
    let Some((approver_id, subject_id)) = head(conn, id).await? else {
        return Err(ApprovalError::NotFound);
    };
    if approver_id != auth.id {
        return Err(ApprovalError::NotFound);
    }
    super::transition::lock_accounts(conn, &[auth.id, subject_id]).await?;
    let dev = key_device_session(conn, auth).await?;
    let row = lock_for_answer(conn, id, dev.device.user_id, dev.device.device_id).await?;
    Ok((dev, row))
}

/// Claim a login attempt with the scanned QR token (C26, SM1): from a key
/// device's own session with `may_approve` on → 201 `ApprovalChallenge`
/// (`login`).
#[utoipa::path(
    post,
    path = "/api/approvals/claim",
    tag = "approvals",
    request_body = crate::auth::login_attempts::types::ClaimRequest,
    responses(
        (status = 201, body = ApprovalChallenge),
        (status = 403, description = "KEY_DEVICE_REQUIRED or APPROVAL_NOT_ALLOWED"),
        (status = 404, description = "LOGIN_CODE_STALE"),
        (status = 409, description = "LOGIN_ATTEMPT_CLAIMED or LOGIN_ATTEMPT_OTHER_ACCOUNT"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth, body), fields(user_id = %auth.id))]
pub async fn claim(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<crate::auth::login_attempts::types::ClaimRequest>,
) -> Result<(StatusCode, Json<ApprovalChallenge>), ApprovalError> {
    let token = crate::auth::login_attempts::core::decode_token(&body.token)
        .ok_or(ApprovalError::LoginCodeStale)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let challenge =
        crate::auth::login_attempts::core::claim_tx(&mut tx, &ctx, &auth, &token).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(challenge)))
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
    let (dev, row) = match lock_challenge(&mut tx, &auth, id).await {
        Ok(locked) => locked,
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
        Some(Kind::Login) => {
            let result = crate::auth::login_attempts::core::answer_tx(
                &mut tx,
                &ctx,
                &dev,
                &row,
                decision,
                &response,
                &body.signature,
            )
            .await;
            match result {
                Ok((answer, events)) => {
                    tx.commit().await?;
                    crate::security_events::spawn_delivery(&state, events);
                    crate::security_events::notices::spawn_pending_delivery(&state);
                    Ok(Json(answer))
                }
                // The attempt ended: its challenge is void (keep that mark).
                Err(ApprovalError::State) => {
                    tx.commit().await?;
                    Err(ApprovalError::State)
                }
                Err(e) => Err(e),
            }
        }
        // Proofs are answered at their own routes.
        _ => Err(ApprovalError::NotFound),
    }
}

/// The unsigned "No" (C26): `enroll` cancels the enrolment its issuer
/// started; `login` denies the attempt.
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
    let (dev, row) = match lock_challenge(&mut tx, &auth, id).await {
        Ok(locked) => locked,
        Err(ApprovalError::Expired) => {
            tx.commit().await?;
            return Err(ApprovalError::Expired);
        }
        Err(e) => return Err(e),
    };
    // Only `enroll` and `login` (L1b) can be denied.
    match row.kind() {
        Some(Kind::Enroll) => {
            // Only the issuing device says "No" to its enrolment.
            super::enrollments::lock_issued_by(&mut tx, row.requester_id, &dev).await?;
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
        Some(Kind::Login) => {
            let _ = dev;
            match crate::auth::login_attempts::core::deny_tx(&mut tx, &row).await {
                Ok(()) | Err(ApprovalError::State) => {
                    tx.commit().await?;
                    Ok(StatusCode::NO_CONTENT)
                }
                Err(e) => Err(e),
            }
        }
        _ => Err(ApprovalError::NotFound),
    }
}

/// Back / dismissed (C26): only `login` claims return to `open`.
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
    match lock_challenge(&mut tx, &auth, id).await {
        // Keep the expiry mark, as answer and deny do.
        Err(ApprovalError::Expired) => {
            tx.commit().await?;
            Err(ApprovalError::Expired)
        }
        Err(e) => Err(e),
        Ok((_, row)) if row.kind() == Some(Kind::Login) => {
            match crate::auth::login_attempts::core::release_tx(&mut tx, &row).await {
                Ok(()) | Err(ApprovalError::State) => {
                    tx.commit().await?;
                    Ok(StatusCode::NO_CONTENT)
                }
                Err(e) => Err(e),
            }
        }
        Ok(_) => Err(ApprovalError::NotFound),
    }
}
