//! Key devices (spec B.1, SM4; C26 `KeyDevice`).
//!
//! `pending` → `active` → `revoked`. Only an `active` device authenticates
//! (sign-in, step-up, approvals); revocation ends its sessions and voids
//! its challenges and grants in the same transaction (B.7 rule 2), and its
//! push subscription is deleted after its last notice was sent.

pub mod attestation;
pub mod handlers;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::error::ApprovalError;
use crate::api::AppState;
use crate::auth::ctx::AuthCtx;
use crate::auth::strong::{AuthenticatorKind, AuthenticatorRef};
use crate::auth::AuthUser;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// Device states (SM4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Pending,
    Active,
    Revoked,
}

impl DeviceState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

/// User verification of the device key (B.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvClass {
    PerUse,
    Window10s,
}

impl UvClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PerUse => "per_use",
            Self::Window10s => "window_10s",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "per_use" => Some(Self::PerUse),
            "window_10s" => Some(Self::Window10s),
            _ => None,
        }
    }
}

/// How a device was enrolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrolledVia {
    Own,
    Admin,
    Request,
    Registration,
    Cli,
}

impl EnrolledVia {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Admin => "admin",
            Self::Request => "request",
            Self::Registration => "registration",
            Self::Cli => "cli",
        }
    }
}

/// Why a device was revoked (C26 `revoke_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    Owner,
    Admin,
    Request,
    Repudiated,
    KeyInvalidated,
    AccountDeleted,
    Cli,
}

impl RevokeReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Request => "request",
            Self::Repudiated => "repudiated",
            Self::KeyInvalidated => "key_invalidated",
            Self::AccountDeleted => "account_deleted",
            Self::Cli => "cli",
        }
    }

    /// `authenticator_removed.details.by` (C28).
    const fn removed_by(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Request => "request",
            Self::Cli => "cli",
            Self::Owner | Self::Repudiated | Self::KeyInvalidated | Self::AccountDeleted => "self",
        }
    }
}

/// Every `approval_devices` column.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KeyDeviceRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub device_id: Uuid,
    pub platform: String,
    pub public_key: Vec<u8>,
    pub public_key_hash: Vec<u8>,
    pub attestation_chain: Vec<Vec<u8>>,
    pub attestation_properties: serde_json::Value,
    pub uv_class: String,
    pub api_level: Option<i32>,
    pub label: String,
    pub state: String,
    pub enrolled_via: String,
    pub enrollment_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoke_reason: Option<String>,
}

/// `KeyDevice` (C26 wire).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct KeyDevice {
    pub id: Uuid,
    pub device_id: Uuid,
    pub label: String,
    pub platform: String,
    pub state: String,
    pub enrolled_via: String,
    pub uv_class: String,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    /// When a `pending` device becomes active (its account request; L1b).
    pub ready_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoke_reason: Option<String>,
    /// The calling session is this device's own session.
    pub is_current: bool,
}

