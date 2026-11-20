//! The `pc_setup` purpose of the desktop flow ("Set up this PC", spec Step 5,
//! desktop part).
//!
//! **SECURITY CRITICAL.** The app starts the flow with the requesting
//! session's bearer and opens the system browser at
//! `/auth/passkeys/desktop/flows/{flow_id}`. That page asks for the phone's
//! code before any ceremony; wrong codes count on the attempt (SM5). The
//! right code answers with a browser secret: a fresh 256-bit value in the
//! `__Secure-kaiku_pcsetup` cookie (`HttpOnly`, `Secure`, `SameSite=Strict`,
//! path = the flow's routes), stored only as its keyed hash in the flow
//! record. `options`, `finish`, `cancel` and the passkey page refuse without
//! it, and code entry works once per flow, so the authorisation belongs to
//! the browser that typed the code, never to the URL. `code`, `options`,
//! `finish` and `cancel` also accept only the page's own origin (403
//! otherwise, the flow untouched).
//!
//! `finish` only verifies the registration and keeps it in the pending
//! result (60 s); `redeem` with the hand-off code, the verifier and the
//! starting session's bearer stores the passkey through
//! [`complete_enrolment`] under the locks of Locking 3. A restart (or any
//! end of the attempt) voids the flow: `finish` and `redeem` re-lock the
//! attempt and refuse it, so a stale flow never stores a passkey.

use axum::extract::{Path, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Form, Json};
use serde::Serialize;
use uuid::Uuid;

use super::ceremony::{self, CeremonyKind, CeremonyRecord, CeremonyState};
use super::desktop::{
    cancel_flow, cas, fail_with, install_ceremony, load, DesktopFlow, FinishForm, FlowOptions,
    FlowOutcome, FlowPurpose, PendingResult, Stored, PENDING_RESULT_TTL_SECS,
};
use super::desktop_page::{
    expired_page, handoff_page, pc_setup_page, refused_page, stale_page, unavailable_page,
    HandoffField, Lang, PcSetupStep,
};
use super::register::options_for;
use super::rp::VerifiedRegistration;
use super::types::PasskeyView;
use super::{failure, rp_of};
use crate::api::AppState;
use crate::approvals::error::ApprovalError;
use crate::approvals::pc_setup::enrolment::{self, CodeOutcome, CodePath, Credential};
use crate::approvals::pc_setup::handlers::{settle, PcSetupCodeEntry};
use crate::approvals::pc_setup::transition::{self, LockedAttempt};
use crate::approvals::pc_setup::AttemptState;
use crate::approvals::secrets::{ct_eq, keyed_hash, Secret256, SecretPurpose};
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::oidc::new_handoff_code;
use crate::auth::{AuthError, AuthResult, AuthUser};

/// The browser-secret cookie. Without `COOKIE_SECURE` (local development)
/// the `__Secure-` prefix and `Secure` are dropped, as for every cookie.
pub const PCSETUP_COOKIE: &str = "__Secure-kaiku_pcsetup";

/// The flow's routes: the page, `code`, `options`, `finish`, `cancel`.
fn flow_path(flow_id: Uuid) -> String {
    format!("/auth/passkeys/desktop/flows/{flow_id}")
}

/// The purpose of a `pc_setup` start: the attempt must be `auth`'s
/// session's (another session's or an unknown one → 404) and wait for its
/// code. Nothing is locked: code entry re-checks everything under the locks.
pub(super) async fn purpose(
    state: &AppState,
    auth: &AuthUser,
    attempt_id: Uuid,
) -> AuthResult<FlowPurpose> {
    let mut conn = state.db.acquire().await?;
    let row = match transition::of_session(&mut conn, auth, attempt_id).await {
        Ok(row) => row,
        Err(ApprovalError::NotFound) => {
            return Err(AuthError::NotFound("pc setup attempt".into()));
        }
        Err(e) => return Err(to_auth(e)),
    };
    // Only the desktop app's own session (its stored client class, not a
    // request header) types the code in the system browser.
    if auth.session.as_ref().map(|s| s.client_class) != Some(ClientClass::Desktop) {
        return Err(AuthError::Forbidden);
    }
    if row.state() != AttemptState::Code {
        return Err(AuthError::Validation(
            "the attempt is not waiting for its code".into(),
        ));
    }
    Ok(FlowPurpose::PcSetup {
        user_id: row.user_id,
        session_id: row.session_id,
        attempt_id,
    })
}

