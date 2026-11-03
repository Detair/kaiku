//! Registration tickets and passwordless accounts (spec F8 "New accounts";
//! C27 "Registration tickets").
//!
//! **SECURITY CRITICAL.** `POST /auth/register` creates the account and a
//! registration ticket (256-bit, stored only as a keyed hash, 30 minutes),
//! never a session. With the ticket the new account binds its first
//! authenticator on that client: a passkey (C29), a key device (an
//! enrolment issued by the ticket, or the Android app's own key by a claim
//! with `X-Kaiku-Ticket`), or TOTP (then it signs in by F4 and gets
//! `bootstrap_open`). An account that binds nothing within 30 minutes is
//! deleted by the minute sweep. The first admin of a fresh server must bind
//! a key device or a passkey (elevation needs a full session).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Duration, Utc};
use fred::prelude::*;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use super::ctx::AuthCtx;
use super::error::{AuthError, AuthResult};
use crate::api::AppState;
use crate::approvals::enrollments::{self, Confirmer, EnrollmentVariant, IssueParams, Issuer};
use crate::approvals::error::ApprovalError;
use crate::approvals::secrets::{keyed_hash, Secret256, SecretPurpose};
use crate::config::Config;
use crate::security_events::{record, spawn_delivery, Actor, EventType, NewSecurityEvent};

/// A ticket lives 30 minutes (F8; also a DB CHECK).
pub const TICKET_MINUTES: i64 = 30;
/// The browser's ticket cookie and its path (C27 common rules).
pub const TICKET_COOKIE: &str = "__Secure-kaiku_ticket";
pub const TICKET_COOKIE_PATH: &str = "/auth/registration";
/// The native header.
pub const TICKET_HEADER: &str = "x-kaiku-ticket";

/// A stored ticket.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TicketRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub bound_at: Option<DateTime<Utc>>,
}

fn ticket_hash(ctx: &AuthCtx<'_>, secret: &Secret256) -> [u8; 32] {
    keyed_hash(
        ctx.hmac_key(),
        SecretPurpose::RegistrationTicket,
        secret.as_bytes(),
    )
}

/// Issue the ticket of a new account (one per account).
pub async fn issue_ticket_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
) -> AuthResult<(Uuid, Secret256, DateTime<Utc>)> {
    let secret = Secret256::random();
    let id = Uuid::new_v4();
    let now = Utc::now();
    let expires_at = now + Duration::minutes(TICKET_MINUTES);
    sqlx::query(
        "INSERT INTO registration_tickets (id, user_id, ticket_hash, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(user_id)
    .bind(ticket_hash(ctx, &secret).to_vec())
    .bind(now)
    .bind(expires_at)
    .execute(&mut *conn)
    .await?;
    Ok((id, secret, expires_at))
}

/// The live ticket of `secret` (unexpired), if any.
pub async fn ticket_by_secret(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
) -> sqlx::Result<Option<TicketRow>> {
    sqlx::query_as(
        "SELECT id, user_id, created_at, expires_at, bound_at FROM registration_tickets
          WHERE ticket_hash = $1 AND expires_at > NOW()",
    )
    .bind(ticket_hash(ctx, secret).to_vec())
    .fetch_optional(&mut *conn)
    .await
}

/// The id of the live ticket of `secret` (the own-key claim, SM2).
pub async fn ticket_id_by_secret(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    secret: &Secret256,
) -> sqlx::Result<Option<Uuid>> {
    Ok(ticket_by_secret(conn, ctx, secret).await?.map(|t| t.id))
}

/// The ticket the request presents: `X-Kaiku-Ticket` (native) or the
/// cookie (browser); unknown or expired → 404 `REGISTRATION_TICKET_INVALID`.
pub async fn ticket_from_request(
    ctx: &AuthCtx<'_>,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> AuthResult<TicketRow> {
    let secret = headers
        .get(TICKET_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            jar.get(super::cookies::cookie_name(TICKET_COOKIE, ctx.config))
                .map(|c| c.value().to_string())
        })
        .and_then(|s| Secret256::from_b64u(&s))
        .ok_or(AuthError::TicketInvalid)?;
    let mut conn = ctx.db.acquire().await?;
    ticket_by_secret(&mut conn, ctx, &secret)
        .await?
        .ok_or(AuthError::TicketInvalid)
}

