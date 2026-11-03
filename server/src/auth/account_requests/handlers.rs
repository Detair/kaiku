//! Account-request routes (C27 "Account requests").

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use uuid::Uuid;

use super::core::{self, NewRequest, Purpose, RequestKind};
use super::types::{
    AccountRequest, AccountRequestSummary, CancelRequest, StartFactor, StartRequest, StartResponse,
    SummaryList,
};
use crate::api::AppState;
use crate::approvals::secrets::Secret256;
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::error::{AuthError, AuthResult};
use crate::auth::factors::{self, Factor, FactorRef};
use crate::auth::middleware::AuthUser;
use crate::auth::owner_rule::{self, OwnerTarget, Part2Factor};
use crate::auth::step_up::{StepUpAction, StepUpTarget};
use crate::config::Config;
use crate::security_events::spawn_delivery;

/// The browser's request cookie and its path (C27 common rules).
pub const REQUEST_COOKIE: &str = "__Secure-kaiku_request";
pub const REQUEST_COOKIE_PATH: &str = "/auth/account-requests";
/// The native header.
pub const REQUEST_HEADER: &str = "x-kaiku-request-secret";

/// The request cookie (`max_age` 0 clears it).
#[must_use]
pub fn request_cookie(value: &str, max_age: i64, config: &Config) -> Cookie<'static> {
    let mut c = Cookie::build((
        crate::auth::cookies::cookie_name(REQUEST_COOKIE, config),
        value.to_owned(),
    ))
    .http_only(true)
    .same_site(SameSite::Strict)
    .path(REQUEST_COOKIE_PATH)
    .secure(config.cookie_secure)
    .max_age(time::Duration::seconds(max_age))
    .build();
    if let Some(domain) = &config.cookie_domain {
        c.set_domain(domain.clone());
    }
    c
}

/// The request secret the request presents (header or cookie); else 404 as
/// an unknown request.
pub fn request_secret(
    headers: &HeaderMap,
    jar: &CookieJar,
    config: &Config,
) -> AuthResult<Secret256> {
    headers
        .get(REQUEST_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            jar.get(crate::auth::cookies::cookie_name(REQUEST_COOKIE, config))
                .map(|c| c.value().to_string())
        })
        .and_then(|s| Secret256::from_b64u(&s))
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))
}

/// Notify the subject, its contact and the admins that a request changed.
pub async fn request_updated(state: &AppState, request_id: Uuid, subject_id: Uuid, st: &str) {
    let mut audience = vec![subject_id];
    if let Ok(rows) = sqlx::query_scalar::<_, Uuid>(
        "SELECT recovery_contact_id FROM users
          WHERE id = $1 AND recovery_contact_id IS NOT NULL
            AND recovery_contact_confirmed_at IS NOT NULL
         UNION SELECT user_id FROM system_admins",
    )
    .bind(subject_id)
    .fetch_all(&state.db)
    .await
    {
        audience.extend(rows);
    }
    audience.sort_unstable();
    audience.dedup();
    for user in audience {
        if let Err(e) = crate::ws::broadcast_to_user(
            &state.redis,
            user,
            &crate::ws::ServerEvent::AccountRequestUpdated {
                request_id,
                subject_id,
                state: st.to_string(),
            },
        )
        .await
        {
            tracing::warn!(error = %e, "account_request_updated hint failed");
        }
    }
}

/// Verify a password off the runtime (dummy hash when none).
async fn password_ok(hash: Option<String>, password: &str) -> AuthResult<bool> {
    let password = password.to_string();
    let hash = hash.unwrap_or_else(|| crate::auth::password::DUMMY_PASSWORD_HASH.clone());
    let has_real = !hash.is_empty();
    tokio::task::spawn_blocking(move || {
        has_real && crate::auth::verify_password(&password, &hash).unwrap_or(false)
    })
    .await
    .map_err(|e| AuthError::Internal(format!("password check failed: {e}")))
}