fn to_auth(e: ApprovalError) -> AuthError {
    match e {
        ApprovalError::Auth(e) => e,
        ApprovalError::Database(e) => AuthError::Database(e),
        ApprovalError::Unavailable => AuthError::ServiceUnavailable,
        _ => AuthError::PasskeyHandoffInvalid,
    }
}

/// Whether `e` is a check's refusal (the attempt can no longer register),
/// which ends the flow, rather than a transient failure (database, Valkey,
/// internal), after which the flow stays for a retry: the code is spent,
/// so a new flow could not be unlocked.
const fn is_refusal(e: &ApprovalError) -> bool {
    !matches!(
        e,
        ApprovalError::Database(_)
            | ApprovalError::Unavailable
            | ApprovalError::Auth(
                AuthError::Database(_) | AuthError::ServiceUnavailable | AuthError::Internal(_)
            )
    )
}

/// The hand-off error code for an attempt that can no longer register.
const fn refusal_code(e: &ApprovalError) -> &'static str {
    match e {
        ApprovalError::Auth(e) => e.status_and_code().1,
        ApprovalError::Expired => "APPROVAL_EXPIRED",
        _ => "APPROVAL_STATE",
    }
}

/// `303` back to the flow's page after a ceremony that did not finish: the
/// page (GET) shows the passkey step again for the cookie holder, and a
/// reload never re-posts the spent finish form. `?failed` only picks the
/// "That didn't work" text.
fn retry_redirect(flow_id: Uuid) -> Response {
    let mut response = (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, format!("{}?failed", flow_path(flow_id)))],
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `(user_id, session_id, attempt_id)` of a `pc_setup` flow.
const fn ids(flow: &DesktopFlow) -> Option<(Uuid, Uuid, Uuid)> {
    match flow.purpose {
        FlowPurpose::PcSetup {
            user_id,
            session_id,
            attempt_id,
        } => Some((user_id, session_id, attempt_id)),
        _ => None,
    }
}

/// A live `pc_setup` flow without a result.
async fn load_open(state: &AppState, flow_id: Uuid) -> AuthResult<Option<Stored>> {
    Ok(load(state, flow_id)
        .await?
        .filter(|s| s.flow.result.is_none() && ids(&s.flow).is_some()))
}

/// Whether the request carries the browser secret this flow's code entry
/// set (constant-time compare of the keyed hashes).
fn has_secret(state: &AppState, headers: &HeaderMap, flow: &DesktopFlow) -> bool {
    let Some(stored) = flow.browser_secret_hash else {
        return false;
    };
    let name = crate::auth::cookies::cookie_name(PCSETUP_COOKIE, &state.config);
    let jar = axum_extra::extract::cookie::CookieJar::from_headers(headers);
    let Some(secret) = jar.get(name).and_then(|c| Secret256::from_b64u(c.value())) else {
        return false;
    };
    let ctx = AuthCtx::from(state);
    let typed = keyed_hash(
        ctx.hmac_key(),
        SecretPurpose::PcSetupBrowser,
        secret.as_bytes(),
    );
    ct_eq(&typed, &stored)
}

/// Whether a state-changing request comes from the flow's own page: its
/// `Origin` is the configured public origin, or (no `Origin`, or the `null`
/// the page's `no-referrer` policy sends with a form post) the browser says
/// `Sec-Fetch-Site: same-origin`. Checked before the flow is read: the
/// `SameSite=Strict` cookie also travels with requests from other origins
/// of the same site.
fn from_the_page(state: &AppState, headers: &HeaderMap) -> bool {
    let site = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok());
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(origin) if origin != "null" => origin == state.config.public_origin.origin,
        _ => site == Some("same-origin"),
    }
}

