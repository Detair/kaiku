//! The request client (C27 "Request client"): staging while `ready`, then Finish.
//!
//! Every route needs the request secret (else 404, as an unknown request)
//! and state `ready` (else 409 `ACCOUNT_REQUEST_NOT_READY {state}`). Staged
//! values live in `account_requests.staged`; Finish runs the completion
//! transaction and mints no session.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::core::{self, RequestRow};
use super::handlers::{request_cookie, request_secret, request_updated};
use super::types::AccountRequest;
use crate::api::AppState;
use crate::approvals::enrollments::{self, Confirmer, EnrollmentVariant, IssueParams, Issuer};
use crate::approvals::error::ApprovalError;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::registration::{CodeRequest, TotpSetup};
use crate::auth::strong::AuthenticatorRef;
use crate::security_events::spawn_delivery;

/// The locked `ready` request of the presented secret.
async fn ready_request(
    tx: &mut sqlx::PgConnection,
    ctx: &AuthCtx<'_>,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> AuthResult<RequestRow> {
    let secret = request_secret(headers, jar, ctx.config)?;
    let row = core::lock_by_secret(tx, ctx, &secret).await?;
    if row.state != "ready" {
        return Err(AuthError::AccountRequestNotReady { state: row.state });
    }
    // Past the Finish deadline (the minute sweep has not run yet).
    if row.expires_at.is_none_or(|t| t <= chrono::Utc::now()) {
        return Err(AuthError::AccountRequestNotReady {
            state: "expired".into(),
        });
    }
    Ok(row)
}

async fn set_staged(
    tx: &mut sqlx::PgConnection,
    id: Uuid,
    key: &str,
    value: serde_json::Value,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE account_requests SET staged = jsonb_set(staged, $2, $3) WHERE id = $1")
        .bind(id)
        .bind(vec![key.to_string()])
        .bind(value)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// `PUT …/current/password` body.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct PasswordBody {
    pub password: String,
}

impl std::fmt::Debug for PasswordBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordBody { .. }")
    }
}

/// Stage a new password (the blocklist applies).
#[utoipa::path(
    put,
    path = "/auth/account-requests/current/password",
    tag = "auth",
    request_body = PasswordBody,
    responses((status = 200, body = AccountRequest), (status = 409, description = "ACCOUNT_REQUEST_NOT_READY")),
)]
#[tracing::instrument(skip_all)]
pub async fn stage_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<PasswordBody>,
) -> AuthResult<Json<AccountRequest>> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar).await?;
    let (username, display_name): (String, String) =
        sqlx::query_as("SELECT username, display_name FROM users WHERE id = $1")
            .bind(row.user_id)
            .fetch_one(&mut *tx)
            .await?;
    let server_name = crate::approvals::server_name(&mut tx).await?;
    crate::auth::password_policy::check(
        &body.password,
        &crate::auth::password_policy::PolicyContext {
            username: &username,
            display_name: &display_name,
            server_name: &server_name,
        },
    )
    .map_err(AuthError::PasswordRejected)?;
    let password = body.password.clone();
    let hash = tokio::task::spawn_blocking(move || crate::auth::hash_password(&password))
        .await
        .map_err(|e| AuthError::Internal(format!("password hashing failed: {e}")))?
        .map_err(|_| AuthError::PasswordHash)?;
    set_staged(&mut tx, row.id, "password_hash", serde_json::json!(hash)).await?;
    let fresh = core::get(&mut tx, row.id).await?.unwrap_or(row);
    let wire = core::to_wire(&mut tx, &fresh).await?;
    tx.commit().await?;
    Ok(Json(wire))
}

/// Stage a new TOTP secret: `{secret, otpauth_uri}` (confirm at
/// `…/totp/verify`).
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/totp",
    tag = "auth",
    responses((status = 200, body = TotpSetup)),
)]
#[tracing::instrument(skip_all)]
pub async fn totp_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<Json<TotpSetup>> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar).await?;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(row.user_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(
        crate::auth::registration::pending_totp(&state, row.user_id, &username).await?,
    ))
}