impl KeyDeviceRow {
    /// The wire form for a reader whose session authenticator is `current`.
    #[must_use]
    pub fn to_wire(&self, current: Option<AuthenticatorRef>) -> KeyDevice {
        KeyDevice {
            id: self.id,
            device_id: self.device_id,
            label: self.label.clone(),
            platform: self.platform.clone(),
            state: self.state.clone(),
            enrolled_via: self.enrolled_via.clone(),
            uv_class: self.uv_class.clone(),
            created_at: self.created_at,
            activated_at: self.activated_at,
            ready_at: None,
            last_used_at: self.last_used_at,
            revoked_at: self.revoked_at,
            revoke_reason: self.revoke_reason.clone(),
            is_current: current == Some(AuthenticatorRef::key_device(self.id)),
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state == DeviceState::Active.as_str()
    }
}

/// One device by row id.
pub async fn find(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<KeyDeviceRow>> {
    sqlx::query_as("SELECT * FROM approval_devices WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

/// A session whose authenticator is an `active` key device of its user
/// (C26 "Key device").
#[derive(Debug, Clone)]
pub struct KeyDeviceSession {
    pub auth: AuthUser,
    pub device: KeyDeviceRow,
}

/// The request's session snapshot names a key device (no lock, no query):
/// the cheap first check before anything is locked. Else 403
/// `KEY_DEVICE_REQUIRED`.
pub fn require_key_device_snapshot(auth: &AuthUser) -> Result<AuthenticatorRef, ApprovalError> {
    match auth.session.as_ref().and_then(|s| s.authenticator) {
        Some(a) if a.kind == AuthenticatorKind::KeyDevice => Ok(a),
        _ => Err(ApprovalError::KeyDeviceRequired),
    }
}

/// The caller's session as a key-device session, else 403
/// `KEY_DEVICE_REQUIRED` (also for `pending` and `revoked` devices).
///
/// B.7 rule 1: takes the caller's account lock (a no-op when the caller
/// already holds it together with other accounts), then locks the device
/// row and the authorising session row and re-checks both: the snapshot
/// `require_auth` read may be stale once the request waited for the lock.
/// An ended session → 401 `INVALID_TOKEN`; a banned account → 403.
/// Lock order: account → device → session (as every revocation).
pub async fn key_device_session(
    conn: &mut PgConnection,
    auth: &AuthUser,
) -> Result<KeyDeviceSession, ApprovalError> {
    let a = require_key_device_snapshot(auth)?;
    let sid = auth.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let locked = crate::approvals::transition::lock_accounts(conn, &[auth.id]).await?;
    if locked.get(&auth.id).is_some_and(|acct| acct.banned) {
        return Err(ApprovalError::AccountBanned);
    }
    let device: KeyDeviceRow =
        sqlx::query_as("SELECT * FROM approval_devices WHERE id = $1 FOR UPDATE")
            .bind(a.id)
            .fetch_optional(&mut *conn)
            .await?
            .filter(|d: &KeyDeviceRow| d.user_id == auth.id && d.is_active())
            .ok_or(ApprovalError::KeyDeviceRequired)?;
    let session: Option<(String, Option<String>, Option<Uuid>)> = sqlx::query_as(
        "SELECT assurance, authenticator_type, authenticator_id FROM sessions
          WHERE id = $1 AND user_id = $2 AND ended_at IS NULL
            AND expires_at > NOW() AND absolute_expires_at > NOW()
          FOR UPDATE",
    )
    .bind(sid)
    .bind(auth.id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((assurance, kind, authenticator_id)) = session else {
        return Err(crate::auth::AuthError::InvalidToken.into());
    };
    if assurance != "full"
        || kind.as_deref() != Some(AuthenticatorKind::KeyDevice.as_str())
        || authenticator_id != Some(device.id)
    {
        return Err(ApprovalError::KeyDeviceRequired);
    }
    Ok(KeyDeviceSession {
        auth: auth.clone(),
        device,
    })
}

/// Revoke a device in the caller's transaction.
///
/// State, the B.7 rule-2 cascade (sessions, challenges, grants), and the
/// notice event (`authenticator_removed`, or `key_invalidated`). Returns
/// the revoked device and the event ids to deliver after commit.
#[tracing::instrument(skip(conn, ctx))]
pub async fn revoke_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    reason: RevokeReason,
    actor: Actor,
) -> Result<(KeyDevice, Vec<Uuid>), ApprovalError> {
    let row: KeyDeviceRow = sqlx::query_as(
        "UPDATE approval_devices
            SET state = 'revoked', revoked_at = NOW(), revoke_reason = $2
          WHERE id = $1 AND state <> 'revoked'
          RETURNING *",
    )
    .bind(id)
    .bind(reason.as_str())
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApprovalError::KeyDeviceRevoked)?;
    let a = AuthenticatorRef::key_device(row.id);
    crate::approvals::cascade::on_authenticator_revoked(conn, ctx, row.user_id, a).await?;
    let (event_type, details) = if reason == RevokeReason::KeyInvalidated {
        (EventType::KeyInvalidated, serde_json::json!({}))
    } else {
        (
            EventType::AuthenticatorRemoved,
            serde_json::json!({"type": "key_device", "by": reason.removed_by()}),
        )
    };
    let mut ev = NewSecurityEvent::new(row.user_id, event_type, actor, details);
    ev.authenticator = Some(a);
    let event_id = record(conn, ev).await?;
    Ok((row.to_wire(None), vec![event_id]))
}

/// After a revocation committed: deliver its notices (so the removed phone
/// still gets the last one), tell the user's sockets, then delete the
/// device's push subscriptions (C19 amendment 3 in C26).
pub async fn after_revoke(state: &AppState, user_id: Uuid, device_id: Uuid, event_ids: Vec<Uuid>) {
    if let Err(e) = crate::security_events::notices::deliver(state, &event_ids).await {
        tracing::warn!(error = %e, "revocation notice delivery failed; the sweep retries");
    }
    notify_authenticators_changed(state, user_id).await;
    if let Err(e) = crate::push::delete_device_subscriptions(&state.db, user_id, device_id).await {
        tracing::warn!(error = %e, "failed to delete the revoked device's push subscriptions");
    }
}

/// WS hint `authenticators_changed {user_id}` to every socket of the user.
pub async fn notify_authenticators_changed(state: &AppState, user_id: Uuid) {
    if let Err(e) = crate::ws::broadcast_to_user(
        &state.redis,
        user_id,
        &crate::ws::ServerEvent::AuthenticatorsChanged { user_id },
    )
    .await
    {
        tracing::warn!(error = %e, "authenticators_changed hint failed");
    }
}
