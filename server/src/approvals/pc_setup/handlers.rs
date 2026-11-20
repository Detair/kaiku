//! "Set up this PC" routes (SM5).
//!
//! PC, every route bound to the requesting session's `sid` (another session
//! of the account gets 404): `POST /auth/pc-setups`, `GET
//! /auth/pc-setups/current`, `GET /auth/pc-setups/{id}?wait=`, `DELETE
//! /auth/pc-setups/{id}`, `POST /auth/pc-setups/{id}/code` and the web
//! registration `POST /auth/pc-setups/{id}/passkey/start` and `/finish`.
//! Phone, from a key device's own session: `POST
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

use super::enrolment::{self, CodeOutcome, CodePath, Credential};
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
/// answer comes at once, as for the login attempts.
const MAX_LONG_POLLS: usize = 32;
static LONG_POLLS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(MAX_LONG_POLLS);
/// Waiting long-polls per user (any of their sessions and devices).
const MAX_LONG_POLLS_PER_USER: usize = 4;

/// Who waits: one long-poll per attempt and side (PC or phone), and at most
/// [`MAX_LONG_POLLS_PER_USER`] per user. A poll that finds its place taken
/// answers at once.
#[derive(Default)]
struct Waiting {
    attempts: std::collections::HashSet<(Uuid, bool)>,
    users: std::collections::HashMap<Uuid, usize>,
}

static WAITING: std::sync::LazyLock<std::sync::Mutex<Waiting>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Waiting::default()));

/// A held place in [`WAITING`], given back on drop.
struct WaitSlot {
    attempt: (Uuid, bool),
    user: Uuid,
}

impl WaitSlot {
    fn take(attempt: Uuid, pc_side: bool, user: Uuid) -> Option<Self> {
        let mut w = WAITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let held = w.users.get(&user).copied().unwrap_or(0);
        if held >= MAX_LONG_POLLS_PER_USER || !w.attempts.insert((attempt, pc_side)) {
            return None;
        }
        w.users.insert(user, held + 1);
        Some(Self {
            attempt: (attempt, pc_side),
            user,
        })
    }
}

/// Whether a long-poll of `attempt` on that side holds its waiting place now
/// (for tests that must wait until it does).
#[doc(hidden)]
#[must_use]
pub fn long_poll_is_waiting(attempt: Uuid, pc_side: bool) -> bool {
    WAITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .attempts
        .contains(&(attempt, pc_side))
}

impl Drop for WaitSlot {
    fn drop(&mut self) {
        let mut w = WAITING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        w.attempts.remove(&self.attempt);
        if let Some(n) = w.users.get_mut(&self.user) {
            *n -= 1;
            if *n == 0 {
                w.users.remove(&self.user);
            }
        }
    }
}

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

/// `POST /auth/pc-setups/{id}/code` body. `Debug` hides the code.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct PcSetupCodeEntry {
    pub code: String,
}

impl std::fmt::Debug for PcSetupCodeEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PcSetupCodeEntry").finish_non_exhaustive()
    }
}

/// What a typed code did: `enrolled` (with the enrolment), `code` (wrong,
/// with the tries left) or `failed` (the third wrong code).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PcSetupCodeResult {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tries_left: Option<i16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enrollment_id: Option<Uuid>,
}

/// `POST /auth/pc-setups/{id}/passkey/finish` body.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct PcSetupPasskeyFinish {
    pub ceremony_id: Uuid,
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
    /// 1..=64 characters; `null` → "Passkey".
    #[serde(default)]
    pub label: Option<String>,
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

/// The refusals a transition gives from its checks, before its first
/// effect: after them the transaction holds only the lazy marks of
/// `lock_attempt` (an expired attempt, a void challenge), which are kept as
/// the sweep would keep them. Every other error (a database or internal
/// error, possibly after an effect) rolls back.
const fn keeps_marks(e: &ApprovalError) -> bool {
    matches!(
        e,
        ApprovalError::NotFound
            | ApprovalError::State
            | ApprovalError::Expired
            | ApprovalError::SignatureInvalid
            | ApprovalError::NotAllowed
            | ApprovalError::KeyDeviceRequired
            | ApprovalError::AccountBanned
            | ApprovalError::LoginCodeStale
            | ApprovalError::LoginAttemptClaimed
            | ApprovalError::PcSetupOtherAccount
            | ApprovalError::PcSetupUnavailable
            | ApprovalError::ResponseMismatch
            | ApprovalError::Validation(_)
            | ApprovalError::Auth(crate::auth::AuthError::InvalidToken)
    )
}

/// Lock attempt `id` of `auth`'s session (another session's → 404).
async fn lock_own(
    conn: &mut sqlx::PgConnection,
    auth: &AuthUser,
    id: Uuid,
) -> Result<transition::LockedAttempt, ApprovalError> {
    transition::of_session(conn, auth, id).await?;
    transition::lock_attempt(conn, id).await
}