/// Start a request (C27): bootstrap here; recovery in Task 27.
#[utoipa::path(
    post,
    path = "/auth/account-requests",
    tag = "auth",
    request_body = StartRequest,
    responses(
        (status = 201, body = StartResponse),
        (status = 401, description = "INVALID_CREDENTIALS or REQUEST_FACTOR_INVALID"),
        (status = 409, description = "ACCOUNT_REQUEST_OPEN or NO_INDEPENDENT_CANCELLER"),
    ),
)]
#[tracing::instrument(skip_all)]
pub async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<StartRequest>,
) -> AuthResult<(StatusCode, CookieJar, Json<StartResponse>)> {
    let ctx = AuthCtx::from(&state);
    let started = match body.purpose.as_str() {
        "bootstrap" => start_bootstrap(&state, &ctx, &body).await?,
        "recovery" => super::recovery::start_recovery(&state, &ctx, &body).await?,
        _ => return Err(AuthError::Validation("purpose".into())),
    };
    let mut conn = state.db.acquire().await?;
    let request = core::to_wire(&mut conn, &started.row).await?;
    drop(conn);
    spawn_delivery(&state, started.event_ids.clone());
    request_updated(&state, started.row.id, started.row.user_id, "waiting").await;
    let native = ClientClass::from_headers(&headers) != ClientClass::Web;
    let secret = started.request_secret.as_ref().map(Secret256::to_b64u);
    let jar = match (&secret, native) {
        (Some(s), false) => jar.add(request_cookie(s, 4 * 24 * 3600, &state.config)),
        _ => jar,
    };
    Ok((
        StatusCode::CREATED,
        jar,
        Json(StartResponse {
            request,
            request_secret: if native { secret } else { None },
        }),
    ))
}

/// Bootstrap (F8 "Secure my account"): `bootstrap_open`, the password
/// (`active`) and an `active` TOTP code; every failure is the neutral 401.
///
/// Review round 1: the password and TOTP counters (B.5) apply, the start
/// counts on the shared budget of three per delay period (SM3), the
/// credentials are re-read under the account lock (an old one is never
/// recorded as a newer generation), and a banned account gets the neutral
/// answer.
async fn start_bootstrap(
    state: &AppState,
    ctx: &AuthCtx<'_>,
    body: &StartRequest,
) -> AuthResult<core::StartedRequest> {
    use crate::ratelimit::account::{AccountLimiter, CounterKey};
    let neutral = || AuthError::InvalidCredentials;
    let password_key = CounterKey::password(&body.username);
    if matches!(body.first, Some(StartFactor::Password { .. })) {
        match AccountLimiter::new(ctx.redis).check(&password_key).await {
            Ok(Ok(())) => {}
            Ok(Err(d)) => {
                return Err(AuthError::TooManyAttempts {
                    retry_after: d.retry_after,
                })
            }
            Err(_) => return Err(AuthError::ServiceUnavailable),
        }
    }
    let user = crate::db::find_user_by_username(&state.db, &body.username).await?;
    let verified_hash = user.as_ref().and_then(|u| u.password_hash.clone());
    // Fable r2 m-d: before a one-time OIDC code is spent, the TOTP delay
    // and the start budget (neutral answers, as for every refusal here).
    if let (Some(StartFactor::Oidc { .. }), Some(u)) = (&body.first, &user) {
        if crate::auth::totp_guard::pre_check(ctx, u.id).await.is_err()
            || starts_in_period(&state.db, ctx, u.id).await? >= super::recovery::STARTS_PER_PERIOD
        {
            return Err(neutral());
        }
    }
    let (first_ok, oidc) = match &body.first {
        Some(StartFactor::Password { password }) => {
            let ok = password_ok(verified_hash.clone(), password).await?;
            if !ok {
                crate::auth::login::account_failure(
                    state,
                    &password_key,
                    user.as_ref().filter(|u| !u.is_system).map(|u| u.id),
                )
                .await?;
            }
            (ok, false)
        }
        Some(StartFactor::Oidc {
            oidc_code,
            verifier,
        }) => {
            let f = super::recovery::redeem_oidc_code(ctx, oidc_code, verifier.as_deref()).await;
            (
                matches!((&f, &user), (Ok(f), Some(u)) if f.user_id == u.id),
                true,
            )
        }
        _ => {
            password_ok(None, "").await?;
            (false, false)
        }
    };
    let Some(user) = user.filter(|u| !u.is_system) else {
        return Err(neutral());
    };
    if !first_ok || crate::auth::queries::is_user_globally_banned(&state.db, user.id).await? {
        return Err(neutral());
    }
    // Fable r2 (i): the delay is only reachable after a correct first
    // factor, so it answers neutrally too.
    if crate::auth::totp_guard::pre_check(ctx, user.id)
        .await
        .is_err()
    {
        return Err(neutral());
    }
    let mut tx = state.db.begin().await?;
    // The subject with every possible canceller (Codex M7).
    crate::approvals::transition::lock_for_start(&mut tx, user.id, &[])
        .await
        .map_err(transition_err)?;
    // The credentials under the lock (Codex B2).
    let (hash_now, secret_now): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT password_hash, mfa_secret FROM users WHERE id = $1")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await?;
    if !oidc && hash_now != verified_hash {
        return Err(neutral());
    }
    // The shared start budget, before the second factor (SM3, B.5).
    let starts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_request_starts
          WHERE user_id = $1 AND started_at > NOW() - make_interval(hours => $2)",
    )
    .bind(user.id)
    .bind(i32::try_from(ctx.config.account_request_delay_hours).unwrap_or(24))
    .fetch_one(&mut *tx)
    .await?;
    if starts >= super::recovery::STARTS_PER_PERIOD {
        tracing::warn!(user_id = %user.id, "bootstrap start budget exhausted (B.5)");
        return Err(neutral());
    }
    sqlx::query("INSERT INTO account_request_starts (user_id) VALUES ($1)")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    let st = factors::states(&mut tx, user.id).await?;
    let password_used = (!oidc).then_some(FactorRef {
        factor: Factor::Password,
        generation: st.password.generation,
    });
    let totp_ok = match (&body.totp_code, &secret_now) {
        (Some(code), Some(secret)) if st.totp.state == factors::FactorState::Active => {
            let ok =
                crate::auth::reauth::accept_totp_ctx(ctx, user.id, secret, &user.username, code)
                    .await?;
            crate::auth::totp_guard::after(
                ctx,
                Some(state),
                user.id,
                ok,
                crate::auth::totp_guard::Used {
                    totp_generation: Some(st.totp.generation),
                    password: password_used,
                },
            )
            .await;
            ok
        }
        _ => false,
    };
    let password_active =
        oidc || (st.has_password && st.password.state == factors::FactorState::Active);
    if !(st.bootstrap_open && password_active && totp_ok) {
        // The start stays counted.
        tx.commit().await?;
        return Err(neutral());
    }
    let mut used = vec![FactorRef {
        factor: Factor::Totp,
        generation: st.totp.generation,
    }];
    used.extend(password_used);
    let started = core::insert_tx(
        &mut tx,
        ctx,
        NewRequest {
            user_id: user.id,
            kind: RequestKind::SelfService,
            purpose: Purpose::Bootstrap,
            factors: used,
            oidc,
            lost_device_ids: Vec::new(),
            started_by: None,
            started_by_device: None,
            enrollment_id: None,
            pending: None,
            with_secret: true,
        },
    )
    .await;
    match started {
        Ok(s) => {
            tx.commit().await?;
            Ok(s)
        }
        Err(e) => {
            tx.commit().await?;
            Err(e)
        }
    }
}