/// A page load the browser made on its own or from the page itself
/// (`Sec-Fetch-Site: none` or `same-origin`). A browser that sends no
/// `Sec-Fetch-Site` counts as the page's own.
fn own_navigation(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|site| matches!(site, "none" | "same-origin"))
}

/// `Set-Cookie` for the browser secret: host-only, session lifetime (the
/// flow lives at most 15 minutes), scoped to the flow's routes.
fn secret_cookie(state: &AppState, flow_id: Uuid, secret: &Secret256) -> AuthResult<HeaderValue> {
    let config = &state.config;
    let name = crate::auth::cookies::cookie_name(PCSETUP_COOKIE, config);
    let secure = if config.cookie_secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{name}={}; HttpOnly{secure}; SameSite=Strict; Path={}",
        secret.to_b64u(),
        flow_path(flow_id)
    ))
    .map_err(|e| AuthError::Internal(e.to_string()))
}

/// Lock the flow's attempt (account → requesting session → attempt); the
/// attempt must be the flow's.
async fn lock(
    conn: &mut sqlx::PgConnection,
    (user_id, session_id, attempt_id): (Uuid, Uuid, Uuid),
) -> Result<LockedAttempt, ApprovalError> {
    let locked = transition::lock_attempt(conn, attempt_id).await?;
    if locked.attempt.user_id != user_id || locked.attempt.session_id != session_id {
        return Err(ApprovalError::NotFound);
    }
    Ok(locked)
}

/// The checks of the passkey step under the locks (attempt `enrolled`, its
/// enrolment live, room for a passkey), committed as `settle` does.
async fn registration_open(
    state: &AppState,
    flow_id: Uuid,
    flow: &DesktopFlow,
) -> Result<(), ApprovalError> {
    let ids = ids(flow).ok_or(ApprovalError::NotFound)?;
    let mut tx = state.db.begin().await?;
    let result = async {
        let locked = lock(&mut tx, ids).await?;
        enrolment::registration_open(&mut tx, &locked, CodePath::DesktopFlow(flow_id)).await
    }
    .await;
    settle(tx, result).await.map(|_| ())
}