/// Confirm the staged TOTP secret with a code (it becomes active only at
/// Finish).
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/totp/verify",
    tag = "auth",
    request_body = CodeRequest,
    responses((status = 200, body = AccountRequest), (status = 401, description = "INVALID_MFA")),
)]
#[tracing::instrument(skip_all)]
pub async fn totp_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<CodeRequest>,
) -> AuthResult<Json<AccountRequest>> {
    use fred::interfaces::KeysInterface;
    let ctx = AuthCtx::from(&state);
    let secret = request_secret(&headers, &jar, ctx.config)?;
    // The code is checked before any lock (a Valkey round trip).
    let seen = {
        let mut conn = state.db.acquire().await?;
        core::by_secret(&mut conn, &ctx, &secret, false)
            .await?
            .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?
    };
    if seen.state != "ready" {
        return Err(AuthError::AccountRequestNotReady { state: seen.state });
    }
    let key = format!("mfa:pending:{}", seen.user_id);
    let pending: Option<String> = state
        .redis
        .get(&key)
        .await
        .map_err(|_| AuthError::ServiceUnavailable)?;
    let encrypted = pending
        .ok_or_else(|| AuthError::Validation("Start the authenticator setup first".into()))?;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(seen.user_id)
        .fetch_one(&state.db)
        .await?;
    if !crate::auth::reauth::accept_totp_ctx(
        &ctx,
        seen.user_id,
        &encrypted,
        &username,
        body.code.trim(),
    )
    .await?
    {
        return Err(AuthError::InvalidMfaCode);
    }
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar).await?;
    set_staged(&mut tx, row.id, "totp_secret", serde_json::json!(encrypted)).await?;
    let fresh = core::get(&mut tx, row.id).await?.unwrap_or(row);
    let wire = core::to_wire(&mut tx, &fresh).await?;
    tx.commit().await?;
    let _ = state.redis.del::<(), _>(&key).await;
    Ok(Json(wire))
}

/// `PUT …/current/recovery-contact` body.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ContactBody {
    pub username: String,
}

/// Stage a recovery contact (the rules of C28 apply).
#[utoipa::path(
    put,
    path = "/auth/account-requests/current/recovery-contact",
    tag = "auth",
    request_body = ContactBody,
    responses((status = 200, body = AccountRequest), (status = 400, description = "RECOVERY_CONTACT_INVALID")),
)]
#[tracing::instrument(skip_all)]
pub async fn stage_contact(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<ContactBody>,
) -> AuthResult<Json<AccountRequest>> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar).await?;
    let contact: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT u.id, EXISTS (SELECT 1 FROM system_admins sa WHERE sa.user_id = u.id)
           FROM users u WHERE u.username = $1 AND NOT u.is_system",
    )
    .bind(&body.username)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((contact, false)) = contact.filter(|(c, _)| *c != row.user_id) else {
        return Err(AuthError::RecoveryContactInvalid);
    };
    set_staged(
        &mut tx,
        row.id,
        "recovery_contact_id",
        serde_json::json!(contact),
    )
    .await?;
    let fresh = core::get(&mut tx, row.id).await?.unwrap_or(row);
    let wire = core::to_wire(&mut tx, &fresh).await?;
    tx.commit().await?;
    Ok(Json(wire))
}

/// `POST …/current/finish` answer: the recovery codes, shown once.
#[derive(Serialize, utoipa::ToSchema)]
pub struct FinishResponse {
    pub request: AccountRequest,
    pub recovery_codes: Vec<String>,
}

impl std::fmt::Debug for FinishResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FinishResponse")
            .field("request", &self.request.id)
            .finish_non_exhaustive()
    }
}

/// Finish: apply everything in one transaction (B.7 rule 1 + 4); no
/// session. A second Finish answers 409 `ACCOUNT_REQUEST_NOT_READY {state:
/// "completed"}`.
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/finish",
    tag = "auth",
    responses((status = 200, body = FinishResponse), (status = 409, description = "ACCOUNT_REQUEST_NOT_READY")),
)]
#[tracing::instrument(skip_all)]
pub async fn finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<(CookieJar, Json<FinishResponse>)> {
    let ctx = AuthCtx::from(&state);
    let secret = request_secret(&headers, &jar, ctx.config)?;
    let mut tx = state.db.begin().await?;
    let row = core::lock_for_finish(&mut tx, &ctx, &secret).await?;
    let out = core::complete_locked_tx(&mut tx, &ctx, &row).await?;
    let done = core::get(&mut tx, row.id).await?.unwrap_or(row);
    let request = core::to_wire(&mut tx, &done).await?;
    tx.commit().await?;
    spawn_delivery(&state, out.event_ids);
    crate::security_events::notices::spawn_pending_delivery(&state);
    request_updated(&state, done.id, done.user_id, &done.state).await;
    if done.state != "completed" {
        return Err(AuthError::AccountRequestNotReady { state: done.state });
    }
    Ok((
        jar.add(request_cookie("", 0, &state.config)),
        Json(FinishResponse {
            request,
            recovery_codes: out.recovery_codes.unwrap_or_default(),
        }),
    ))
}