/// The request client's view (request secret).
#[utoipa::path(
    get,
    path = "/auth/account-requests/current",
    tag = "auth",
    responses((status = 200, body = AccountRequest), (status = 404)),
)]
#[tracing::instrument(skip_all)]
pub async fn current(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<Json<AccountRequest>> {
    let ctx = AuthCtx::from(&state);
    let secret = request_secret(&headers, &jar, &state.config)?;
    let mut conn = state.db.acquire().await?;
    let row = core::by_secret(&mut conn, &ctx, &secret, false)
        .await?
        .ok_or_else(|| AuthError::NotFound("Unknown request".into()))?;
    Ok(Json(core::to_wire(&mut conn, &row).await?))
}

/// Withdraw (the request client).
#[utoipa::path(
    delete,
    path = "/auth/account-requests/current",
    tag = "auth",
    responses((status = 204), (status = 404)),
)]
#[tracing::instrument(skip_all)]
pub async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> AuthResult<(CookieJar, StatusCode)> {
    let ctx = AuthCtx::from(&state);
    let secret = request_secret(&headers, &jar, &state.config)?;
    let mut tx = state.db.begin().await?;
    let events = core::withdraw_tx(&mut tx, &ctx, &secret).await?;
    let row = core::by_secret(&mut tx, &ctx, &secret, false).await?;
    tx.commit().await?;
    spawn_delivery(&state, events);
    if let Some(r) = row {
        request_updated(&state, r.id, r.user_id, "withdrawn").await;
    }
    Ok((
        jar.add(request_cookie("", 0, &state.config)),
        StatusCode::NO_CONTENT,
    ))
}

