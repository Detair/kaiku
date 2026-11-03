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
    ceremony_page, expired_page, handoff_page, stale_page, HandoffField, Lang, LoopbackTarget,
    PageHeading,
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
    SignIn,
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

async fn load(state: &AppState, flow_id: Uuid) -> AuthResult<Option<DesktopFlow>> {
    let raw: Option<String> = state
        .redis
        .get(key(flow_id))
        .await
        .map_err(|e| unavailable(&e))?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

async fn take(state: &AppState, flow_id: Uuid) -> AuthResult<Option<DesktopFlow>> {
    let raw: Option<String> = state
        .redis
        .getdel(key(flow_id))
        .await
        .map_err(|e| unavailable(&e))?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

/// Update a live flow in place (`SET XX KEEPTTL`); false if it is gone.
async fn update(state: &AppState, flow_id: Uuid, flow: &DesktopFlow) -> AuthResult<bool> {
    let json = serde_json::to_string(flow).map_err(|e| AuthError::Internal(e.to_string()))?;
    let set: Option<String> = state
        .redis
        .set(
            key(flow_id),
            json,
            Some(fred::types::Expiration::KEEPTTL),
            Some(fred::types::SetOptions::XX),
            false,
        )
        .await
        .map_err(|e| unavailable(&e))?;
    Ok(set.is_some())
}

async fn delete(state: &AppState, flow_id: Uuid) {
    let r: Result<i64, _> = state.redis.del(key(flow_id)).await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "could not delete a passkey flow");
    }
}

fn require_desktop(headers: &HeaderMap) -> AuthResult<()> {
    if ClientClass::from_headers(headers) == ClientClass::Desktop {
        Ok(())
    } else {
        Err(AuthError::ClientOutdated)
    }
}

/// `POST /auth/passkeys/desktop/flows`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
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
        (status = 403, description = "FULL_SESSION_REQUIRED"),
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
            FlowPurpose::SignIn
        }
        "step_up" => {
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
    let label = |sql: &'static str| async move {
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
    Ok(match action {
        StepUpAction::PasskeyRemove => PageHeading::RemovePasskey(
            label("SELECT label FROM passkeys WHERE id = $1 AND user_id = $2").await?,
        ),
        StepUpAction::KeyDeviceRemove => PageHeading::RemoveDevice(
            label("SELECT label FROM approval_devices WHERE id = $1 AND user_id = $2").await?,
        ),
        StepUpAction::AdminElevate => PageHeading::Elevate,
        _ => PageHeading::Other,
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
        Ok(Some(f)) if f.result.is_none() => f,
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
    pub options: serde_json::Value,
}

/// Fresh `get` options for the page (a reload gets new ones; the previous
/// ceremony can no longer finish).
#[tracing::instrument(skip_all)]
pub async fn options(
    State(state): State<AppState>,
    Path(flow_id): Path<Uuid>,
) -> AuthResult<Json<FlowOptions>> {
    let rp = rp_of(&state)?;
    let mut flow = load(&state, flow_id)
        .await?
        .filter(|f| f.result.is_none())
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    let (rcr, record) = match &flow.purpose {
        FlowPurpose::SignIn => {
            let (rcr, st) = rp.start_sign_in().map_err(failure)?;
            (
                serde_json::to_value(&rcr),
                CeremonyRecord {
                    kind: CeremonyKind::SignIn,
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
    if let Some(old) = flow.ceremony_id.replace(ceremony_id) {
        let _ = ceremony::take(&state.redis, old).await;
    }
    if !update(&state, flow_id, &flow).await? {
        return Err(AuthError::PasskeyCeremonyInvalid);
    }
    Ok(Json(FlowOptions {
        ceremony: "get",
        options,
    }))
}

#[derive(Debug, Deserialize)]
pub struct FinishForm {
    #[serde(default)]
    pub credential: String,
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
    let mut flow = match load(&state, flow_id).await {
        Ok(Some(f)) => f,
        Ok(None) => return expired_page(lang),
        Err(e) => return e.into_response(),
    };
    if flow.result.is_some() {
        return stale_page(lang);
    }
    // GETDEL the current ceremony: of two racing finishes only one proceeds.
    let Some(ceremony_id) = flow.ceremony_id else {
        return stale_page(lang);
    };
    let rec = match ceremony::take(&state.redis, ceremony_id).await {
        Ok(Some(r)) => r,
        Ok(None) => return stale_page(lang),
        Err(e) => return e.into_response(),
    };
    let credential = serde_json::from_str::<serde_json::Value>(&form.credential)
        .unwrap_or(serde_json::Value::Null);
    let verified = match (&flow.purpose, rec.state) {
        (FlowPurpose::SignIn, CeremonyState::SignIn(st)) => {
            login::verify_sign_in(&state, st, credential)
                .await
                .map(|(row, _)| FlowOutcome::SignIn {
                    user_id: row.user_id,
                    passkey_id: row.id,
                })
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
        Err(e) => {
            // A failed finish ends the flow; the app starts a new one.
            delete(&state, flow_id).await;
            let (_, code) = e.status_and_code();
            tracing::info!(code, "desktop passkey finish refused");
            return handoff_page(&flow.loopback, flow_id, HandoffField::Error(code));
        }
    };
    let code = new_handoff_code();
    let ctx = AuthCtx::from(&state);
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
    match update(&state, flow_id, &flow).await {
        Ok(true) => handoff_page(&flow.loopback, flow_id, HandoffField::Code(&code)),
        Ok(false) => expired_page(lang),
        Err(_) => handoff_page(
            &flow.loopback,
            flow_id,
            HandoffField::Error("SERVICE_UNAVAILABLE"),
        ),
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
    match take(&state, flow_id).await {
        Ok(Some(flow)) => {
            if let Some(c) = flow.ceremony_id {
                let _ = ceremony::take(&state.redis, c).await;
            }
            handoff_page(
                &flow.loopback,
                flow_id,
                HandoffField::Error("PASSKEY_CANCELLED"),
            )
        }
        Ok(None) => expired_page(lang),
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

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DesktopSignInRedeemed {
    pub purpose: &'static str,
    pub user_id: Uuid,
    #[serde(flatten)]
    pub auth: AuthResponse,
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
            FlowPurpose::SignIn,
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
