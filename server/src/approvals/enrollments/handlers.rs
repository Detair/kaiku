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
