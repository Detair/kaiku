//! Device-key sign-in (spec B.4 "Device-key sign-in", SM4; C26 `device_login`).
//!
//! An `active` key device whose session lapsed (inactivity, the absolute cap,
//! a plain sign-out) asks for a `device_login` challenge naming itself and
//! signs it with user verification. Under the account lock the server checks
//! the device is still `active` and the account not banned, consumes the
//! challenge and mints a `full` session; the device's older sessions end
//! (`superseded`, DC12). Public routes with their own `AuthFlow` limit, never
//! IP-blocked (B.6: a blocked IP never locks a key phone out).

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use chrono::Duration;
use uuid::Uuid;

use super::context::CanonicalContext;
use super::core::{
    issue, lock_for_answer, set_state, verify, ChallengeState, NewChallenge, Requester,
};
use super::devices::{DeviceState, KeyDeviceRow};
use super::error::ApprovalError;
use super::signing::{Decision, Kind};
use super::types::{ApprovalChallenge, DeviceLoginChallengeRequest, DeviceLoginRequest};
use crate::api::AppState;
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::revocation::{end_sessions_tx, EndReason};
use crate::auth::session_issue::{
    issue_session_tx, IssuedSession, NewSession, SessionClient, SignInMethod,
};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::types::AuthResponse;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// `device_login` challenges live 2 minutes (C26).
const DEVICE_LOGIN_TTL_SECS: i64 = 120;

/// A completed device-key sign-in.
#[derive(Debug)]
pub struct DeviceSignIn {
    pub session: IssuedSession,
    /// The account still has to replace an admin-set password (as `/auth/login`).
    pub password_change_required: bool,
}

/// The device's state as a sign-in refusal (`None` = `active`).
fn refusal(device: &KeyDeviceRow) -> Option<ApprovalError> {
    if device.state == DeviceState::Revoked.as_str() {
        Some(ApprovalError::KeyDeviceRevoked)
    } else if device.state == DeviceState::Pending.as_str() {
        // `ready_at` comes from the device's account request (L1b).
        Some(ApprovalError::KeyDevicePending { ready_at: None })
    } else {
        None
    }
}

/// A pending device's refusal carries its account request's `ready_at`.
async fn with_ready_at(
    conn: &mut sqlx::PgConnection,
    device: &KeyDeviceRow,
    e: ApprovalError,
) -> sqlx::Result<ApprovalError> {
    if let ApprovalError::KeyDevicePending { .. } = e {
        let ready_at = crate::auth::account_requests::core::pending_ready_at(
            conn,
            AuthenticatorRef::key_device(device.id),
        )
        .await?;
        return Ok(ApprovalError::KeyDevicePending { ready_at });
    }
    Ok(e)
}

/// Issue a `device_login` challenge for `(user_id, device_id)`.
///
/// Takes no lock: [`sign_in`] re-checks everything under the account lock.
/// Unknown device or the system account → 404 `KEY_DEVICE_UNKNOWN`; only
/// revoked rows → 403 `KEY_DEVICE_REVOKED`; banned → 403 `ACCOUNT_BANNED`;
/// `pending` → 409 `KEY_DEVICE_PENDING {ready_at}`.
#[tracing::instrument(skip(ctx))]
pub async fn challenge(
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    device_id: Uuid,
) -> Result<ApprovalChallenge, ApprovalError> {
    if crate::system_account::is_system_user(user_id) {
        return Err(ApprovalError::KeyDeviceUnknown);
    }
    let mut tx = ctx.db.begin().await?;
    // The live row (at most one, `approval_devices_live`), else the newest
    // revoked one.
    let device: KeyDeviceRow = sqlx::query_as(
        "SELECT d.* FROM approval_devices d JOIN users u ON u.id = d.user_id
          WHERE d.user_id = $1 AND d.device_id = $2 AND NOT u.is_system
          ORDER BY (d.state <> 'revoked') DESC, d.created_at DESC
          LIMIT 1",
    )
    .bind(user_id)
    .bind(device_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApprovalError::KeyDeviceUnknown)?;
    if device.state == DeviceState::Revoked.as_str() {
        return Err(ApprovalError::KeyDeviceRevoked);
    }
    let banned: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM global_bans
                         WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW()))",
    )
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await?;
    if banned {
        return Err(ApprovalError::AccountBanned);
    }
    if let Some(e) = refusal(&device) {
        return Err(with_ready_at(&mut tx, &device, e).await?);
    }
    let challenge = issue(
        &mut tx,
        ctx,
        NewChallenge {
            context: CanonicalContext::DeviceLogin {
                client_class: ClientClass::Android,
            },
            approver_id: user_id,
            subject_id: user_id,
            device_id,
            requester: Requester::Device(device.id),
            display: serde_json::json!({}),
            transfer_hash: None,
            ttl: Duration::seconds(DEVICE_LOGIN_TTL_SECS),
        },
    )
    .await?;
    tx.commit().await?;
    Ok(challenge)
}