// ---------------------------------------------------------------------------
// A phone for the request (variant `request`, C26 issuer routes)
// ---------------------------------------------------------------------------

const fn approval(e: AuthError) -> ApprovalError {
    ApprovalError::Auth(e)
}

/// `POST /auth/account-requests/current/enrollments` → 201 `EnrollmentIssued`
/// (a new one voids the old).
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/enrollments",
    tag = "auth",
    responses((status = 201, body = enrollments::EnrollmentIssued)),
)]
#[tracing::instrument(skip_all)]
pub async fn enrollment_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<(StatusCode, Json<enrollments::EnrollmentIssued>), ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar)
        .await
        .map_err(approval)?;
    let issued = enrollments::issue_tx(
        &mut tx,
        &ctx,
        IssueParams {
            variant: EnrollmentVariant::Request,
            subject_id: row.user_id,
            issuer: Issuer::Request { request_id: row.id },
            claimant: crate::approvals::context::ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(issued)))
}

/// The request's own enrolment `id` (else 404).
async fn request_enrollment(
    conn: &mut sqlx::PgConnection,
    request_id: Uuid,
    id: Uuid,
) -> Result<enrollments::EnrollmentRow, ApprovalError> {
    let row: Option<enrollments::EnrollmentRow> = sqlx::query_as(
        "SELECT * FROM enrollments
          WHERE id = $1 AND issuer_kind = 'request' AND issuer_request_id = $2",
    )
    .bind(id)
    .bind(request_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.ok_or(ApprovalError::NotFound)
}

/// `GET /auth/account-requests/current/enrollments/{id}` → `Enrollment`.
#[utoipa::path(
    get,
    path = "/auth/account-requests/current/enrollments/{id}",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    responses((status = 200, body = enrollments::Enrollment)),
)]
#[tracing::instrument(skip_all, fields(enrollment_id = %id))]
pub async fn enrollment_get(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Json<enrollments::Enrollment>, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let secret = request_secret(&headers, &jar, ctx.config).map_err(approval)?;
    let mut conn = state.db.acquire().await?;
    let seen = core::by_secret(&mut conn, &ctx, &secret, false)
        .await?
        .ok_or(ApprovalError::NotFound)?;
    let row = request_enrollment(&mut conn, seen.id, id).await?;
    Ok(Json(enrollments::to_wire(&mut conn, &row).await?))
}

/// `POST /auth/account-requests/current/enrollments/{id}/confirm {code}`.
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/enrollments/{id}/confirm",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    request_body = CodeRequest,
    responses((status = 200, body = enrollments::Enrollment), (status = 400, description = "ENROLL_CODE_WRONG")),
)]
#[tracing::instrument(skip_all, fields(enrollment_id = %id))]
pub async fn enrollment_confirm(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<CodeRequest>,
) -> Result<Json<enrollments::Enrollment>, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let code: String = body.code.chars().filter(|c| !c.is_whitespace()).collect();
    // One transaction: the request stays `ready` while its device lands.
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar)
        .await
        .map_err(approval)?;
    request_enrollment(&mut tx, row.id, id).await?;
    match enrollments::confirm_tx(
        &mut tx,
        &ctx,
        id,
        Confirmer::Request(row.id),
        code.as_bytes(),
    )
    .await
    {
        Ok((enrollment, after)) => {
            tx.commit().await?;
            enrollments::after_done(&state, after).await;
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

/// `DELETE /auth/account-requests/current/enrollments/{id}` → 204.
#[utoipa::path(
    delete,
    path = "/auth/account-requests/current/enrollments/{id}",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Enrolment id")),
    responses((status = 204)),
)]
#[tracing::instrument(skip_all, fields(enrollment_id = %id))]
pub async fn enrollment_cancel(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<StatusCode, ApprovalError> {
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar)
        .await
        .map_err(approval)?;
    request_enrollment(&mut tx, row.id, id).await?;
    enrollments::cancel_tx(&mut tx, &ctx, id, enrollments::EnrollmentCanceller::Issuer).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stage a passkey (C29 request variant, `pending` until Finish).
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/passkey/start",
    tag = "auth",
    request_body = crate::auth::passkeys::types::RegisterStartRequest,
    responses((status = 200, body = crate::auth::passkeys::types::CeremonyStart)),
)]
#[tracing::instrument(skip_all)]
pub async fn passkey_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<crate::auth::passkeys::types::RegisterStartRequest>,
) -> AuthResult<Json<crate::auth::passkeys::types::CeremonyStart>> {
    use crate::auth::passkeys::register::{begin, clean_label, RegistrationBinding};
    crate::auth::passkeys::rp_of(&state)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar).await?;
    tx.commit().await?;
    let label = clean_label(body.label.as_deref())?;
    let (ceremony_id, ccr) = begin(
        &state,
        row.user_id,
        RegistrationBinding::Request { request_id: row.id },
        label,
    )
    .await?;
    Ok(Json(crate::auth::passkeys::types::CeremonyStart {
        ceremony_id,
        options: serde_json::to_value(&ccr).map_err(|e| AuthError::Internal(e.to_string()))?,
    }))
}

