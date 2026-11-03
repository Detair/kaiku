//! `/auth/enrollments/*` (C26).
//!
//! The new device's public routes and the issuing key device's `enroll` challenge. The HTTP
//! issuer routes are mounted per issuer in L1b; L1a drives `issue_tx`/`confirm_tx` directly.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use super::{
    claim as claim_core, issuer_challenge_tx, open as open_core, pop_challenge as pop_core,
    status as status_core, ClaimRequest, ClaimResponse, OpenResponse, PopChallengeRequest,
    PopChallengeResponse, StatusRequest, StatusResponse,
};
use crate::api::AppState;
use crate::approvals::devices::key_device_session;
use crate::approvals::error::ApprovalError;
use crate::approvals::secrets::Secret256;
use crate::approvals::types::ApprovalChallenge;
use crate::auth::ctx::AuthCtx;
use crate::auth::AuthUser;

/// `POST /auth/enrollments/open` body (a POST so the token stays out of
/// URLs and logs).
#[derive(Deserialize, utoipa::ToSchema)]
pub struct OpenRequest {
    pub setup_token: String,
}

/// SM2 "open": server name and enrolment nonce; consumes nothing.
#[utoipa::path(
    post,
    path = "/auth/enrollments/open",
    tag = "approvals",
    request_body = OpenRequest,
    responses((status = 200, body = OpenResponse), (status = 404, description = "ENROLLMENT_TOKEN_INVALID")),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn open(
    State(state): State<AppState>,
    Json(body): Json<OpenRequest>,
) -> Result<Json<OpenResponse>, ApprovalError> {
    Ok(Json(
        open_core(&AuthCtx::from(&state), &body.setup_token).await?,
    ))
}

/// SM2 `enroll_pop` challenge for the new device's key.
#[utoipa::path(
    post,
    path = "/auth/enrollments/pop-challenge",
    tag = "approvals",
    request_body = PopChallengeRequest,
    responses((status = 201, body = PopChallengeResponse)),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn pop_challenge(
    State(state): State<AppState>,
    Json(body): Json<PopChallengeRequest>,
) -> Result<(StatusCode, Json<PopChallengeResponse>), ApprovalError> {
    Ok((
        StatusCode::CREATED,
        Json(pop_core(&AuthCtx::from(&state), body).await?),
    ))
}

/// SM2 "claim": the code goes only to this response.
#[utoipa::path(
    post,
    path = "/auth/enrollments/claim",
    tag = "approvals",
    request_body = ClaimRequest,
    responses(
        (status = 201, body = ClaimResponse),
        (status = 400, description = "DEVICE_UNSUITABLE or SIGNATURE_INVALID"),
        (status = 409, description = "ENROLLMENT_ALREADY_CLAIMED"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn claim(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ClaimRequest>,
) -> Result<(StatusCode, Json<ClaimResponse>), ApprovalError> {
    let ctx = AuthCtx::from(&state);
    // The Android app enrolling itself sends its own ticket or request
    // secret (C26/C27 headers).
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(Secret256::from_b64u)
    };
    let own = header("x-kaiku-ticket")
        .map(super::OwnClaim::Ticket)
        .or_else(|| header("x-kaiku-request-secret").map(super::OwnClaim::Request));
    let (response, after) = claim_core(&ctx, body, own).await?;
    if let Some(after) = after {
        super::after_done(&state, after).await;
        return Ok((StatusCode::CREATED, Json(response)));
    }
    // Tell the issuer's screen that a device claimed the setup.
    if let Ok(Some(user)) = sqlx::query_scalar::<_, Uuid>(
        "SELECT d.user_id FROM enrollments e JOIN approval_devices d ON d.id = e.issuer_device_id
          WHERE e.id = $1",
    )
    .bind(response.enrollment_id())
    .fetch_optional(&state.db)
    .await
    {
        super::enrollment_updated(&state, user, response.enrollment_id(), "claimed").await;
    }
    Ok((StatusCode::CREATED, Json(response)))
}

/// SM2 status for the claiming device (long-poll, `wait` ≤ 25 s).
#[utoipa::path(
    post,
    path = "/auth/enrollments/status",
    tag = "approvals",
    request_body = StatusRequest,
    responses((status = 200, body = StatusResponse), (status = 404, description = "ENROLLMENT_TOKEN_INVALID")),
    security(()),
)]
#[tracing::instrument(skip_all, fields(enrollment_id = %body.enrollment_id))]
pub async fn status(
    State(state): State<AppState>,
    Json(body): Json<StatusRequest>,
) -> Result<Json<StatusResponse>, ApprovalError> {
    let secret =
        Secret256::from_b64u(&body.claim_secret).ok_or(ApprovalError::EnrollmentTokenInvalid)?;
    Ok(Json(
        status_core(
            &AuthCtx::from(&state),
            body.enrollment_id,
            &secret,
            body.wait.unwrap_or(0),
        )
        .await?,
    ))
}

