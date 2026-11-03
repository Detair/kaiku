//! Desktop passkey flows in the system browser (F1 rev 4, C29 steps 1–7).
//!
//! The app binds its loopback listener first, then starts a flow here. The
//! ceremony runs on a server page in the system browser; `finish` and
//! `cancel` answer with the hand-off page, whose form POST carries the
//! one-time code (or an error) to the app's loopback. Nothing takes effect
//! before `redeem` with code + verifier (+ the starting session's bearer for
//! `step_up`), which re-checks the account, the passkey and the session.
//! The code is stored only as a keyed hash and is never rendered, logged,
//! or put in an address bar, a redirect or an API body.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form, Json};
use fred::interfaces::KeysInterface;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ceremony::{self, CeremonyKind, CeremonyRecord, CeremonyState};
use super::desktop_page::{
    ceremony_page, expired_page, handoff_page, stale_page, unavailable_page, HandoffField, Lang,
    LoopbackTarget, PageHeading,
};
use super::queries::{self, PasskeyState};
use super::{failure, login, rp_of, step_up};
use crate::api::AppState;
use crate::approvals::secrets::{b64u_decode, ct_eq, keyed_hash, SecretPurpose};
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::oidc::{client_challenge_matches, is_valid_client_challenge, new_handoff_code};
use crate::auth::session_issue::SessionClient;
use crate::auth::step_up::{StepUpAction, StepUpGrant, StepUpTarget};
use crate::auth::types::AuthResponse;
use crate::auth::{AuthError, AuthResult, AuthUser};