/// Commit on success and after a check's refusal ([`keeps_marks`]); roll
/// back after anything else.
pub(crate) async fn settle<T>(
    tx: Transaction<'_, Postgres>,
    result: Result<T, ApprovalError>,
) -> Result<T, ApprovalError> {
    match result {
        Ok(v) => {
            tx.commit().await?;
            Ok(v)
        }
        Err(e) if keeps_marks(&e) => {
            tx.commit().await?;
            Err(e)
        }
        // Dropping the transaction rolls it back.
        Err(e) => Err(e),
    }
}

/// Wait for a change of `id` (`rotation`: the PC's poll, which also mints
/// due QR tokens). An ended attempt never changes, so it answers at once;
/// so does a poll without a free place (process, user, attempt).
async fn long_poll(
    state: &AppState,
    user: Uuid,
    id: Uuid,
    first: &AttemptRow,
    wait: u8,
    rotation: bool,
) -> Result<(AttemptRow, Option<Secret256>), ApprovalError> {
    let ctx = AuthCtx::from(state);
    let wants = wait > 0 && !first.state().is_terminal();
    let permit = wants.then(|| LONG_POLLS.try_acquire().ok()).flatten();
    let slot = permit
        .as_ref()
        .and_then(|_| WaitSlot::take(id, rotation, user));
    let wait = if slot.is_some() {
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
    let (row, token) = long_poll(&state, auth.id, id, &first, q.wait.unwrap_or(0), true).await?;
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

/// Type the phone's code (SM5 `code → enrolled`; the third wrong code →
/// `failed`). A wrong code answers 200 with the tries left.
#[utoipa::path(
    post,
    path = "/auth/pc-setups/{id}/code",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Attempt id")),
    request_body = PcSetupCodeEntry,
    responses(
        (status = 200, body = PcSetupCodeResult),
        (status = 400, description = "VALIDATION_ERROR: not 6 digits (no try spent)"),
        (status = 404, description = "APPROVAL_NOT_FOUND"),
        (status = 409, description = "APPROVAL_STATE: no code to type (ended, or already typed)"),
        (status = 410, description = "APPROVAL_EXPIRED: the code's 5 minutes ran out"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, attempt_id = %id))]
pub async fn code(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PcSetupCodeEntry>,
) -> Result<Json<PcSetupCodeResult>, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let result = async {
        let locked = lock_own(&mut tx, &auth, id).await?;
        enrolment::enter_code(&mut tx, &ctx, &locked, &body.code, CodePath::Web).await
    }
    .await;
    let out = match settle(tx, result).await? {
        CodeOutcome::Enrolled { enrollment_id } => PcSetupCodeResult {
            state: "enrolled".into(),
            tries_left: None,
            enrollment_id: Some(enrollment_id),
        },
        CodeOutcome::Wrong { tries_left } => PcSetupCodeResult {
            state: "code".into(),
            tries_left: Some(tries_left),
            enrollment_id: None,
        },
        CodeOutcome::Failed => {
            crate::security_events::notices::spawn_pending_delivery(&state);
            PcSetupCodeResult {
                state: "failed".into(),
                tries_left: Some(0),
                enrollment_id: None,
            }
        }
    };
    Ok(Json(out))
}

/// Start the web registration of the attempt's passkey (`enrolled` only).
#[utoipa::path(
    post,
    path = "/auth/pc-setups/{id}/passkey/start",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Attempt id")),
    responses(
        (status = 200, body = crate::auth::passkeys::types::CeremonyStart),
        (status = 404, description = "APPROVAL_NOT_FOUND"),
        (status = 409, description = "APPROVAL_STATE (no right code yet, or ended) or PASSKEY_LIMIT"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, attempt_id = %id))]
pub async fn passkey_start(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::auth::passkeys::types::CeremonyStart>, ApprovalError> {
    use crate::auth::passkeys::register::{begin, RegistrationBinding};
    crate::auth::passkeys::rp_of(&state)?;
    let mut tx = state.db.begin().await?;
    let result = async {
        let locked = lock_own(&mut tx, &auth, id).await?;
        let enrollment_id = enrolment::registration_open(&mut tx, &locked, CodePath::Web).await?;
        Ok((locked.attempt.session_id, enrollment_id))
    }
    .await;
    let (session_id, enrollment_id) = settle(tx, result).await?;
    // The ceremony is bound to this attempt, its enrolment and its session;
    // the finish re-checks all three under the locks.
    let (ceremony_id, ccr) = begin(
        &state,
        auth.id,
        RegistrationBinding::PcSetup {
            session_id,
            attempt_id: id,
            enrollment_id,
        },
        "Passkey".into(),
    )
    .await?;
    Ok(Json(crate::auth::passkeys::types::CeremonyStart {
        ceremony_id,
        options: serde_json::to_value(&ccr)
            .map_err(|e| crate::auth::AuthError::Internal(e.to_string()))?,
    }))
}

/// Finish the web registration → 201 `Passkey` (`active`); the attempt is
/// `completed`.
#[utoipa::path(
    post,
    path = "/auth/pc-setups/{id}/passkey/finish",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Attempt id")),
    request_body = PcSetupPasskeyFinish,
    responses(
        (status = 201, body = crate::auth::passkeys::types::PasskeyView),
        (status = 400, description = "PASSKEY_CEREMONY_INVALID, PASSKEY_INVALID, PASSKEY_NOT_DISCOVERABLE or VALIDATION_ERROR (label)"),
        (status = 404, description = "APPROVAL_NOT_FOUND"),
        (status = 409, description = "APPROVAL_STATE (ended, restarted), PASSKEY_LIMIT or PASSKEY_ALREADY_REGISTERED: the attempt stays readable"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, attempt_id = %id))]
