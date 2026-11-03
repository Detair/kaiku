//! Sign in with a passkey (F1, C29 `login/*`): a new full session.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use uuid::Uuid;
use webauthn_rs::prelude::{AuthenticationResult, DiscoverableAuthentication, PublicKeyCredential};

use super::ceremony::{self, CeremonyKind, CeremonyRecord, CeremonyState};
use super::queries::{self, PasskeyRow, PasskeyState};
use super::types::CeremonyStart;
use super::{audit, failure, rp_of};
use crate::api::AppState;
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::session_issue::{
    issue_session_tx, IssuedSession, NewSession, SessionClient, SignInMethod,
};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::types::AuthResponse;
use crate::auth::{AuthError, AuthResult};
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// `POST /auth/passkeys/login/finish`.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PasskeyLoginFinishRequest {
    pub ceremony_id: Uuid,
    /// `PublicKeyCredential.toJSON()`.
    #[schema(value_type = Object)]
    pub credential: serde_json::Value,
    /// The user name typed on the login page, if any: the passkey must be
    /// that account's (case-insensitive), so a shared device never signs in
    /// someone else.
    #[serde(default)]
    pub username: Option<String>,
}

/// Start a discoverable sign-in (`allowCredentials: []`).
pub(super) async fn new_sign_in_ceremony(state: &AppState) -> AuthResult<CeremonyStart> {
    let rp = rp_of(state)?;
    let (rcr, st) = rp.start_sign_in().map_err(failure)?;
    let ceremony_id = ceremony::new_ceremony_id();
    ceremony::put_new(
        &state.redis,
        ceremony_id,
        &CeremonyRecord {
            kind: CeremonyKind::SignIn { flow_id: None },
            state: CeremonyState::SignIn(st),
            created_at: chrono::Utc::now().timestamp(),
        },
    )
    .await?;
    Ok(CeremonyStart {
        ceremony_id,
        options: serde_json::to_value(&rcr).map_err(|e| AuthError::Internal(e.to_string()))?,
    })
}

/// Verify a discoverable assertion (web, Android and the desktop page):
/// the user handle names an account; a missing, revoked or foreign
/// credential → `PASSKEY_UNKNOWN`; the signature is checked for `pending`
/// rows too, so only the holder learns `ready_at` (409 `PASSKEY_PENDING`).
/// Records the use of an active passkey.
pub(super) async fn verify_sign_in(
    state: &AppState,
    st: DiscoverableAuthentication,
    credential: serde_json::Value,
) -> AuthResult<(PasskeyRow, AuthenticationResult)> {
    let rp = rp_of(state)?;
    let cred: PublicKeyCredential =
        serde_json::from_value(credential).map_err(|_| AuthError::PasskeyInvalid)?;
    let (handle, credential_id) = rp.identify(&cred).map_err(failure)?;
    let user_id = queries::user_id_for_handle(&state.db, handle).await?;
    let row = queries::find_by_credential_id(&state.db, &credential_id)
        .await?
        .filter(|r| Some(r.user_id) == user_id && r.state != PasskeyState::Revoked)
        .ok_or(AuthError::PasskeyUnknown)?;
    let result = match rp.finish_sign_in(&cred, st, &row.credential.0, row.backup_eligible) {
        Ok(r) => r,
        Err(f) => {
            return Err(failure(f));
        }
    };
    if row.state == PasskeyState::Pending {
        return Err(AuthError::PasskeyPending {
            ready_at: row.ready_at.unwrap_or_else(chrono::Utc::now),
        });
    }
    if super::rp::counter_regressed(
        super::rp::stored_counter(&row.credential.0),
        result.counter(),
    ) {
        // A signal, never a refusal (B.1).
        audit::counter_regression(state, &row, result.counter()).await;
    }
    queries::record_use(&state.db, row.id, result.counter(), result.backup_state()).await?;
    Ok((row, result))
}

/// The typed user name, if any, must be the passkey's account
/// (case-insensitive): a shared device never signs in someone else.
pub(super) async fn check_username_hint(
    state: &AppState,
    user_id: Uuid,
    hint: Option<&str>,
) -> AuthResult<()> {
    let Some(hint) = hint.map(str::trim).filter(|h| !h.is_empty()) else {
        return Ok(());
    };
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;
    if username.eq_ignore_ascii_case(hint) {
        Ok(())
    } else {
        Err(AuthError::PasskeyUserMismatch)
    }
}