/// [`ticket_from_request`] for a route that binds: a bound ticket binds
/// nothing more (Codex M9; 404 `REGISTRATION_TICKET_INVALID`).
pub async fn unbound_ticket(
    ctx: &AuthCtx<'_>,
    headers: &HeaderMap,
    jar: &CookieJar,
) -> AuthResult<TicketRow> {
    let t = ticket_from_request(ctx, headers, jar).await?;
    if t.bound_at.is_some() {
        return Err(AuthError::TicketInvalid);
    }
    Ok(t)
}

/// Under the account lock: the ticket is still unbound (a concurrent bind
/// wins).
async fn still_unbound(conn: &mut PgConnection, ticket_id: Uuid) -> AuthResult<()> {
    let unbound: bool = sqlx::query_scalar(
        "SELECT bound_at IS NULL FROM registration_tickets WHERE id = $1 FOR UPDATE",
    )
    .bind(ticket_id)
    .fetch_optional(&mut *conn)
    .await?
    .unwrap_or(false);
    if unbound {
        Ok(())
    } else {
        Err(AuthError::TicketInvalid)
    }
}

/// Mark the account's ticket bound (anything was bound to it).
pub async fn mark_bound_tx(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE registration_tickets SET bound_at = COALESCE(bound_at, NOW()) WHERE user_id = $1",
    )
    .bind(user_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The minute sweep (F8): delete accounts whose ticket bound nothing within
/// 30 minutes, and the tickets of bound accounts once they expired.
pub async fn delete_unbound_due(conn: &mut PgConnection) -> sqlx::Result<u64> {
    let deleted = sqlx::query(
        "DELETE FROM users u USING registration_tickets t
          WHERE t.user_id = u.id AND t.bound_at IS NULL
            AND t.created_at <= NOW() - make_interval(mins => $1) AND NOT u.is_system",
    )
    .bind(i32::try_from(TICKET_MINUTES).unwrap_or(30))
    .execute(&mut *conn)
    .await?
    .rows_affected();
    sqlx::query(
        "DELETE FROM registration_tickets WHERE bound_at IS NOT NULL AND expires_at <= NOW()",
    )
    .execute(&mut *conn)
    .await?;
    Ok(deleted)
}

/// The ticket cookie (`max_age` 0 clears it).
#[must_use]
pub fn ticket_cookie(value: &str, max_age: i64, config: &Config) -> Cookie<'static> {
    let mut c = Cookie::build((
        super::cookies::cookie_name(TICKET_COOKIE, config),
        value.to_owned(),
    ))
    .http_only(true)
    .same_site(SameSite::Strict)
    .path(TICKET_COOKIE_PATH)
    .secure(config.cookie_secure)
    .max_age(time::Duration::seconds(max_age))
    .build();
    if let Some(domain) = &config.cookie_domain {
        c.set_domain(domain.clone());
    }
    c
}

/// Whether the account is a system admin (the first admin may not bind
/// only TOTP, F8).
async fn is_admin(conn: &mut PgConnection, user_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM system_admins WHERE user_id = $1)")
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await
}

// ============================================================================
// Routes
// ============================================================================

/// `GET /auth/registration` answer.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TicketStatus {
    pub expires_at: DateTime<Utc>,
    pub bound: bool,
}

/// The ticket's state.
#[utoipa::path(
    get,
    path = "/auth/registration",
    tag = "auth",
    responses((status = 200, body = TicketStatus), (status = 404, description = "REGISTRATION_TICKET_INVALID")),
)]
#[tracing::instrument(skip_all)]
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<Json<TicketStatus>> {
    let t = ticket_from_request(&AuthCtx::from(&state), &headers, &jar).await?;
    Ok(Json(TicketStatus {
        expires_at: t.expires_at,
        bound: t.bound_at.is_some(),
    }))
}

/// `POST /auth/registration/totp` answer.
#[derive(Serialize, utoipa::ToSchema)]
pub struct TotpSetup {
    /// Base32 secret.
    pub secret: String,
    pub otpauth_uri: String,
}

impl std::fmt::Debug for TotpSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TotpSetup { .. }")
    }
}

