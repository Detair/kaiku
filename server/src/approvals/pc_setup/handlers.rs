//! "Set up this PC" routes (SM5 up to `code`).
//!
//! PC, every route bound to the requesting session's `sid` (another session
//! of the account gets 404): `POST /auth/pc-setups`, `GET
//! /auth/pc-setups/current`, `GET /auth/pc-setups/{id}?wait=`, `DELETE
//! /auth/pc-setups/{id}`. Phone, from a key device's own session: `POST
//! /api/pc-setups/claim`, `POST /api/pc-setups/{challenge_id}/answer` and
//! `/refuse`, `GET /api/pc-setups/{attempt_id}?wait=` (the scanning device
//! only); `POST /api/approvals/{id}/release` releases a claim as for `login`.
//!
//! No push: both sides learn the state by polling (long-poll ≤ 25 s, re-read
//! every 500 ms). The code is in the approving phone's answer and nowhere
//! else: never in a PC response, a log line or an audit row.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::transition::{self, Answered, AttemptRow};
use crate::api::AppState;
use crate::approvals::error::ApprovalError;
use crate::approvals::secrets::{b64u_decode, Secret256};
use crate::approvals::signing::Decision;
use crate::approvals::types::{AnswerRequest, ApprovalChallenge};
use crate::auth::ctx::AuthCtx;
use crate::auth::login_attempts::types::ClaimRequest;
use crate::auth::AuthUser;

/// The longest long-poll (as DC13).
pub const MAX_WAIT_SECS: u8 = 25;
/// Long-poll re-read interval (as DC13).
const POLL_STEP: std::time::Duration = std::time::Duration::from_millis(500);
/// Concurrent attempt long-polls per process; when they are taken the
/// answer comes at once (as the login attempts' Fable M3).
const MAX_LONG_POLLS: usize = 32;
static LONG_POLLS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_LONG_POLLS);

/// `POST /auth/pc-setups` → 201.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PcSetupStarted {
    pub attempt_id: Uuid,
    /// `kaiku://pcsetup/v1?o=<percent-encoded origin>&t=<43-char b64u token>`.
    pub qr_payload: String,
    /// When the PC's poll gets the next QR.
    pub rotates_at: DateTime<Utc>,
}

impl std::fmt::Debug for PcSetupStarted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcSetupStarted")
            .field("attempt_id", &self.attempt_id)
            .finish_non_exhaustive()
    }
}

/// The PC's view of its attempt.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PcSetupStatus {
    pub attempt_id: Uuid,
    /// SM5 state: `waiting`, `claimed`, `code`, `enrolled`, `completed`,
    /// `refused`, `reported`, `failed`, `cancelled` or `expired`.
    pub state: String,
    /// Why it ended, e.g. `session_ended`, `may_approve_off`, `refused`,
    /// `restarted`, `timeout`.
    pub reason: Option<String>,
    /// Only in a response that minted a new QR token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotates_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enrollment_id: Option<Uuid>,
}

impl std::fmt::Debug for PcSetupStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcSetupStatus")
            .field("attempt_id", &self.attempt_id)
            .field("state", &self.state)
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

/// The phone's view of the attempt it scanned.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PcSetupPhoneStatus {
    pub state: String,
    pub reason: Option<String>,
}

/// `answer` with `approve`: the code, to the phone only.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PcSetupCode {
    pub attempt_id: Uuid,
    /// Six digits, valid 5 minutes, never sent again.
    pub code: String,
    pub code_expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for PcSetupCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcSetupCode")
            .field("attempt_id", &self.attempt_id)
            .finish_non_exhaustive()
    }
}

/// `answer` with `report`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PcSetupReported {
    pub attempt_id: Uuid,
}

/// `?wait=` of the poll routes (seconds, 0..=25).
#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct WaitQuery {
    #[serde(default)]
    pub wait: Option<u8>,
}

fn status(origin: &str, a: &AttemptRow, minted: Option<&Secret256>) -> PcSetupStatus {
    PcSetupStatus {
        attempt_id: a.id,
        state: a.state.clone(),
        reason: a.reason.clone(),
        qr_payload: minted.map(|t| transition::qr_payload(origin, t)),
        rotates_at: minted.map(|_| transition::rotates_at(a)),
        enrollment_id: a.enrollment_id,
    }
}