/// `GET /auth/passkeys/desktop/flows/{flow_id}`: the keypad until the right
/// code, then the passkey step for the browser that typed it. Any other
/// browser gets the "expired" page.
#[tracing::instrument(skip_all)]
pub async fn page(
    State(state): State<AppState>,
    Path(flow_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let lang = Lang::from_headers(&headers);
    let Ok(flow_id) = Uuid::parse_str(&flow_id) else {
        return expired_page(lang);
    };
    // A load the browser marks as coming from another origin gets the expired
    // page at every step and stores nothing.
    if !own_navigation(&headers) {
        return expired_page(lang);
    }
    let stored = match load_open(&state, flow_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return expired_page(lang),
        Err(e) => return e.into_response(),
    };
    if stored.flow.browser_secret_hash.is_none() {
        pc_setup_page(lang, &PcSetupStep::Code, flow_id)
    } else if has_secret(&state, &headers, &stored.flow) {
        // An attempt that can no longer register (the passkey limit, an
        // ended attempt) hands its refusal to the app instead of offering a
        // ceremony that cannot work.
        match registration_open(&state, flow_id, &stored.flow).await {
            Err(e) if is_refusal(&e) => {
                return fail_with(&state, lang, flow_id, &stored, refusal_code(&e)).await;
            }
            _ => {}
        }
        let failed = query.as_deref() == Some("failed");
        pc_setup_page(lang, &PcSetupStep::Passkey { failed }, flow_id)
    } else {
        expired_page(lang)
    }
}

/// `POST /auth/passkeys/desktop/flows/{flow_id}/code {code}` answer.
#[derive(Debug, Serialize)]
struct CodeAnswer {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tries_left: Option<i16>,
}

/// Type the phone's code in the browser (SM5 `code → enrolled | failed`).
///
/// It runs [`enrolment::enter_code`] under the attempt's locks, so wrong
/// codes count on the attempt as on the PC's own route. The right code
/// stores the keyed hash of a fresh browser secret in the flow (before the
/// commit: if the commit fails, the attempt stays in `code`, this flow
/// refuses further entries, and the code works in a new flow once the
/// person started one) and sets the cookie. Once a flow has taken the
/// right code, every further entry is refused (409 `APPROVAL_STATE`).
#[tracing::instrument(skip_all)]
pub async fn code(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<PcSetupCodeEntry>,
) -> Result<Response, ApprovalError> {
    if !from_the_page(&state, &headers) {
        return Err(AuthError::Forbidden.into());
    }
    let stored = load_open(&state, flow_id)
        .await?
        .ok_or(ApprovalError::NotFound)?;
    if stored.flow.browser_secret_hash.is_some() {
        return Err(ApprovalError::State);
    }
    let ids = ids(&stored.flow).ok_or(ApprovalError::NotFound)?;
    let ctx = AuthCtx::from(&state);
    let mut tx = state.db.begin().await?;
    let result = async {
        let locked = lock(&mut tx, ids).await?;
        enrolment::enter_code(
            &mut tx,
            &ctx,
            &locked,
            &body.code,
            CodePath::DesktopFlow(flow_id),
        )
        .await
    }
    .await;
    let outcome = match result {
        Ok(CodeOutcome::Enrolled { .. }) => {
            let secret = Secret256::random();
            let cookie = secret_cookie(&state, flow_id, &secret)?;
            let mut flow = stored.flow.clone();
            flow.browser_secret_hash = Some(keyed_hash(
                ctx.hmac_key(),
                SecretPurpose::PcSetupBrowser,
                secret.as_bytes(),
            ));
            // The flow changed meanwhile (a cancel, another entry): roll
            // back, the code stays unspent.
            // The flow lives as long as the enrolment the code opened.
            let ttl_ms = super::desktop::PENDING_RESULT_TTL_SECS
                .max(crate::approvals::pc_setup::ENROLLMENT_MINUTES * 60)
                * 1000;
            if !cas(&state, flow_id, &stored.raw, Some(&flow), Some(ttl_ms)).await? {
                return Err(ApprovalError::State);
            }
            tx.commit().await?;
            tracing::info!("pc setup code entered in the desktop flow");
            let mut response = Json(CodeAnswer {
                state: "enrolled",
                tries_left: None,
            })
            .into_response();
            response.headers_mut().insert(header::SET_COOKIE, cookie);
            return Ok(no_store(response));
        }
        other => settle(tx, other).await?,
    };
    let answer = match outcome {
        CodeOutcome::Wrong { tries_left } => CodeAnswer {
            state: "code",
            tries_left: Some(tries_left),
        },
        CodeOutcome::Failed => {
            crate::security_events::notices::spawn_pending_delivery(&state);
            CodeAnswer {
                state: "failed",
                tries_left: Some(0),
            }
        }
        CodeOutcome::Enrolled { .. } => unreachable!("handled above"),
    };
    Ok(no_store(Json(answer).into_response()))
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Fresh creation options for the browser that typed the code. Without
/// the cookie → 400 `PASSKEY_CEREMONY_INVALID`. An attempt that can no
/// longer register is refused here and the flow is kept: the page reloads
/// and hands the refusal to the app (`page`).
#[tracing::instrument(skip_all)]
pub async fn options(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<FlowOptions>, ApprovalError> {
    if !from_the_page(&state, &headers) {
        return Err(AuthError::Forbidden.into());
    }
    rp_of(&state)?;
    let stored = load_open(&state, flow_id)
        .await?
        .filter(|s| has_secret(&state, &headers, &s.flow))
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (user_id, session_id, attempt_id) =
        ids(&stored.flow).ok_or(AuthError::PasskeyCeremonyInvalid)?;
    // A refusal keeps the flow: the page reloads and hands it to the app.
    registration_open(&state, flow_id, &stored.flow).await?;
    let (ccr, st) = options_for(&state, user_id).await?;
    let record = CeremonyRecord {
        kind: CeremonyKind::PcSetup {
            user_id,
            session_id,
            attempt_id,
            flow_id,
        },
        state: CeremonyState::Register(st),
        created_at: chrono::Utc::now().timestamp(),
    };
    let options = serde_json::to_value(&ccr).map_err(|e| AuthError::Internal(e.to_string()))?;
    let ceremony_id = install_ceremony(&state, flow_id, &stored, &record).await?;
    Ok(Json(FlowOptions {
        ceremony: "create",
        ceremony_id,
        options,
    }))
}

/// Verify the registration and hand back a one-time code to the app's
/// loopback listener. Nothing is stored: the credential waits in the
/// pending result for `redeem`.
///
/// Without the cookie, or for a ceremony that is not the flow's current
/// one, the stale page (nothing is taken). A registration that does not
/// verify keeps the flow and shows the passkey step again: the code is
/// spent, so the retry must happen in this browser. An attempt that can no
/// longer register ends the flow with its error for the app.
#[tracing::instrument(skip_all)]
pub async fn finish(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
    headers: HeaderMap,
    Form(form): Form<FinishForm>,
) -> Response {
    let lang = Lang::from_headers(&headers);
    if !from_the_page(&state, &headers) {
        return refused_page(lang);
    }
    let stored = match load_open(&state, flow_id).await {
        Ok(Some(s)) if has_secret(&state, &headers, &s.flow) => s,
        Ok(_) => return stale_page(lang),
        Err(e) => return e.into_response(),
    };
    let rp = match rp_of(&state) {
        Ok(rp) => rp,
        Err(e) => return e.into_response(),
    };
    let submitted = match form.ceremony_id.as_deref().filter(|c| !c.is_empty()) {
        Some(raw) => Uuid::parse_str(raw).ok(),
        None => stored.flow.ceremony_id,
    };
    let Some(ceremony_id) = submitted.filter(|id| stored.flow.ceremony_id == Some(*id)) else {
        return stale_page(lang);
    };
    // GETDEL: of two racing finishes only one proceeds.
    let rec = match ceremony::take(&state.redis, ceremony_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return stale_page(lang),
        Err(e) => return e.into_response(),
    };
    let bound = match &rec.kind {
        CeremonyKind::PcSetup {
            user_id,
            session_id,
            attempt_id,
            flow_id: f,
        } => *f == flow_id && ids(&stored.flow) == Some((*user_id, *session_id, *attempt_id)),
        _ => false,
    };
    let CeremonyState::Register(st) = rec.state else {
        return fail_with(&state, lang, flow_id, &stored, "PASSKEY_CEREMONY_INVALID").await;
    };
    if !bound {
        return fail_with(&state, lang, flow_id, &stored, "PASSKEY_CEREMONY_INVALID").await;
    }
    let verified = match serde_json::from_str(&form.credential)
        .map_err(|_| AuthError::PasskeyInvalid)
        .and_then(|reg| rp.finish_registration(&reg, &st).map_err(failure))
    {
        Ok(v) => v,
        Err(e) => {
            let (_, code) = e.status_and_code();
            tracing::info!(code, "pc setup desktop registration did not verify");
            return retry_redirect(flow_id);
        }
    };
    // An ended or restarted attempt refuses here already (it is re-locked);
    // a transient failure keeps the flow for a retry.
    match registration_open(&state, flow_id, &stored.flow).await {
        Ok(()) => {}
        Err(e) if is_refusal(&e) => {
            return fail_with(&state, lang, flow_id, &stored, refusal_code(&e)).await;
        }
        Err(e) => {
            tracing::warn!(error = %e, "pc setup desktop registration check failed");
            return retry_redirect(flow_id);
        }
    }
    // Named after the browser that made it ("Firefox on Linux"), like a
    // passkey added in the browser itself; renamed in settings.
    let label = crate::auth::login_attempts::core::client_label(
        ClientClass::Web,
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
    );
    let code = new_handoff_code();
    let ctx = AuthCtx::from(&state);
    let mut flow = stored.flow.clone();
    flow.ceremony_id = None;
    flow.result = Some(PendingResult {
        code_hash: keyed_hash(
            ctx.hmac_key(),
            SecretPurpose::PasskeyHandoffCode,
            code.as_bytes(),
        ),
        outcome: FlowOutcome::PcSetup {
            verified: Box::new(verified),
            label,
        },
        created_at: chrono::Utc::now().timestamp(),
    });
    match cas(
        &state,
        flow_id,
        &stored.raw,
        Some(&flow),
        Some(PENDING_RESULT_TTL_SECS * 1000),
    )
    .await
    {
        Ok(true) => handoff_page(&flow.loopback, flow_id, HandoffField::Code(&code)),
        Ok(false) => stale_page(lang),
        Err(err) => {
            tracing::warn!(error = %err, "could not store a passkey result");
            unavailable_page(lang)
        }
    }
}

/// "Back to the app": before the right code anyone with the page may end
/// the flow (as in F1); after it only the browser that typed it.
#[tracing::instrument(skip_all)]
pub async fn cancel(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let lang = Lang::from_headers(&headers);
    if !from_the_page(&state, &headers) {
        return refused_page(lang);
    }
    let stored = match load_open(&state, flow_id).await {
        Ok(Some(s))
            if s.flow.browser_secret_hash.is_none() || has_secret(&state, &headers, &s.flow) =>
        {
            s
        }
        Ok(_) => return expired_page(lang),
        Err(e) => return e.into_response(),
    };
    cancel_flow(&state, lang, flow_id, &stored).await
}

/// `redeem` of a `pc_setup` flow → 200 `{purpose, user_id, passkey}`, no
/// tokens. The caller checked the code, the verifier and the starting
/// session's bearer. Locks account → requesting session → attempt →
/// enrolment ([`complete_enrolment`](enrolment::complete_enrolment)), holds
/// them through commit, then delivers the notice.
pub(super) async fn redeem(
    state: &AppState,
    flow_id: Uuid,
    user_id: Uuid,
    session_id: Uuid,
    attempt_id: Uuid,
    verified: &VerifiedRegistration,
    label: &str,
) -> AuthResult<Response> {
    let ctx = AuthCtx::from(state);
    let mut tx = state.db.begin().await?;
    let result = async {
        let locked = lock(&mut tx, (user_id, session_id, attempt_id)).await?;
        enrolment::complete_enrolment(
            &mut tx,
            &ctx,
            &locked,
            CodePath::DesktopFlow(flow_id),
            Credential { verified, label },
        )
        .await
    }
    .await;
    let row = settle(tx, result).await.map_err(to_auth)?;
    crate::security_events::notices::spawn_pending_delivery(state);
    crate::approvals::devices::notify_authenticators_changed(state, user_id).await;
    tracing::info!(passkey_id = %row.id, "pc setup passkey added through the desktop flow");
    Ok((
        StatusCode::OK,
        Json(DesktopPcSetupRedeemed {
            purpose: "pc_setup",
            user_id,
            passkey: (&row).into(),
        }),
    )
        .into_response())
}

/// `redeem` answer for `pc_setup`: the new passkey, no session, no grant.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DesktopPcSetupRedeemed {
    pub purpose: &'static str,
    pub user_id: Uuid,
    pub passkey: PasskeyView,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_check_refusals_end_the_flow() {
        for e in [
            ApprovalError::State,
            ApprovalError::Expired,
            ApprovalError::NotFound,
            ApprovalError::Auth(AuthError::PasskeyLimit),
        ] {
            assert!(is_refusal(&e), "{e:?}");
        }
        for e in [
            ApprovalError::Database(sqlx::Error::PoolTimedOut),
            ApprovalError::Unavailable,
            ApprovalError::Auth(AuthError::ServiceUnavailable),
            ApprovalError::Auth(AuthError::Database(sqlx::Error::PoolTimedOut)),
            ApprovalError::Auth(AuthError::Internal("x".into())),
        ] {
            assert!(!is_refusal(&e), "{e:?}");
        }
        assert_eq!(refusal_code(&ApprovalError::Expired), "APPROVAL_EXPIRED");
        assert_eq!(
            refusal_code(&ApprovalError::Auth(AuthError::PasskeyLimit)),
            "PASSKEY_LIMIT"
        );
    }
}