/// The issuing key device's `enroll` approval (a new one voids the open one).
#[utoipa::path(
    post,
    path = "/auth/enrollments/{id}/challenge",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    responses((status = 201, body = ApprovalChallenge), (status = 403, description = "KEY_DEVICE_REQUIRED")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn issuer_challenge(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<ApprovalChallenge>), ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let dev = key_device_session(&mut tx, &auth).await?;
    let result = issuer_challenge_tx(&mut tx, &ctx, &dev, id).await;
    // An expired code is marked so: keep that even on the error.
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(result?)))
}

// ---------------------------------------------------------------------------
// The bearer issuer routes (`own`, F5a; `admin` enrolments for GET/DELETE)
// ---------------------------------------------------------------------------

/// `POST /auth/enrollments` → 201 `EnrollmentIssued` ("Add a phone", `own`).
///
/// From a key device with `may_approve` (else 403 `APPROVAL_NOT_ALLOWED`) by
/// its `enrollment.issue` grant, or from a full PC session by a passkey
/// grant; the grant is consumed here and recorded on the enrolment.
#[utoipa::path(
    post,
    path = "/auth/enrollments",
    tag = "approvals",
    responses(
        (status = 201, body = super::EnrollmentIssued),
        (status = 403, description = "FULL_SESSION_REQUIRED, STEP_UP_REQUIRED or APPROVAL_NOT_ALLOWED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn issue_own(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<(StatusCode, Json<super::EnrollmentIssued>), ApprovalError> {
    use crate::auth::step_up::{self, StepUpAction, StepUpTarget};
    use crate::auth::strong::AuthenticatorKind;
    crate::auth::guards::require_full(&auth)?;
    let sid = auth.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let locked = crate::approvals::transition::lock_accounts(&mut tx, &[auth.id]).await?;
    let key_device = crate::approvals::devices::require_key_device_snapshot(&auth).is_ok();
    let issuer = if key_device {
        let dev = key_device_session(&mut tx, &auth).await?;
        if !locked.get(&auth.id).is_some_and(|a| a.may_approve) {
            return Err(ApprovalError::NotAllowed);
        }
        step_up::consume_tx(
            &mut tx,
            &auth,
            StepUpAction::EnrollmentIssue,
            &StepUpTarget::None,
        )
        .await?;
        super::Issuer::KeyDevice {
            session_id: sid,
            device_id: dev.device.id,
        }
    } else {
        let grant = step_up::consume_tx(
            &mut tx,
            &auth,
            StepUpAction::EnrollmentIssue,
            &StepUpTarget::None,
        )
        .await?;
        if grant.by.kind != AuthenticatorKind::Passkey {
            return Err(ApprovalError::KeyDeviceRequired);
        }
        super::Issuer::PcSession {
            session_id: sid,
            passkey_id: grant.by.id,
            grant_id: grant.id,
        }
    };
    let issued = super::issue_tx(
        &mut tx,
        &ctx,
        super::IssueParams {
            variant: super::EnrollmentVariant::Own,
            subject_id: auth.id,
            issuer,
            claimant: crate::approvals::context::ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(issued)))
}

/// The enrolment `id` as its issuer (or, for an `admin` enrolment, any
/// admin) sees it; else 404.
async fn issued_by(
    conn: &mut sqlx::PgConnection,
    auth: &AuthUser,
    id: Uuid,
) -> Result<super::EnrollmentRow, ApprovalError> {
    let row: Option<super::EnrollmentRow> = sqlx::query_as(
        "SELECT e.* FROM enrollments e
          WHERE e.id = $1
            AND (EXISTS (SELECT 1 FROM approval_devices d
                          WHERE d.id = e.issuer_device_id AND d.user_id = $2)
                 OR EXISTS (SELECT 1 FROM sessions s
                             WHERE s.id = e.issuer_session_id AND s.user_id = $2)
                 OR (e.variant = 'admin'
                     AND EXISTS (SELECT 1 FROM system_admins WHERE user_id = $2)))",
    )
    .bind(id)
    .bind(auth.id)
    .fetch_optional(&mut *conn)
    .await?;
    row.ok_or(ApprovalError::NotFound)
}

/// `GET /auth/enrollments/{id}` → `Enrollment` (issuer).
#[utoipa::path(
    get,
    path = "/auth/enrollments/{id}",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    responses((status = 200, body = super::Enrollment)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn get_issued(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<super::Enrollment>, ApprovalError> {
    let mut conn = state.db.acquire().await?;
    let row = issued_by(&mut conn, &auth, id).await?;
    Ok(Json(super::to_wire(&mut conn, &row).await?))
}

/// `DELETE /auth/enrollments/{id}` → 204 (issuer; any admin for `admin`).
#[utoipa::path(
    delete,
    path = "/auth/enrollments/{id}",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    responses((status = 204), (status = 409, description = "ENROLLMENT_STATE")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip(state, auth), fields(user_id = %auth.id))]
pub async fn cancel_issued(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let seen = issued_by(&mut tx, &auth, id).await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[seen.subject_id, auth.id]).await?;
    let by = if seen.variant == "admin" {
        super::EnrollmentCanceller::Admin(auth.id)
    } else {
        super::EnrollmentCanceller::Issuer
    };
    super::cancel_tx(&mut tx, &ctx, id, by).await?;
    tx.commit().await?;
    super::enrollment_updated(&state, seen.subject_id, id, "cancelled").await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /auth/enrollments/{id}/confirm {code}` → `Enrollment` (the
/// issuing PC session; a key device confirms by its `enroll` approval).
#[utoipa::path(
    post,
    path = "/auth/enrollments/{id}/confirm",
    tag = "approvals",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    request_body = crate::auth::registration::CodeRequest,
    responses((status = 200, body = super::Enrollment), (status = 400, description = "ENROLL_CODE_WRONG")),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, enrollment_id = %id))]
pub async fn confirm_pc(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::auth::registration::CodeRequest>,
) -> Result<Json<super::Enrollment>, ApprovalError> {
    crate::auth::guards::require_full(&auth)?;
    let ctx = AuthCtx::from(&state);
    let code: String = body.code.chars().filter(|c| !c.is_whitespace()).collect();
    let mut tx = state.db.begin().await?;
    match super::confirm_tx(
        &mut tx,
        &ctx,
        id,
        super::Confirmer::Pc(&auth),
        code.as_bytes(),
    )
    .await
    {
        Ok((enrollment, after)) => {
            tx.commit().await?;
            super::after_done(&state, after).await;
            Ok(Json(enrollment))
        }
        Err(
            e @ (ApprovalError::EnrollCodeWrong { .. }
            | ApprovalError::EnrollmentFailed
            | ApprovalError::EnrollmentState),
        ) => {
            tx.commit().await?;
            Err(e)
        }
        Err(e) => Err(e),
    }
}