/// A flow lives at most 10 minutes.
pub const DESKTOP_FLOW_TTL_SECS: i64 = 600;
/// A verified result waits at most 60 s for its redemption.
pub const PENDING_RESULT_TTL_SECS: i64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowPurpose {
    /// `username`: the name typed in the app (C29 addition, 2026-11-03); the
    /// passkey must be that account's.
    SignIn {
        #[serde(default)]
        username: Option<String>,
    },
    StepUp {
        user_id: Uuid,
        session_id: Uuid,
        action: StepUpAction,
        /// `StepUpTarget::canonical()`.
        target: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowOutcome {
    SignIn { user_id: Uuid, passkey_id: Uuid },
    StepUp { passkey_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingResult {
    /// `keyed_hash(.., SecretPurpose::PasskeyHandoffCode, code)`.
    pub code_hash: [u8; 32],
    pub outcome: FlowOutcome,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopFlow {
    pub purpose: FlowPurpose,
    pub loopback: LoopbackTarget,
    pub client_challenge: String,
    pub ceremony_id: Option<Uuid>,
    pub result: Option<PendingResult>,
    pub created_at: i64,
}

fn key(flow_id: Uuid) -> String {
    format!("passkey:flow:{flow_id}")
}

fn unavailable(e: &fred::error::Error) -> AuthError {
    tracing::warn!(error = %e, "passkey flow store unavailable");
    AuthError::ServiceUnavailable
}

/// A flow as stored, with the exact stored JSON for compare-and-set.
struct Stored {
    raw: String,
    flow: DesktopFlow,
}

async fn load(state: &AppState, flow_id: Uuid) -> AuthResult<Option<Stored>> {
    let raw: Option<String> = state
        .redis
        .get(key(flow_id))
        .await
        .map_err(|e| unavailable(&e))?;
    Ok(raw.and_then(|raw| {
        serde_json::from_str(&raw)
            .ok()
            .map(|flow| Stored { raw, flow })
    }))
}

async fn take(state: &AppState, flow_id: Uuid) -> AuthResult<Option<DesktopFlow>> {
    let raw: Option<String> = state
        .redis
        .getdel(key(flow_id))
        .await
        .map_err(|e| unavailable(&e))?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

/// Compare-and-set on the flow key in one script: only if the stored JSON is
/// still `expected`. `ttl_ms = None` keeps the TTL (never extends or
/// shortens it); `Some(ms)` sets exactly `ms` (the pending result's 60 s of
/// C29 step 4, also near the flow's end); `new = None` deletes.
const FLOW_CAS: &str = r"
local cur = redis.call('GET', KEYS[1])
if (not cur) or cur ~= ARGV[1] then return 0 end
if ARGV[3] == 'del' then
  redis.call('DEL', KEYS[1])
elseif ARGV[3] == 'keep' then
  redis.call('SET', KEYS[1], ARGV[2], 'KEEPTTL')
else
  redis.call('SET', KEYS[1], ARGV[2], 'PX', tonumber(ARGV[3]))
end
return 1
";

async fn cas(
    state: &AppState,
    flow_id: Uuid,
    expected: &str,
    new: Option<&DesktopFlow>,
    ttl_ms: Option<i64>,
) -> AuthResult<bool> {
    use fred::interfaces::LuaInterface;
    let new_json = match new {
        Some(f) => serde_json::to_string(f).map_err(|e| AuthError::Internal(e.to_string()))?,
        None => String::new(),
    };
    let mode = match (new, ttl_ms) {
        (None, _) => "del".to_string(),
        (Some(_), None) => "keep".to_string(),
        (Some(_), Some(ms)) => ms.to_string(),
    };
    let done: i64 = state
        .redis
        .eval(
            FLOW_CAS,
            vec![key(flow_id)],
            vec![expected.to_string(), new_json, mode],
        )
        .await
        .map_err(|e| unavailable(&e))?;
    Ok(done == 1)
}

fn require_desktop(headers: &HeaderMap) -> AuthResult<()> {
    if ClientClass::from_headers(headers) == ClientClass::Desktop {
        Ok(())
    } else {
        Err(AuthError::ClientOutdated)
    }
}

/// `POST /auth/passkeys/desktop/flows`. `Debug` leaves out the callback path.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct DesktopFlowRequest {
    /// `sign_in` or `step_up`.
    pub purpose: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    pub loopback_port: u32,
    pub callback_path: String,
    pub client_challenge: String,
    /// `sign_in` only: the user name typed in the app, `null` for none.
    #[serde(default)]
    pub username: Option<String>,
}

impl std::fmt::Debug for DesktopFlowRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopFlowRequest")
            .field("purpose", &self.purpose)
            .field("action", &self.action)
            .field("loopback_port", &self.loopback_port)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DesktopFlowStarted {
    pub flow_id: Uuid,
    pub url: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Start a desktop flow (C29 step 2): `sign_in` is public, `step_up` needs
/// the bearer of a full session and validates like `step-up/start`.
#[utoipa::path(
    post,
    path = "/auth/passkeys/desktop/flows",
    tag = "passkeys",
    request_body = DesktopFlowRequest,
    responses(
        (status = 201, body = DesktopFlowStarted),
        (status = 400, description = "CLIENT_OUTDATED or VALIDATION_ERROR"),
        (status = 401, description = "step_up without a bearer"),
        (status = 403, description = "FULL_SESSION_REQUIRED, or NOT_ADMIN for an admin.* action"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn start(
    State(state): State<AppState>,
    auth: Option<Extension<AuthUser>>,
    headers: HeaderMap,
    Json(body): Json<DesktopFlowRequest>,
) -> AuthResult<(StatusCode, Json<DesktopFlowStarted>)> {
    require_desktop(&headers)?;
    rp_of(&state)?;
    let invalid = |m: &str| AuthError::Validation(m.to_string());
    let port = u16::try_from(body.loopback_port)
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| invalid("loopback_port must be 1..=65535"))?;
    if body.callback_path.len() != 43
        || b64u_decode(&body.callback_path).is_none_or(|b| b.len() != 32)
    {
        return Err(invalid("callback_path must be 32 bytes, base64url"));
    }
    if !is_valid_client_challenge(&body.client_challenge) {
        return Err(invalid("client_challenge must be S256 of the verifier"));
    }
    let purpose = match body.purpose.as_str() {
        "sign_in" => {
            if body.action.is_some() || body.target.as_deref().is_some_and(|t| !t.is_empty()) {
                return Err(invalid("sign_in takes no action or target"));
            }
            let username = body
                .username
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(str::to_owned);
            if username.as_ref().is_some_and(|u| u.chars().count() > 64) {
                return Err(invalid("username is too long"));
            }
            FlowPurpose::SignIn { username }
        }
        "step_up" => {
            if body.username.is_some() {
                return Err(invalid("step_up takes no username"));
            }
            let Some(Extension(auth)) = auth else {
                return Err(AuthError::MissingAuthHeader);
            };
            let raw_action = body.action.as_deref().unwrap_or("");
            if raw_action == StepUpAction::PasskeyAdd.as_str() {
                // DC12: the desktop app never registers a passkey (B.1).
                return Err(invalid("passkey.add is not available in the desktop app"));
            }
            let (action, target, _) =
                step_up::check_start(&state, &auth, raw_action, body.target.as_deref()).await?;
            // The ceremony page must be able to name the action (F7).
            if super::desktop_page::step_up_text(Lang::En, action, "", false).is_none() {
                return Err(invalid(
                    "this action cannot be confirmed in the desktop app",
                ));
            }
            FlowPurpose::StepUp {
                user_id: auth.id,
                session_id: auth.session_id.ok_or(AuthError::InvalidToken)?,
                action,
                target: target.canonical(),
            }
        }
        _ => return Err(invalid("purpose must be sign_in or step_up")),
    };
    let now = chrono::Utc::now();
    let flow = DesktopFlow {
        purpose,
        loopback: LoopbackTarget {
            port,
            callback_path: body.callback_path,
        },
        client_challenge: body.client_challenge,
        ceremony_id: None,
        result: None,
        created_at: now.timestamp(),
    };
    let flow_id = Uuid::new_v4();
    let json = serde_json::to_string(&flow).map_err(|e| AuthError::Internal(e.to_string()))?;
    let set: Option<String> = state
        .redis
        .set(
            key(flow_id),
            json,
            Some(fred::types::Expiration::EX(DESKTOP_FLOW_TTL_SECS)),
            Some(fred::types::SetOptions::NX),
            false,
        )
        .await
        .map_err(|e| unavailable(&e))?;
    if set.is_none() {
        return Err(AuthError::Internal("flow id collision".into()));
    }
    Ok((
        StatusCode::CREATED,
        Json(DesktopFlowStarted {
            flow_id,
            url: format!(
                "{}/auth/passkeys/desktop/{flow_id}",
                state.config.public_origin.origin
            ),
            expires_at: now + chrono::Duration::seconds(DESKTOP_FLOW_TTL_SECS),
        }),
    ))
}

async fn heading(state: &AppState, purpose: &FlowPurpose) -> AuthResult<PageHeading> {
    let FlowPurpose::StepUp {
        user_id,
        action,
        target,
        ..
    } = purpose
    else {
        return Ok(PageHeading::SignIn);
    };
    let id = Uuid::parse_str(target).ok();
    // Labels of the caller's own objects only (owner-scoped); admin actions
    // name the other account (its user name) or its authenticator.
    let own = |sql: &'static str| async move {
        match id {
            Some(id) => sqlx::query_scalar::<_, String>(sql)
                .bind(id)
                .bind(user_id)
                .fetch_optional(&state.db)
                .await
                .map(Option::unwrap_or_default),
            None => Ok(String::new()),
        }
    };
    // Another account's names only for a system admin (the start refuses
    // admin.* flows of non-admins; checked again here, at page time).
    let admin = action.is_admin()
        && crate::permissions::queries::is_system_admin(&state.db, *user_id).await?;
    let other = |sql: &'static str| async move {
        match id {
            Some(id) if admin || !action.is_admin() => sqlx::query_scalar::<_, String>(sql)
                .bind(id)
                .fetch_optional(&state.db)
                .await
                .map(Option::unwrap_or_default),
            _ => Ok(String::new()),
        }
    };
    let label = match action {
        StepUpAction::PasskeyRemove => {
            own("SELECT label FROM passkeys WHERE id = $1 AND user_id = $2 AND state = 'active'").await?
        }
        StepUpAction::KeyDeviceRemove => {
            own("SELECT label FROM approval_devices WHERE id = $1 AND user_id = $2 AND state = 'active'")
                .await?
        }
        StepUpAction::IdentityUnlink => {
            own("SELECT provider_slug FROM user_identities WHERE id = $1 AND user_id = $2").await?
        }
        StepUpAction::SessionsEnd => own(
            "SELECT COALESCE(user_agent, '') FROM sessions WHERE id = $1 AND user_id = $2",
        )
        .await
        .map(|ua| {
            if ua.is_empty() {
                ua
            } else {
                crate::auth::ua_parser::parse_device_name(&ua)
            }
        })?,
        StepUpAction::RecoveryContactSet
        | StepUpAction::AdminEnrollmentIssue
        | StepUpAction::AdminMayApprove
        | StepUpAction::AdminFactorSetState
        | StepUpAction::AdminRecoveryContactSet => {
            other("SELECT username FROM users WHERE id = $1 AND NOT is_system").await?
        }
        StepUpAction::AdminAuthenticatorRemove => other(
            "SELECT label FROM approval_devices WHERE id = $1
             UNION ALL SELECT label FROM passkeys WHERE id = $1 LIMIT 1",
        )
        .await?,
        _ => String::new(),
    };
    Ok(PageHeading::StepUp {
        action: *action,
        label,
        others: target == "others",
    })
}

/// The ceremony page (C29 step 3).
#[tracing::instrument(skip_all)]
pub async fn page(
    State(state): State<AppState>,
    Path(flow_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let lang = Lang::from_headers(&headers);
    let Ok(flow_id) = Uuid::parse_str(&flow_id) else {
        return expired_page(lang);
    };
    let flow = match load(&state, flow_id).await {
        Ok(Some(s)) if s.flow.result.is_none() => s.flow,
        Ok(_) => return expired_page(lang),
        Err(e) => return e.into_response(),
    };
    match heading(&state, &flow.purpose).await {
        Ok(h) => ceremony_page(lang, &h, flow_id),
        Err(e) => e.into_response(),
    }
}

#[derive(Debug, Serialize)]
pub struct FlowOptions {
    pub ceremony: &'static str,
    /// The page sends it back with `finish`, so a stale tab can never finish
    /// the current ceremony (additive to C29).
    pub ceremony_id: Uuid,
    pub options: serde_json::Value,
}

/// Fresh `get` options for the page (a reload gets new ones; the previous
/// ceremony can no longer finish). The flow update is a compare-and-set that
/// keeps the TTL; a flow that holds a result is never touched.
#[tracing::instrument(skip_all)]
pub async fn options(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
) -> AuthResult<Json<FlowOptions>> {
    let rp = rp_of(&state)?;
    let stored = load(&state, flow_id)
        .await?
        .filter(|s| s.flow.result.is_none())
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let mut flow = stored.flow.clone();
    let (rcr, record) = match &flow.purpose {
        FlowPurpose::SignIn { .. } => {
            let (rcr, st) = rp.start_sign_in().map_err(failure)?;
            (
                serde_json::to_value(&rcr),
                CeremonyRecord {
                    kind: CeremonyKind::SignIn {
                        flow_id: Some(flow_id),
                    },
                    state: CeremonyState::SignIn(st),
                    created_at: chrono::Utc::now().timestamp(),
                },
            )
        }
        FlowPurpose::StepUp {
            user_id,
            session_id,
            action,
            target,
        } => {
            let creds: Vec<_> = queries::list_active(&state.db, *user_id)
                .await?
                .into_iter()
                .take(super::MAX_ACTIVE_PASSKEYS)
                .map(|r| r.credential.0)
                .collect();
            if creds.is_empty() {
                // Nothing can ever finish: end the flow (no reload loop).
                cas(&state, flow_id, &stored.raw, None, None).await?;
                return Err(AuthError::PasskeyCeremonyInvalid);
            }
            let (rcr, st) = rp.start_step_up(&creds).map_err(failure)?;
            (
                serde_json::to_value(&rcr),
                CeremonyRecord {
                    kind: CeremonyKind::StepUp {
                        user_id: *user_id,
                        session_id: *session_id,
                        action: *action,
                        target: target.clone(),
                        flow_id: Some(flow_id),
                    },
                    state: CeremonyState::StepUp(st),
                    created_at: chrono::Utc::now().timestamp(),
                },
            )
        }
    };
    let options = rcr.map_err(|e| AuthError::Internal(e.to_string()))?;
    let ceremony_id = ceremony::new_ceremony_id();
    ceremony::put_new(&state.redis, ceremony_id, &record).await?;
    let old = flow.ceremony_id.replace(ceremony_id);
    if !cas(&state, flow_id, &stored.raw, Some(&flow), None).await? {
        // The flow changed meanwhile (a finish, a cancel, another reload).
        let _ = ceremony::take(&state.redis, ceremony_id).await;
        return Err(AuthError::PasskeyCeremonyInvalid);
    }
    if let Some(old) = old {
        let _ = ceremony::take(&state.redis, old).await;
    }
    Ok(Json(FlowOptions {
        ceremony: "get",
        ceremony_id,
        options,
    }))
}

#[derive(Debug, Deserialize)]
pub struct FinishForm {
    #[serde(default)]
    pub credential: String,
    /// The ceremony the page ran (from `options`).
    #[serde(default)]
    pub ceremony_id: Option<String>,
}

/// End a flow after a failed finish and hand back the error code.
///
/// Only while the flow is still the one this finish read: when another
/// page changed it meanwhile (a newer ceremony, a result), this finish lost
/// and gets the stale page. A terminal error for the app's listener would
/// end the app's wait and so break the winner's hand-off.
async fn fail(
    state: &AppState,
    lang: Lang,
    flow_id: Uuid,
    stored: &Stored,
    e: &AuthError,
) -> Response {
    match cas(state, flow_id, &stored.raw, None, None).await {
        Ok(true) => {}
        Ok(false) => return stale_page(lang),
        // Not known whether the flow is still this one: no callback.
        Err(err) => {
            tracing::warn!(error = %err, "could not end a passkey flow");
            return unavailable_page(lang);
        }
    }
    let (_, code) = e.status_and_code();
    tracing::info!(code, "desktop passkey finish refused");
    handoff_page(&stored.flow.loopback, flow_id, HandoffField::Error(code))
}

/// Verify the assertion and hand back a one-time code (or the error) to
/// the loopback listener (C29 step 4).
#[tracing::instrument(skip_all)]
pub async fn finish(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
    headers: HeaderMap,
    Form(form): Form<FinishForm>,
) -> Response {
    let lang = Lang::from_headers(&headers);
    let stored = match load(&state, flow_id).await {
        Ok(Some(s)) => s,
        Ok(None) => return expired_page(lang),
        Err(e) => return e.into_response(),
    };
    if stored.flow.result.is_some() {
        return stale_page(lang);
    }
    // The ceremony the page ran; an old page (no field) means the current
    // one. Anything but the flow's current ceremony (a stale tab's, another
    // flow's, a web one, garbage) gets the stale page before any ceremony is
    // taken: it consumes nothing and leaves the flow as it is.
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
        CeremonyKind::SignIn { flow_id: f } | CeremonyKind::StepUp { flow_id: f, .. } => {
            *f == Some(flow_id)
        }
        CeremonyKind::Register { .. } => false,
    };
    if !bound {
        // The flow named a ceremony that is not its own: it cannot go on.
        return fail(
            &state,
            lang,
            flow_id,
            &stored,
            &AuthError::PasskeyCeremonyInvalid,
        )
        .await;
    }
    let credential = serde_json::from_str::<serde_json::Value>(&form.credential)
        .unwrap_or(serde_json::Value::Null);
    let verified = match (&stored.flow.purpose, rec.state) {
        (FlowPurpose::SignIn { username }, CeremonyState::SignIn(st)) => {
            match login::verify_sign_in(&state, st, credential).await {
                Ok((row, _)) => {
                    login::check_username_hint(&state, row.user_id, username.as_deref())
                        .await
                        .map(|()| FlowOutcome::SignIn {
                            user_id: row.user_id,
                            passkey_id: row.id,
                        })
                }
                Err(e) => Err(e),
            }
        }
        (FlowPurpose::StepUp { user_id, .. }, CeremonyState::StepUp(st)) => {
            step_up::verify(&state, *user_id, &st, credential)
                .await
                .map(|(row, _)| FlowOutcome::StepUp { passkey_id: row.id })
        }
        _ => Err(AuthError::PasskeyCeremonyInvalid),
    };
    let outcome = match verified {
        Ok(o) => o,
        Err(e) => return fail(&state, lang, flow_id, &stored, &e).await,
    };
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
        outcome,
        created_at: chrono::Utc::now().timestamp(),
    });
    // The pending result lives 60 s (C29 step 4), also near the flow's end.
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
        // Another page changed the flow meanwhile: this finish lost; the
        // winner's hand-off must not be cut short by an error here.
        Ok(false) => stale_page(lang),
        // Not known whether this finish won: no callback (the app waits
        // until the flow expires or the user cancels).
        Err(err) => {
            tracing::warn!(error = %err, "could not store a passkey result");
            unavailable_page(lang)
        }
    }
}

/// "Back to the app": end the flow, hand back `PASSKEY_CANCELLED`.
#[tracing::instrument(skip_all)]
pub async fn cancel(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
    headers: HeaderMap,
) -> Response {
    let lang = Lang::from_headers(&headers);
    let stored = match load(&state, flow_id).await {
        Ok(Some(s)) if s.flow.result.is_none() => s,
        Ok(_) => return expired_page(lang),
        Err(e) => return e.into_response(),
    };
    match cas(&state, flow_id, &stored.raw, None, None).await {
        Ok(true) => {
            if let Some(c) = stored.flow.ceremony_id {
                let _ = ceremony::take(&state.redis, c).await;
            }
            handoff_page(
                &stored.flow.loopback,
                flow_id,
                HandoffField::Error("PASSKEY_CANCELLED"),
            )
        }
        Ok(false) => expired_page(lang),
        Err(e) => e.into_response(),
    }
}

/// `POST /auth/passkeys/desktop/redeem`.
#[derive(Deserialize, utoipa::ToSchema)]
pub struct DesktopRedeemRequest {
    pub flow_id: Uuid,
    pub code: String,
    pub verifier: String,
}

impl std::fmt::Debug for DesktopRedeemRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopRedeemRequest")
            .field("flow_id", &self.flow_id)
            .finish_non_exhaustive()
    }
}