/// Finish the staged passkey → 201 `Passkey` (`pending`, `ready_at`).
#[utoipa::path(
    post,
    path = "/auth/account-requests/current/passkey/finish",
    tag = "auth",
    request_body = crate::auth::passkeys::types::CeremonyFinishRequest,
    responses((status = 201, body = crate::auth::passkeys::types::PasskeyView)),
)]
#[tracing::instrument(skip_all)]
pub async fn passkey_finish(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<crate::auth::passkeys::types::CeremonyFinishRequest>,
) -> AuthResult<(StatusCode, Json<crate::auth::passkeys::types::PasskeyView>)> {
    use crate::auth::passkeys::ceremony::{self, CeremonyKind, CeremonyState};
    use crate::auth::passkeys::queries::PasskeyState;
    use crate::auth::passkeys::register::{
        complete_recorded, RegistrationBinding, RegistrationVia,
    };
    let rp = crate::auth::passkeys::rp_of(&state)?;
    let ctx = AuthCtx::from(&state);
    let rec = ceremony::take(&state.redis, body.ceremony_id)
        .await?
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (
        CeremonyKind::Register {
            user_id,
            binding: RegistrationBinding::Request { request_id },
            label,
        },
        CeremonyState::Register(st),
    ) = (rec.kind, rec.state)
    else {
        return Err(AuthError::PasskeyCeremonyInvalid);
    };
    let reg = serde_json::from_value(body.credential).map_err(|_| AuthError::PasskeyInvalid)?;
    let verified = rp
        .finish_registration(&reg, &st)
        .map_err(crate::auth::passkeys::failure)?;
    let mut tx = state.db.begin().await?;
    let row = ready_request(&mut tx, &ctx, &headers, &jar).await?;
    if row.id != request_id || row.user_id != user_id {
        return Err(AuthError::PasskeyCeremonyInvalid);
    }
    let (pk, added) = complete_recorded(
        &mut tx,
        row.user_id,
        &label,
        &verified,
        PasskeyState::Pending,
        Some(row.ready_at),
        RegistrationVia::Request,
    )
    .await?;
    core::stage_authenticator_tx(&mut tx, row.id, AuthenticatorRef::passkey(pk.id)).await?;
    tx.commit().await?;
    spawn_delivery(&state, vec![added]);
    Ok((
        StatusCode::CREATED,
        Json(crate::auth::passkeys::types::PasskeyView::from(&pk)),
    ))
}

/// The request-client routes under `/auth/account-requests/current`
/// (merged into `public_router`).
pub fn routes() -> axum::Router<AppState> {
    use axum::routing::{get, post, put};
    axum::Router::new()
        .route("/account-requests/current/password", put(stage_password))
        .route("/account-requests/current/totp", post(totp_start))
        .route("/account-requests/current/totp/verify", post(totp_verify))
        .route(
            "/account-requests/current/recovery-contact",
            put(stage_contact),
        )
        .route("/account-requests/current/finish", post(finish))
        .route(
            "/account-requests/current/enrollments",
            post(enrollment_issue),
        )
        .route(
            "/account-requests/current/enrollments/{id}",
            get(enrollment_get).delete(enrollment_cancel),
        )
        .route(
            "/account-requests/current/enrollments/{id}/confirm",
            post(enrollment_confirm),
        )
        .route(
            "/account-requests/current/passkey/start",
            post(passkey_start),
        )
        .route(
            "/account-requests/current/passkey/finish",
            post(passkey_finish),
        )
}