/// A pending TOTP secret for `user` (5 minutes in Valkey, encrypted), and
/// its setup answer.
pub async fn pending_totp(
    state: &AppState,
    user_id: Uuid,
    username: &str,
) -> AuthResult<TotpSetup> {
    use totp_rs::{Algorithm, Secret, TOTP};
    let key = state
        .config
        .mfa_encryption_key
        .as_ref()
        .and_then(|k| hex::decode(k).ok())
        .filter(|k| k.len() == 32)
        .ok_or_else(|| AuthError::Internal("MFA encryption not configured".into()))?;
    let secret = Secret::default();
    let secret_str = secret.to_encoded().to_string();
    let encrypted = super::mfa_crypto::encrypt_mfa_secret(&secret_str, &key)
        .map_err(|e| AuthError::Internal(format!("encrypt TOTP secret: {e}")))?;
    state
        .redis
        .set::<(), _, _>(
            format!("mfa:pending:{user_id}"),
            encrypted.as_str(),
            Some(Expiration::EX(300)),
            None,
            false,
        )
        .await
        .map_err(|_| AuthError::ServiceUnavailable)?;
    let totp = TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret
            .to_bytes()
            .map_err(|_| AuthError::Internal("TOTP secret".into()))?,
        Some("Kaiku".to_string()),
        username.to_string(),
    )
    .map_err(|e| AuthError::Internal(format!("TOTP: {e}")))?;
    Ok(TotpSetup {
        secret: secret_str,
        otpauth_uri: totp.get_url(),
    })
}

/// Start binding TOTP to a new account (not for the first admin: 403
/// `TOTP_UNAVAILABLE`).
#[utoipa::path(
    post,
    path = "/auth/registration/totp",
    tag = "auth",
    responses((status = 200, body = TotpSetup), (status = 403, description = "TOTP_UNAVAILABLE")),
)]
#[tracing::instrument(skip_all)]
pub async fn totp_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<Json<TotpSetup>> {
    let ctx = AuthCtx::from(&state);
    let t = unbound_ticket(&ctx, &headers, &jar).await?;
    let mut conn = state.db.acquire().await?;
    if is_admin(&mut conn, t.user_id).await? {
        return Err(AuthError::TotpUnavailable);
    }
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(t.user_id)
        .fetch_one(&mut *conn)
        .await?;
    drop(conn);
    Ok(Json(pending_totp(&state, t.user_id, &username).await?))
}

/// `POST …/totp/verify` body.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct CodeRequest {
    pub code: String,
}

impl std::fmt::Debug for CodeRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CodeRequest { .. }")
    }
}

/// `POST …/totp/verify` answer: the recovery codes, shown once.
#[derive(Serialize, utoipa::ToSchema)]
pub struct RecoveryCodesOnce {
    pub recovery_codes: Vec<String>,
}

impl std::fmt::Debug for RecoveryCodesOnce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryCodesOnce { .. }")
    }
}

/// Confirm the pending TOTP secret: TOTP and recovery codes are set, the
/// account gets `bootstrap_open` (it signs in by F4), the ticket is bound.
#[utoipa::path(
    post,
    path = "/auth/registration/totp/verify",
    tag = "auth",
    request_body = CodeRequest,
    responses((status = 200, body = RecoveryCodesOnce), (status = 401, description = "INVALID_MFA")),
)]
#[tracing::instrument(skip_all)]
pub async fn totp_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<CodeRequest>,
) -> AuthResult<(CookieJar, Json<RecoveryCodesOnce>)> {
    let ctx = AuthCtx::from(&state);
    let t = unbound_ticket(&ctx, &headers, &jar).await?;
    let key = format!("mfa:pending:{}", t.user_id);
    let pending: Option<String> = state
        .redis
        .get(&key)
        .await
        .map_err(|_| AuthError::ServiceUnavailable)?;
    let encrypted = pending
        .ok_or_else(|| AuthError::Validation("Start the authenticator setup first".into()))?;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(t.user_id)
        .fetch_one(&state.db)
        .await?;
    if !super::reauth::accept_totp_ctx(&ctx, t.user_id, &encrypted, &username, body.code.trim())
        .await?
    {
        return Err(AuthError::InvalidMfaCode);
    }
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[t.user_id]).await?;
    if is_admin(&mut tx, t.user_id).await? {
        return Err(AuthError::TotpUnavailable);
    }
    still_unbound(&mut tx, t.id).await?;
    let generation =
        super::factors::replace_tx(&mut tx, &ctx, t.user_id, super::factors::Factor::Totp).await?;
    // Fable m1: the entitlement only while no strong authenticator is active.
    sqlx::query(
        "UPDATE users SET mfa_secret = $2,
                bootstrap_open = NOT EXISTS (SELECT 1 FROM approval_devices
                                              WHERE user_id = $1 AND state = 'active')
                             AND NOT EXISTS (SELECT 1 FROM passkeys
                                              WHERE user_id = $1 AND state = 'active')
          WHERE id = $1",
    )
    .bind(t.user_id)
    .bind(&encrypted)
    .execute(&mut *tx)
    .await?;
    let codes = super::recovery_codes::replace_tx(&mut tx, &ctx, t.user_id).await?;
    super::strong::mark_first_activity(&mut tx, t.user_id).await?;
    mark_bound_tx(&mut tx, t.user_id).await?;
    let events = vec![
        record(
            &mut tx,
            NewSecurityEvent::new(
                t.user_id,
                EventType::AuthenticatorAdded,
                Actor::User(t.user_id),
                serde_json::json!({"type": "totp", "generation": generation}),
            ),
        )
        .await?,
        record(
            &mut tx,
            NewSecurityEvent::new(
                t.user_id,
                EventType::RecoveryCodesCreated,
                Actor::User(t.user_id),
                serde_json::json!({}),
            ),
        )
        .await?,
    ];
    tx.commit().await?;
    let _ = state.redis.del::<(), _>(&key).await;
    spawn_delivery(&state, events);
    Ok((
        jar.add(ticket_cookie("", 0, &state.config)),
        Json(RecoveryCodesOnce {
            recovery_codes: codes,
        }),
    ))
}