/// `Debug` leaves out the tokens.
#[derive(Serialize, utoipa::ToSchema)]
pub struct DesktopSignInRedeemed {
    pub purpose: &'static str,
    pub user_id: Uuid,
    #[serde(flatten)]
    pub auth: AuthResponse,
}

impl std::fmt::Debug for DesktopSignInRedeemed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopSignInRedeemed")
            .field("purpose", &self.purpose)
            .field("user_id", &self.user_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DesktopStepUpRedeemed {
    pub purpose: &'static str,
    pub user_id: Uuid,
    pub grant: StepUpGrant,
}

/// Redeem the hand-off (C29 step 6): only now is the session or the grant
/// created, after re-checking the account, the passkey and (step-up) the
/// session. Every attempt spends the flow; any failure → 400
/// `PASSKEY_HANDOFF_INVALID`.
#[utoipa::path(
    post,
    path = "/auth/passkeys/desktop/redeem",
    tag = "passkeys",
    request_body = DesktopRedeemRequest,
    responses(
        (status = 200, description = "sign_in: AuthResponse + purpose + user_id (refresh in the body); step_up: {purpose, user_id, grant}"),
        (status = 400, description = "PASSKEY_HANDOFF_INVALID or CLIENT_OUTDATED"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn redeem(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    auth: Option<Extension<AuthUser>>,
    headers: HeaderMap,
    Json(body): Json<DesktopRedeemRequest>,
) -> AuthResult<Response> {
    require_desktop(&headers)?;
    rp_of(&state)?;
    let flow = take(&state, body.flow_id).await?;
    let auth = auth.map(|Extension(a)| a);
    let result = redeem_inner(&state, addr, &headers, auth.as_ref(), flow, &body).await;
    if let Err(e) = &result {
        let (_, code) = e.status_and_code();
        tracing::info!(code, "desktop passkey redemption refused");
    }
    result.map_err(|e| match e {
        AuthError::ServiceUnavailable | AuthError::Database(_) => e,
        _ => AuthError::PasskeyHandoffInvalid,
    })
}

async fn redeem_inner(
    state: &AppState,
    addr: SocketAddr,
    headers: &HeaderMap,
    auth: Option<&AuthUser>,
    flow: Option<DesktopFlow>,
    body: &DesktopRedeemRequest,
) -> AuthResult<Response> {
    let ctx = AuthCtx::from(state);
    let flow = flow.ok_or(AuthError::PasskeyHandoffInvalid)?;
    let result = flow.result.ok_or(AuthError::PasskeyHandoffInvalid)?;
    let age = chrono::Utc::now().timestamp() - result.created_at;
    let hash = keyed_hash(
        ctx.hmac_key(),
        SecretPurpose::PasskeyHandoffCode,
        body.code.as_bytes(),
    );
    if !(0..=PENDING_RESULT_TTL_SECS).contains(&age)
        || !ct_eq(&hash, &result.code_hash)
        || !client_challenge_matches(&body.verifier, &flow.client_challenge)
    {
        return Err(AuthError::PasskeyHandoffInvalid);
    }
    match (flow.purpose, result.outcome) {
        (
            FlowPurpose::SignIn { .. },
            FlowOutcome::SignIn {
                user_id,
                passkey_id,
            },
        ) => {
            let row = queries::find_own(&state.db, user_id, passkey_id)
                .await?
                .filter(|r| r.state == PasskeyState::Active)
                .ok_or(AuthError::PasskeyHandoffInvalid)?;
            let client = SessionClient::from_request(state, headers, addr);
            let (issued, must_change) =
                login::issue(state, &row, &client, ClientClass::Desktop).await?;
            Ok(Json(DesktopSignInRedeemed {
                purpose: "sign_in",
                user_id,
                auth: issued.into_response(must_change, true),
            })
            .into_response())
        }
        (
            FlowPurpose::StepUp {
                user_id,
                session_id,
                action,
                target,
            },
            FlowOutcome::StepUp { passkey_id },
        ) => {
            let auth = auth.ok_or(AuthError::PasskeyHandoffInvalid)?;
            if auth.id != user_id || auth.session_id != Some(session_id) {
                return Err(AuthError::PasskeyHandoffInvalid);
            }
            crate::auth::guards::require_full(auth)?;
            let target = StepUpTarget::parse(action, &target)?;
            // `grant_tx` locks the session row and re-checks it live and
            // full, and re-checks the passkey active and the caller's own.
            let grant = step_up::grant(state, auth, action, &target, passkey_id).await?;
            Ok(Json(DesktopStepUpRedeemed {
                purpose: "step_up",
                user_id,
                grant,
            })
            .into_response())
        }
        _ => Err(AuthError::PasskeyHandoffInvalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "c2VjcmV0LWNhbGxiYWNrLXBhdGgtMzItYnl0ZXMtYWJj";

    #[test]
    fn debug_output_never_carries_the_callback_path_or_tokens() {
        let target = LoopbackTarget {
            port: 4242,
            callback_path: PATH.into(),
        };
        let flow = DesktopFlow {
            purpose: FlowPurpose::SignIn { username: None },
            loopback: target.clone(),
            client_challenge: "c".repeat(43),
            ceremony_id: None,
            result: None,
            created_at: 0,
        };
        let request = DesktopFlowRequest {
            purpose: "sign_in".into(),
            action: None,
            target: None,
            loopback_port: 4242,
            callback_path: PATH.into(),
            client_challenge: "c".repeat(43),
            username: None,
        };
        let redeemed = DesktopSignInRedeemed {
            purpose: "sign_in",
            user_id: Uuid::nil(),
            auth: AuthResponse {
                access_token: "secret-access".into(),
                refresh_token: Some("secret-refresh".into()),
                expires_in: 900,
                token_type: "Bearer".into(),
                setup_required: false,
                password_change_required: false,
                session_id: Uuid::nil(),
                assurance: crate::auth::assurance::Assurance::Full,
                absolute_expires_at: chrono::Utc::now(),
            },
        };
        for out in [
            format!("{target:?}"),
            format!("{flow:?}"),
            format!("{request:?}"),
            format!("{redeemed:?}"),
        ] {
            assert!(!out.contains(PATH), "{out}");
            assert!(!out.contains("secret-"), "{out}");
        }
        assert!(format!("{target:?}").contains("4242"));
    }
}