/// Complete a device-key sign-in in one transaction.
///
/// Lock order (B.7): the account, then the device row, then the challenge.
/// A revocation of the device (which takes the account lock first) either
/// commits before — this sees `revoked` — or after, and then ends the
/// session minted here.
#[tracing::instrument(skip(ctx, signature, client))]
pub async fn sign_in(
    ctx: &AuthCtx<'_>,
    challenge_id: Uuid,
    signature: &str,
    client: &SessionClient,
) -> Result<DeviceSignIn, ApprovalError> {
    let mut tx = ctx.db.begin().await?;
    // Read without a lock to learn whose account to lock first.
    let head: Option<(String, String, Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT kind, requester_kind, requester_id, approver_id, device_id
           FROM approvals WHERE id = $1",
    )
    .bind(challenge_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((kind, requester_kind, device_row_id, user_id, device_id)) = head else {
        return Err(ApprovalError::NotFound);
    };
    if Kind::parse(&kind) != Some(Kind::DeviceLogin) || requester_kind != "device" {
        return Err(ApprovalError::NotFound);
    }

    let locked = super::transition::lock_accounts(&mut tx, &[user_id]).await?;
    match locked.get(&user_id) {
        Some(a) if a.exists && !a.is_system => {
            if a.banned {
                return Err(ApprovalError::AccountBanned);
            }
        }
        _ => return Err(ApprovalError::KeyDeviceUnknown),
    }
    let device: KeyDeviceRow =
        sqlx::query_as("SELECT * FROM approval_devices WHERE id = $1 FOR UPDATE")
            .bind(device_row_id)
            .fetch_optional(&mut *tx)
            .await?
            .filter(|d: &KeyDeviceRow| d.user_id == user_id && d.device_id == device_id)
            .ok_or(ApprovalError::KeyDeviceUnknown)?;
    if let Some(e) = refusal(&device) {
        return Err(with_ready_at(&mut tx, &device, e).await?);
    }

    let row = match lock_for_answer(&mut tx, challenge_id, user_id, device_id).await {
        Ok(row) => row,
        Err(ApprovalError::Expired) => {
            tx.commit().await?;
            return Err(ApprovalError::Expired);
        }
        Err(e) => return Err(e),
    };
    verify(
        &row,
        ctx.origin(),
        &device.public_key,
        Decision::Approve,
        &[],
        signature,
    )?;
    set_state(
        &mut tx,
        row.id,
        &[ChallengeState::Issued],
        ChallengeState::Consumed,
    )
    .await?;

    // DC12: one live session per key device.
    let authenticator = AuthenticatorRef::key_device(device.id);
    let older: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sessions
          WHERE user_id = $1 AND authenticator_type = 'key_device' AND authenticator_id = $2
            AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(device.id)
    .fetch_all(&mut *tx)
    .await?;
    end_sessions_tx(&mut tx, ctx, user_id, &older, EndReason::Superseded).await?;

    let mut session = issue_session_tx(
        &mut tx,
        ctx,
        &NewSession {
            user_id,
            method: SignInMethod::DeviceKey,
            client,
            client_class: ClientClass::Android,
            authenticator: Some(authenticator),
        },
    )
    .await?;
    sqlx::query("UPDATE approval_devices SET last_used_at = NOW() WHERE id = $1")
        .bind(device.id)
        .execute(&mut *tx)
        .await?;
    let mut ev = NewSecurityEvent::new(
        user_id,
        EventType::SignIn,
        Actor::User(user_id),
        serde_json::json!({
            "auth_methods": SignInMethod::DeviceKey.auth_methods(),
            "assurance": session.assurance.as_str(),
        }),
    );
    ev.client = Some((ClientClass::Android, device.label.clone()));
    ev.ip = client.ip;
    ev.session_id = Some(session.session_id);
    ev.authenticator = Some(authenticator);
    session.event_ids.push(record(&mut tx, ev).await?);
    let password_change_required: bool =
        sqlx::query_scalar("SELECT must_change_password FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    tracing::info!(%user_id, session_id = %session.session_id, "device-key sign-in");
    Ok(DeviceSignIn {
        session,
        password_change_required,
    })
}

/// Ask for a `device_login` challenge (C26).
#[utoipa::path(
    post,
    path = "/auth/device-login/challenge",
    tag = "approvals",
    request_body = DeviceLoginChallengeRequest,
    responses(
        (status = 201, body = ApprovalChallenge),
        (status = 403, description = "KEY_DEVICE_REVOKED or ACCOUNT_BANNED"),
        (status = 404, description = "KEY_DEVICE_UNKNOWN"),
        (status = 409, description = "KEY_DEVICE_PENDING {ready_at}"),
        (status = 429, description = "TOO_MANY_ATTEMPTS"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all)]
pub async fn challenge_handler(
    State(state): State<AppState>,
    Json(body): Json<DeviceLoginChallengeRequest>,
) -> Result<(StatusCode, Json<ApprovalChallenge>), ApprovalError> {
    let c = challenge(&AuthCtx::from(&state), body.user_id, body.device_id).await?;
    Ok((StatusCode::CREATED, Json(c)))
}

/// Sign in with the device key (C26): a full session, the refresh token in
/// the body.
#[utoipa::path(
    post,
    path = "/auth/device-login",
    tag = "approvals",
    request_body = DeviceLoginRequest,
    responses(
        (status = 200, body = AuthResponse),
        (status = 400, description = "SIGNATURE_INVALID"),
        (status = 403, description = "KEY_DEVICE_REVOKED or ACCOUNT_BANNED"),
        (status = 404, description = "APPROVAL_NOT_FOUND or KEY_DEVICE_UNKNOWN"),
        (status = 409, description = "APPROVAL_STATE or KEY_DEVICE_PENDING"),
        (status = 410, description = "APPROVAL_EXPIRED"),
    ),
    security(()),
)]
#[tracing::instrument(skip_all, fields(challenge_id = %body.challenge_id))]
pub async fn sign_in_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<DeviceLoginRequest>,
) -> Result<Json<AuthResponse>, ApprovalError> {
    let client = SessionClient::from_request(&state, &headers, addr);
    let done = sign_in(
        &AuthCtx::from(&state),
        body.challenge_id,
        &body.signature,
        &client,
    )
    .await?;
    crate::security_events::spawn_delivery(&state, done.session.event_ids.clone());
    Ok(Json(
        done.session
            .into_response(done.password_change_required, true),
    ))
}