/// Start registering the new account's passkey (C29, `active`).
#[utoipa::path(
    post,
    path = "/auth/registration/passkey/start",
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
    let t = unbound_ticket(&AuthCtx::from(&state), &headers, &jar).await?;
    let label = clean_label(body.label.as_deref())?;
    let (ceremony_id, ccr) = begin(
        &state,
        t.user_id,
        RegistrationBinding::Ticket { ticket_id: t.id },
        label,
    )
    .await?;
    Ok(Json(crate::auth::passkeys::types::CeremonyStart {
        ceremony_id,
        options: serde_json::to_value(&ccr).map_err(|e| AuthError::Internal(e.to_string()))?,
    }))
}

/// Finish the new account's passkey → 201 `Passkey` (`active`); the ticket
/// is bound.
#[utoipa::path(
    post,
    path = "/auth/registration/passkey/finish",
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
) -> AuthResult<(
    StatusCode,
    CookieJar,
    Json<crate::auth::passkeys::types::PasskeyView>,
)> {
    use crate::auth::passkeys::ceremony::{self, CeremonyKind, CeremonyState};
    use crate::auth::passkeys::queries::PasskeyState;
    use crate::auth::passkeys::register::{
        complete_recorded, RegistrationBinding, RegistrationVia,
    };
    let rp = crate::auth::passkeys::rp_of(&state)?;
    let t = unbound_ticket(&AuthCtx::from(&state), &headers, &jar).await?;
    let rec = ceremony::take(&state.redis, body.ceremony_id)
        .await?
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (
        CeremonyKind::Register {
            user_id,
            binding: RegistrationBinding::Ticket { ticket_id },
            label,
        },
        CeremonyState::Register(st),
    ) = (rec.kind, rec.state)
    else {
        return Err(AuthError::PasskeyCeremonyInvalid);
    };
    if user_id != t.user_id || ticket_id != t.id {
        return Err(AuthError::PasskeyCeremonyInvalid);
    }
    let reg = serde_json::from_value(body.credential).map_err(|_| AuthError::PasskeyInvalid)?;
    let verified = rp
        .finish_registration(&reg, &st)
        .map_err(crate::auth::passkeys::failure)?;
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[t.user_id]).await?;
    still_unbound(&mut tx, t.id).await?;
    let (row, added) = complete_recorded(
        &mut tx,
        t.user_id,
        &label,
        &verified,
        PasskeyState::Active,
        None,
        RegistrationVia::Ticket,
    )
    .await?;
    mark_bound_tx(&mut tx, t.user_id).await?;
    // An active passkey consumes the bootstrap entitlement (F8 rev 4).
    crate::auth::account_requests::core::on_strong_activated(
        &mut tx,
        &AuthCtx::from(&state),
        t.user_id,
        None,
    )
    .await?;
    tx.commit().await?;
    spawn_delivery(&state, vec![added]);
    Ok((
        StatusCode::CREATED,
        jar.add(ticket_cookie("", 0, &state.config)),
        Json(crate::auth::passkeys::types::PasskeyView::from(&row)),
    ))
}

// ---------------------------------------------------------------------------
// Enrolment issuer routes of a ticket (variant `registration`, C26)
// ---------------------------------------------------------------------------

const fn approval(e: AuthError) -> ApprovalError {
    ApprovalError::Auth(e)
}