/// Commit after a result: the effect on success; after a refusal the lazy
/// marks the locks left (an expired attempt, a void challenge), as the sweep
/// would. A database error rolls back.
async fn settle<T>(
    tx: Transaction<'_, Postgres>,
    result: Result<T, ApprovalError>,
) -> Result<T, ApprovalError> {
    match result {
        Err(e @ ApprovalError::Database(_)) => Err(e),
        other => {
            tx.commit().await?;
            other
        }
    }
}

/// Wait for a change of `id` (`rotation`: and mint due QR tokens).
async fn long_poll(
    state: &AppState,
    id: Uuid,
    first: &AttemptRow,
    wait: u8,
    rotation: bool,
) -> Result<(AttemptRow, Option<Secret256>), ApprovalError> {
    let ctx = AuthCtx::from(state);
    let permit = LONG_POLLS.try_acquire().ok();
    let wait = if permit.is_some() {
        wait.min(MAX_WAIT_SECS)
    } else {
        0
    };
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(u64::from(wait));
    loop {
        let (row, token) = transition::poll_step(&state.db, &ctx, id, rotation).await?;
        if token.is_some()
            || row.state != first.state
            || row.reason != first.reason
            || tokio::time::Instant::now() >= deadline
        {
            return Ok((row, token));
        }
        tokio::time::sleep(POLL_STEP).await;
    }
}

// ============================================================================
// PC
// ============================================================================

/// Start "Set up this PC" from a probation session (a live attempt of the
/// same session is restarted).
#[utoipa::path(
    post,
    path = "/auth/pc-setups",
    tag = "approvals",
    responses(
        (status = 201, body = PcSetupStarted),
        (status = 401, description = "INVALID_TOKEN: the session ended"),
        (status = 403, description = "PC_SETUP_UNAVAILABLE: a full session, or no active key device with may_approve on"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<(StatusCode, Json<PcSetupStarted>), ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let started = transition::start_tx(&mut tx, &ctx, &auth).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(PcSetupStarted {
            attempt_id: started.attempt.id,
            qr_payload: transition::qr_payload(ctx.origin(), &started.token),
            rotates_at: transition::rotates_at(&started.attempt),
        }),
    ))
}

/// The session's live attempt (after a reload), with a fresh QR while it is
/// `waiting`.
#[utoipa::path(
    get,
    path = "/auth/pc-setups/current",
    tag = "approvals",
    responses((status = 200, body = PcSetupStatus), (status = 404, description = "APPROVAL_NOT_FOUND")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn current(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<PcSetupStatus>, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let id = transition::current_of_session(&mut tx, &auth).await?;
    let result = transition::refresh_for_pc_tx(&mut tx, &ctx, &auth, id, true).await;
    let (row, token) = settle(tx, result).await?;
    Ok(Json(status(ctx.origin(), &row, token.as_ref())))
}

/// The PC's poll: the state, a new QR when the old one is due.
#[utoipa::path(
    get,
    path = "/auth/pc-setups/{id}",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Attempt id"), WaitQuery),
    responses((status = 200, body = PcSetupStatus), (status = 404, description = "APPROVAL_NOT_FOUND")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, attempt_id = %id))]
pub async fn poll(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<WaitQuery>,
) -> Result<Json<PcSetupStatus>, ApprovalError> {
    let first = {
        let mut conn = state.db.acquire().await?;
        transition::of_session(&mut conn, &auth, id).await?
    };
    let (row, token) = long_poll(&state, id, &first, q.wait.unwrap_or(0), true).await?;
    Ok(Json(status(
        AuthCtx::from(&state).origin(),
        &row,
        token.as_ref(),
    )))
}

/// The PC cancels its attempt (terminal attempts stay as they are).
#[utoipa::path(
    delete,
    path = "/auth/pc-setups/{id}",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Attempt id")),
    responses((status = 204), (status = 404, description = "APPROVAL_NOT_FOUND")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, attempt_id = %id))]