/// Open requests on the caller's account and those it may cancel as the
/// contact (any session).
#[utoipa::path(
    get,
    path = "/api/me/account-requests",
    tag = "auth",
    responses((status = 200, body = SummaryList)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn list_own(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AuthResult<Json<SummaryList>> {
    let mut conn = state.db.acquire().await?;
    let rows: Vec<core::RequestRow> = sqlx::query_as(
        "SELECT r.* FROM account_requests r JOIN users u ON u.id = r.user_id
          WHERE r.state IN ('waiting', 'ready')
            AND (r.user_id = $1 OR (u.recovery_contact_id = $1
                                    AND u.recovery_contact_confirmed_at IS NOT NULL))
          ORDER BY r.created_at DESC",
    )
    .bind(auth.id)
    .fetch_all(&mut *conn)
    .await?;
    let mut requests = Vec::with_capacity(rows.len());
    for r in &rows {
        requests.push(core::to_summary(&mut conn, r, auth.id).await?);
    }
    Ok(Json(SummaryList { requests }))
}

/// `GET /api/admin/account-requests?state=open` query.
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct AdminListQuery {
    #[serde(default)]
    pub state: Option<String>,
}

/// Every open request (elevated admins).
#[utoipa::path(
    get,
    path = "/api/admin/account-requests",
    tag = "admin",
    params(AdminListQuery),
    responses((status = 200, body = SummaryList)),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn list_admin(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<AdminListQuery>,
) -> AuthResult<Json<SummaryList>> {
    let open_only = q.state.as_deref().is_none_or(|s| s == "open");
    let mut conn = state.db.acquire().await?;
    let rows: Vec<core::RequestRow> = sqlx::query_as(
        "SELECT * FROM account_requests
          WHERE ($1 = false OR state IN ('waiting', 'ready'))
          ORDER BY created_at DESC LIMIT 200",
    )
    .bind(open_only)
    .fetch_all(&mut *conn)
    .await?;
    let mut requests = Vec::with_capacity(rows.len());
    for r in &rows {
        requests.push(core::to_summary(&mut conn, r, auth.id).await?);
    }
    Ok(Json(SummaryList { requests }))
}

/// Cancel (B.2 owner rule, the contact, admins) → 200 `AccountRequestSummary`.
#[utoipa::path(
    post,
    path = "/api/account-requests/{id}/cancel",
    tag = "auth",
    params(("id" = Uuid, Path, description = "Request id")),
    request_body = CancelRequest,
    responses(
        (status = 200, body = AccountRequestSummary),
        (status = 403, description = "CANCEL_NOT_ALLOWED, FULL_SESSION_REQUIRED or STEP_UP_REQUIRED"),
        (status = 409, description = "ACCOUNT_REQUEST_COMPLETED"),
    ),
    security(("bearer_auth" = [])),
)]
#[tracing::instrument(skip_all, fields(user_id = %auth.id, request_id = %id))]
pub async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    body: Option<Json<CancelRequest>>,
) -> AuthResult<Json<AccountRequestSummary>> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let ctx = AuthCtx::from(&state);
    let part2 = match body.part2 {
        Some(p) => Some(match p.factor.kind.as_str() {
            "totp" => Part2Factor::Totp(p.factor.code),
            "recovery_code" => Part2Factor::RecoveryCode(p.factor.code),
            _ => return Err(AuthError::Validation("part2.factor.type".into())),
        }),
        None => None,
    };
    let mut tx = state.db.begin().await?;
    let row = core::lock_for_cancel(&mut tx, id, auth.id).await?;
    if row.state == "completed" {
        return Err(AuthError::AccountRequestCompleted);
    }
    if !row.is_open() {
        return Err(AuthError::CancelNotAllowed);
    }
    let by = owner_rule::authorize(
        &mut tx,
        &ctx,
        &auth,
        &OwnerTarget {
            subject: row.user_id,
            action: StepUpAction::RequestCancel,
            target: StepUpTarget::Id(row.id),
            event_at: None,
            created_by_event: Vec::new(),
        },
        part2,
        || AuthError::CancelNotAllowed,
    )
    .await?;
    let events =
        core::cancel_locked_tx(&mut tx, &ctx, &row, &by, auth.id, body.reason.as_deref()).await?;
    let done = core::get(&mut tx, id)
        .await?
        .ok_or(AuthError::CancelNotAllowed)?;
    let summary = core::to_summary(&mut tx, &done, auth.id).await?;
    tx.commit().await?;
    spawn_delivery(&state, events);
    crate::security_events::notices::spawn_pending_delivery(&state);
    request_updated(&state, row.id, row.user_id, "cancelled").await;
    Ok(Json(summary))
}

fn transition_err(e: crate::approvals::transition::TransitionError) -> AuthError {
    match e {
        crate::approvals::transition::TransitionError::Db(d) => AuthError::Database(d),
        _ => AuthError::ServiceUnavailable,
    }
}

/// `self` starts of `user` in the current delay period (B.5 budget).
async fn starts_in_period(db: &sqlx::PgPool, ctx: &AuthCtx<'_>, user: Uuid) -> AuthResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_request_starts
          WHERE user_id = $1 AND started_at > NOW() - make_interval(hours => $2)",
    )
    .bind(user)
    .bind(i32::try_from(ctx.config.account_request_delay_hours).unwrap_or(24))
    .fetch_one(db)
    .await?)
}