/// Create the new full session of a verified passkey and record the C28
/// `sign_in` event (DC10), in one transaction. Returns the session and the
/// account's `must_change_password`.
pub(super) async fn issue(
    state: &AppState,
    row: &PasskeyRow,
    client: &SessionClient,
    client_class: ClientClass,
) -> AuthResult<(IssuedSession, bool)> {
    let ctx = AuthCtx::from(state);
    let authenticator = AuthenticatorRef::passkey(row.id);
    let mut tx = state.db.begin().await?;
    let mut issued = issue_session_tx(
        &mut tx,
        &ctx,
        &NewSession {
            user_id: row.user_id,
            method: SignInMethod::Passkey,
            client,
            client_class,
            authenticator: Some(authenticator),
        },
    )
    .await?;
    // The passkey must still be active under the account lock that
    // `issue_session_tx` took (a removal commits before or after).
    if !queries::is_active(&mut tx, row.id).await? {
        return Err(AuthError::PasskeyUnknown);
    }
    let mut ev = NewSecurityEvent::new(
        row.user_id,
        EventType::SignIn,
        Actor::User(row.user_id),
        serde_json::json!({
            "auth_methods": SignInMethod::Passkey.auth_methods(),
            "assurance": issued.assurance.as_str(),
        }),
    );
    let label = client
        .user_agent
        .as_deref()
        .map(crate::auth::ua_parser::parse_device_name)
        .unwrap_or_default();
    ev.client = Some((client_class, label));
    ev.ip = client.ip;
    ev.session_id = Some(issued.session_id);
    ev.authenticator = Some(authenticator);
    issued.event_ids.push(record(&mut tx, ev).await?);
    let must_change: bool =
        sqlx::query_scalar("SELECT must_change_password FROM users WHERE id = $1")
            .bind(row.user_id)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    crate::security_events::spawn_delivery(state, issued.event_ids.clone());
    tracing::info!(user_id = %row.user_id, session_id = %issued.session_id, "passkey sign-in");
    Ok((issued, must_change))
}

/// Start a passkey sign-in.
#[utoipa::path(
    post,
    path = "/auth/passkeys/login/start",
    tag = "passkeys",
    responses(
        (status = 200, body = CeremonyStart),
        (status = 429, description = "TOO_MANY_ATTEMPTS"),
        (status = 503, description = "PASSKEYS_UNAVAILABLE or SERVICE_UNAVAILABLE"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn start(State(state): State<AppState>) -> AuthResult<Json<CeremonyStart>> {
    Ok(Json(new_sign_in_ceremony(&state).await?))
}

/// Finish a passkey sign-in → a new full session (`auth_methods:
/// ["passkey"]`). Browsers get the refresh cookie, native clients the
/// refresh token in the body.
#[utoipa::path(
    post,
    path = "/auth/passkeys/login/finish",
    tag = "passkeys",
    request_body = PasskeyLoginFinishRequest,
    responses(
        (status = 200, body = AuthResponse),
        (status = 400, description = "PASSKEY_CEREMONY_INVALID, PASSKEY_INVALID, PASSKEY_UNKNOWN or PASSKEY_USER_MISMATCH"),
        (status = 403, description = "ACCOUNT_BANNED"),
        (status = 409, description = "PASSKEY_PENDING {ready_at}"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn finish(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<PasskeyLoginFinishRequest>,
) -> AuthResult<(CookieJar, Json<AuthResponse>)> {
    let outcome = finish_inner(&state, addr, &headers, body).await;
    crate::observability::metrics::record_auth_login_attempt(outcome.is_ok());
    let (issued, must_change, native) = outcome?;
    let jar = if native {
        jar
    } else {
        jar.add(issued.refresh_cookie(&state.config))
    };
    Ok((jar, Json(issued.into_response(must_change, native))))
}

async fn finish_inner(
    state: &AppState,
    addr: SocketAddr,
    headers: &HeaderMap,
    body: PasskeyLoginFinishRequest,
) -> AuthResult<(IssuedSession, bool, bool)> {
    rp_of(state)?;
    let rec = ceremony::take(&state.redis, body.ceremony_id)
        .await?
        .ok_or(AuthError::PasskeyCeremonyInvalid)?;
    // A desktop flow's ceremony never finishes here (it would mint a web session).
    let (CeremonyKind::SignIn { flow_id: None }, CeremonyState::SignIn(st)) = (rec.kind, rec.state)
    else {
        return Err(AuthError::PasskeyCeremonyInvalid);
    };
    let (row, _) = verify_sign_in(state, st, body.credential).await?;
    check_username_hint(state, row.user_id, body.username.as_deref()).await?;
    let client = SessionClient::from_request(state, headers, addr);
    let client_class = ClientClass::from_headers(headers);
    let (issued, must_change) = issue(state, &row, &client, client_class).await?;
    Ok((issued, must_change, client_class.is_native()))
}