/// `POST /auth/registration/enrollments` → 201 `EnrollmentIssued`.
#[utoipa::path(
    post,
    path = "/auth/registration/enrollments",
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
    let t = unbound_ticket(&ctx, &headers, &jar)
        .await
        .map_err(approval)?;
    let mut tx = state.db.begin().await?;
    let issued = enrollments::issue_tx(
        &mut tx,
        &ctx,
        IssueParams {
            variant: EnrollmentVariant::Registration,
            subject_id: t.user_id,
            issuer: Issuer::Ticket { ticket_id: t.id },
            claimant: crate::approvals::context::ClaimantCheck::None,
            claimant_note: None,
            lost_device_ids: Vec::new(),
        },
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(issued)))
}

async fn ticket_enrollment(
    state: &AppState,
    t: &TicketRow,
    id: Uuid,
) -> Result<enrollments::EnrollmentRow, ApprovalError> {
    let row: Option<enrollments::EnrollmentRow> = sqlx::query_as(
        "SELECT * FROM enrollments WHERE id = $1 AND issuer_kind = 'ticket' AND issuer_ticket_id = $2",
    )
    .bind(id)
    .bind(t.id)
    .fetch_optional(&state.db)
    .await?;
    row.ok_or(ApprovalError::NotFound)
}

/// `GET /auth/registration/enrollments/{id}` → `Enrollment`.
#[utoipa::path(
    get,
    path = "/auth/registration/enrollments/{id}",
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
    let t = ticket_from_request(&AuthCtx::from(&state), &headers, &jar)
        .await
        .map_err(approval)?;
    let row = ticket_enrollment(&state, &t, id).await?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(enrollments::to_wire(&mut conn, &row).await?))
}

/// `POST /auth/registration/enrollments/{id}/confirm {code}` → `Enrollment`.
#[utoipa::path(
    post,
    path = "/auth/registration/enrollments/{id}/confirm",
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
    let t = unbound_ticket(&ctx, &headers, &jar)
        .await
        .map_err(approval)?;
    ticket_enrollment(&state, &t, id).await?;
    let code: String = body.code.chars().filter(|c| !c.is_whitespace()).collect();
    let mut tx = state.db.begin().await?;
    let r =
        enrollments::confirm_tx(&mut tx, &ctx, id, Confirmer::Ticket(t.id), code.as_bytes()).await;
    match r {
        Ok((enrollment, after)) => {
            tx.commit().await?;
            enrollments::after_done(&state, after).await;
            Ok(Json(enrollment))
        }
        Err(e @ (ApprovalError::EnrollCodeWrong { .. } | ApprovalError::EnrollmentFailed)) => {
            tx.commit().await?;
            Err(e)
        }
        Err(e @ ApprovalError::EnrollmentState) => {
            tx.commit().await?;
            Err(e)
        }
        Err(e) => Err(e),
    }
}

/// `DELETE /auth/registration/enrollments/{id}` → 204.
#[utoipa::path(
    delete,
    path = "/auth/registration/enrollments/{id}",
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
    let t = unbound_ticket(&ctx, &headers, &jar)
        .await
        .map_err(approval)?;
    ticket_enrollment(&state, &t, id).await?;
    let mut tx = state.db.begin().await?;
    crate::approvals::transition::lock_accounts(&mut tx, &[t.user_id]).await?;
    enrollments::cancel_tx(&mut tx, &ctx, id, enrollments::EnrollmentCanceller::Issuer).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The ticket routes under `/auth` (`AuthFlow` per IP, never IP-blocked).
pub fn router(state: &AppState) -> axum::Router<AppState> {
    use axum::middleware::{from_fn, from_fn_with_state};
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/registration", get(status))
        .route("/registration/totp", post(totp_start))
        .route("/registration/totp/verify", post(totp_verify))
        .route("/registration/passkey/start", post(passkey_start))
        .route("/registration/passkey/finish", post(passkey_finish))
        .route("/registration/enrollments", post(enrollment_issue))
        .route(
            "/registration/enrollments/{id}",
            get(enrollment_get).delete(enrollment_cancel),
        )
        .route(
            "/registration/enrollments/{id}/confirm",
            post(enrollment_confirm),
        )
        .layer(from_fn_with_state(
            state.clone(),
            crate::ratelimit::rate_limit_by_ip,
        ))
        .layer(from_fn(crate::ratelimit::with_category(
            crate::ratelimit::RateLimitCategory::AuthFlow,
        )))
}