pub async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApprovalError> {
    let mut tx = state.db.begin().await?;
    let result = transition::cancel_tx(&mut tx, &auth, id).await;
    settle(tx, result).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Phone
// ============================================================================

/// Scan: a `pc_setup` challenge for the scanning key device.
#[utoipa::path(
    post,
    path = "/api/pc-setups/claim",
    tag = "approvals",
    request_body = ClaimRequest,
    responses(
        (status = 201, body = ApprovalChallenge),
        (status = 403, description = "KEY_DEVICE_REQUIRED or APPROVAL_NOT_ALLOWED"),
        (status = 404, description = "LOGIN_CODE_STALE: rotated, unknown or not live"),
        (status = 409, description = "PC_SETUP_OTHER_ACCOUNT or LOGIN_ATTEMPT_CLAIMED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn claim(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ClaimRequest>,
) -> Result<(StatusCode, Json<ApprovalChallenge>), ApprovalError> {
    let token = crate::auth::login_attempts::core::decode_token(&body.token)
        .ok_or(ApprovalError::LoginCodeStale)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let result = transition::claim_tx(&mut tx, &ctx, &auth, &token).await;
    let challenge = settle(tx, result).await?;
    Ok((StatusCode::CREATED, Json(challenge)))
}

/// "Yes, continue" (`approve`) or "I didn't start this" (`report`), with UV
/// and a signature whose `response` is the scanned token.
#[utoipa::path(
    post,
    path = "/api/pc-setups/{challenge_id}/answer",
    tag = "approvals",
    params(("challenge_id" = Uuid, Path, description = "Challenge id")),
    request_body = AnswerRequest,
    responses(
        (status = 200, description = "approve: PcSetupCode; report: PcSetupReported", body = PcSetupCode),
        (status = 400, description = "SIGNATURE_INVALID, APPROVAL_RESPONSE_MISMATCH or VALIDATION_ERROR"),
        (status = 403, description = "APPROVAL_NOT_ALLOWED or KEY_DEVICE_REQUIRED"),
        (status = 404, description = "APPROVAL_NOT_FOUND"),
        (status = 409, description = "APPROVAL_STATE: the attempt ended or moved on"),
        (status = 410, description = "APPROVAL_EXPIRED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, challenge_id = %id))]
pub async fn answer(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AnswerRequest>,
) -> Result<Response, ApprovalError> {
    let decision = match body.decision.as_str() {
        "approve" => Decision::Approve,
        "report" => Decision::Report,
        _ => {
            return Err(ApprovalError::Validation(
                "decision must be approve or report".into(),
            ))
        }
    };
    let response = b64u_decode(&body.response)
        .ok_or_else(|| ApprovalError::Validation("response must be base64url".into()))?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let result = transition::answer_tx(
        &mut tx,
        &ctx,
        &auth,
        id,
        decision,
        &response,
        &body.signature,
    )
    .await;
    match settle(tx, result).await? {
        Answered::Approved {
            attempt_id,
            code,
            code_expires_at,
        } => Ok(Json(PcSetupCode {
            attempt_id,
            code,
            code_expires_at,
        })
        .into_response()),
        Answered::Reported { attempt_id, events } => {
            crate::security_events::spawn_delivery(&state, events);
            Ok(Json(PcSetupReported { attempt_id }).into_response())
        }
    }
}

/// "No": unsigned, ends only the attempt.
#[utoipa::path(
    post,
    path = "/api/pc-setups/{challenge_id}/refuse",
    tag = "approvals",
    params(("challenge_id" = Uuid, Path, description = "Challenge id")),
    responses((status = 204), (status = 404, description = "APPROVAL_NOT_FOUND"), (status = 409, description = "APPROVAL_STATE")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, challenge_id = %id))]
pub async fn refuse(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApprovalError> {
    let mut tx = state.db.begin().await?;
    let result = transition::refuse_tx(&mut tx, &auth, id).await;
    settle(tx, result).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Release a `pc_setup` claim (`POST /api/approvals/{id}/release` dispatches
/// here).
pub async fn release(state: &AppState, auth: &AuthUser, id: Uuid) -> Result<(), ApprovalError> {
    let mut tx = state.db.begin().await?;
    let result = transition::release_tx(&mut tx, auth, id).await;
    settle(tx, result).await
}

/// The scanning phone's poll (while it shows the code).
#[utoipa::path(
    get,
    path = "/api/pc-setups/{attempt_id}",
    tag = "approvals",
    params(("attempt_id" = Uuid, Path, description = "Attempt id"), WaitQuery),
    responses((status = 200, body = PcSetupPhoneStatus), (status = 404, description = "APPROVAL_NOT_FOUND")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, attempt_id = %id))]
pub async fn phone_poll(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(q): Query<WaitQuery>,
) -> Result<Json<PcSetupPhoneStatus>, ApprovalError> {
    let first = {
        let mut conn = state.db.acquire().await?;
        transition::of_scanner(&mut conn, &auth, id).await?
    };
    let (row, _) = long_poll(&state, id, &first, q.wait.unwrap_or(0), false).await?;
    Ok(Json(PcSetupPhoneStatus {
        state: row.state,
        reason: row.reason,
    }))
}