pub async fn passkey_finish(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PcSetupPasskeyFinish>,
) -> Result<(StatusCode, Json<crate::auth::passkeys::types::PasskeyView>), ApprovalError> {
    use crate::auth::passkeys::ceremony::{self, CeremonyKind, CeremonyState};
    use crate::auth::passkeys::register::{clean_label, RegistrationBinding};
    use crate::auth::AuthError;
    let rp = crate::auth::passkeys::rp_of(&state)?;
    // Another session's attempt is unknown before its ceremony is touched.
    // A code typed in a desktop flow registers through that flow only: 409
    // before the ceremony is taken (the locked check below decides again).
    {
        let mut conn = state.db.acquire().await?;
        let row = transition::of_session(&mut conn, &auth, id).await?;
        if row.code_via.as_deref().is_some_and(|via| via != "web") {
            return Err(ApprovalError::State);
        }
    }
    let label = clean_label(body.label.as_deref())?;
    let rec = ceremony::take(&state.redis, body.ceremony_id)
        .await?
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (
        CeremonyKind::Register {
            user_id,
            binding:
                RegistrationBinding::PcSetup {
                    session_id,
                    attempt_id,
                    enrollment_id,
                },
            ..
        },
        CeremonyState::Register(st),
    ) = (rec.kind, rec.state)
    else {
        return Err(AuthError::PasskeyCeremonyInvalid.into());
    };
    if user_id != auth.id || Some(session_id) != auth.session_id || attempt_id != id {
        return Err(AuthError::PasskeyCeremonyInvalid.into());
    }
    let reg = serde_json::from_value(body.credential).map_err(|_| AuthError::PasskeyInvalid)?;
    let verified = rp
        .finish_registration(&reg, &st)
        .map_err(crate::auth::passkeys::failure)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let result = async {
        let locked = lock_own(&mut tx, &auth, id).await?;
        // The ceremony belongs to this attempt's enrolment.
        if locked.attempt.enrollment_id != Some(enrollment_id) {
            return Err(ApprovalError::State);
        }
        enrolment::complete_enrolment(
            &mut tx,
            &ctx,
            &locked,
            CodePath::Web,
            Credential {
                verified: &verified,
                label: &label,
            },
        )
        .await
    }
    .await;
    let row = settle(tx, result).await?;
    crate::security_events::notices::spawn_pending_delivery(&state);
    crate::approvals::devices::notify_authenticators_changed(&state, auth.id).await;
    tracing::info!(passkey_id = %row.id, "pc setup passkey added");
    Ok((StatusCode::CREATED, Json((&row).into())))
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
    let (row, _) = long_poll(&state, auth.id, id, &first, q.wait.unwrap_or(0), false).await?;
    Ok(Json(PcSetupPhoneStatus {
        state: row.state,
        reason: row.reason,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthError;

    #[test]
    fn wait_slots_are_one_per_attempt_side_and_few_per_user() {
        let user = Uuid::new_v4();
        let a = Uuid::new_v4();
        let pc = WaitSlot::take(a, true, user).expect("first");
        assert!(
            WaitSlot::take(a, true, user).is_none(),
            "same attempt and side"
        );
        let phone = WaitSlot::take(a, false, user).expect("the phone's side");
        let more: Vec<_> = (0..2)
            .filter_map(|_| WaitSlot::take(Uuid::new_v4(), true, user))
            .collect();
        assert_eq!(more.len(), 2);
        assert!(
            WaitSlot::take(Uuid::new_v4(), true, user).is_none(),
            "per user"
        );
        drop(pc);
        assert!(
            WaitSlot::take(a, true, user).is_some(),
            "given back on drop"
        );
        drop((phone, more));
    }

    #[test]
    fn only_check_refusals_keep_the_lazy_marks() {
        for e in [
            ApprovalError::NotFound,
            ApprovalError::State,
            ApprovalError::Expired,
            ApprovalError::SignatureInvalid,
            ApprovalError::NotAllowed,
            ApprovalError::LoginCodeStale,
            ApprovalError::PcSetupOtherAccount,
            ApprovalError::ResponseMismatch,
            ApprovalError::Auth(AuthError::InvalidToken),
        ] {
            assert!(keeps_marks(&e), "{e:?}");
        }
        // Errors that can follow an effect (a report's session end, its
        // event) roll the whole transition back.
        for e in [
            ApprovalError::Database(sqlx::Error::RowNotFound),
            ApprovalError::Auth(AuthError::Internal("notify".into())),
            ApprovalError::Auth(AuthError::Database(sqlx::Error::PoolTimedOut)),
            ApprovalError::Unavailable,
        ] {
            assert!(!keeps_marks(&e), "{e:?}");
        }
    }
}
