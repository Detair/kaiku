//! Action-bound step-up grants (spec F7, C27 common rules; L1a owns the
//! store, LP adds the passkey method).
//!
//! A grant is for one action and one canonical target, valid ≤ 300 s,
//! single use, bound to the `sid` of a **full** session, and produced by a
//! passkey assertion (LP) or a `device_stepup` signature (C26). It is
//! consumed in the transaction of the action it authorises. Never a
//! step-up: password, TOTP, recovery code or OIDC, alone or together.
//!
//! Every function locks the session row and re-checks it live and full
//! (B.7 rule 4); [`consume_tx`] also re-checks that the authenticator that
//! produced the grant is still active.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use super::assurance::Assurance;
use super::error::{AuthError, AuthResult};
use super::middleware::AuthUser;
use super::strong::{AuthenticatorKind, AuthenticatorRef};

/// Grant lifetime (F7: at most 5 minutes; also a DB CHECK).
pub const STEP_UP_TTL_SECS: i64 = 300;

/// The action registry (C27). The wire strings are append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepUpAction {
    #[serde(rename = "passkey.add")]
    PasskeyAdd,
    #[serde(rename = "passkey.remove")]
    PasskeyRemove,
    #[serde(rename = "key_device.remove")]
    KeyDeviceRemove,
    #[serde(rename = "enrollment.issue")]
    EnrollmentIssue,
    #[serde(rename = "password.set")]
    PasswordSet,
    #[serde(rename = "password.remove")]
    PasswordRemove,
    #[serde(rename = "totp.set")]
    TotpSet,
    #[serde(rename = "totp.remove")]
    TotpRemove,
    #[serde(rename = "recovery_codes.create")]
    RecoveryCodesCreate,
    #[serde(rename = "identity.link")]
    IdentityLink,
    #[serde(rename = "identity.unlink")]
    IdentityUnlink,
    #[serde(rename = "sessions.end")]
    SessionsEnd,
    #[serde(rename = "recovery_contact.set")]
    RecoveryContactSet,
    #[serde(rename = "request.cancel")]
    RequestCancel,
    #[serde(rename = "event.repudiate")]
    EventRepudiate,
    #[serde(rename = "account.delete")]
    AccountDelete,
    #[serde(rename = "admin.elevate")]
    AdminElevate,
    #[serde(rename = "admin.enrollment.issue")]
    AdminEnrollmentIssue,
    #[serde(rename = "admin.authenticator.remove")]
    AdminAuthenticatorRemove,
    #[serde(rename = "admin.may_approve")]
    AdminMayApprove,
    #[serde(rename = "admin.factor.set_state")]
    AdminFactorSetState,
    #[serde(rename = "admin.recovery_contact.set")]
    AdminRecoveryContactSet,
}

/// What target an action takes (C27 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetRule {
    /// No target (`""`).
    None,
    /// A lowercase UUID.
    Id,
    /// A lowercase UUID or the literal `others` (`sessions.end`).
    IdOrOthers,
    /// A lowercase UUID or `""` to remove (`recovery_contact.set`).
    IdOrEmpty,
}

impl StepUpAction {
    /// Every action, in C27 order.
    pub const ALL: [Self; 22] = [
        Self::PasskeyAdd,
        Self::PasskeyRemove,
        Self::KeyDeviceRemove,
        Self::EnrollmentIssue,
        Self::PasswordSet,
        Self::PasswordRemove,
        Self::TotpSet,
        Self::TotpRemove,
        Self::RecoveryCodesCreate,
        Self::IdentityLink,
        Self::IdentityUnlink,
        Self::SessionsEnd,
        Self::RecoveryContactSet,
        Self::RequestCancel,
        Self::EventRepudiate,
        Self::AccountDelete,
        Self::AdminElevate,
        Self::AdminEnrollmentIssue,
        Self::AdminAuthenticatorRemove,
        Self::AdminMayApprove,
        Self::AdminFactorSetState,
        Self::AdminRecoveryContactSet,
    ];

    /// The C27 wire string (also signed in `device_stepup` contexts).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PasskeyAdd => "passkey.add",
            Self::PasskeyRemove => "passkey.remove",
            Self::KeyDeviceRemove => "key_device.remove",
            Self::EnrollmentIssue => "enrollment.issue",
            Self::PasswordSet => "password.set",
            Self::PasswordRemove => "password.remove",
            Self::TotpSet => "totp.set",
            Self::TotpRemove => "totp.remove",
            Self::RecoveryCodesCreate => "recovery_codes.create",
            Self::IdentityLink => "identity.link",
            Self::IdentityUnlink => "identity.unlink",
            Self::SessionsEnd => "sessions.end",
            Self::RecoveryContactSet => "recovery_contact.set",
            Self::RequestCancel => "request.cancel",
            Self::EventRepudiate => "event.repudiate",
            Self::AccountDelete => "account.delete",
            Self::AdminElevate => "admin.elevate",
            Self::AdminEnrollmentIssue => "admin.enrollment.issue",
            Self::AdminAuthenticatorRemove => "admin.authenticator.remove",
            Self::AdminMayApprove => "admin.may_approve",
            Self::AdminFactorSetState => "admin.factor.set_state",
            Self::AdminRecoveryContactSet => "admin.recovery_contact.set",
        }
    }

    /// Parse the exact wire string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.as_str() == s)
    }

    /// An `admin.*` action: only a system admin may ask for its step-up.
    #[must_use]
    pub const fn is_admin(self) -> bool {
        matches!(
            self,
            Self::AdminElevate
                | Self::AdminEnrollmentIssue
                | Self::AdminAuthenticatorRemove
                | Self::AdminMayApprove
                | Self::AdminFactorSetState
                | Self::AdminRecoveryContactSet
        )
    }

    /// The action's target rule (C27).
    #[must_use]
    pub const fn target_rule(self) -> TargetRule {
        match self {
            Self::PasskeyAdd
            | Self::EnrollmentIssue
            | Self::PasswordSet
            | Self::PasswordRemove
            | Self::TotpSet
            | Self::TotpRemove
            | Self::RecoveryCodesCreate
            | Self::IdentityLink
            | Self::AccountDelete
            | Self::AdminElevate => TargetRule::None,
            Self::PasskeyRemove
            | Self::KeyDeviceRemove
            | Self::IdentityUnlink
            | Self::RequestCancel
            | Self::EventRepudiate
            | Self::AdminEnrollmentIssue
            | Self::AdminAuthenticatorRemove
            | Self::AdminMayApprove
            | Self::AdminFactorSetState
            | Self::AdminRecoveryContactSet => TargetRule::Id,
            Self::SessionsEnd => TargetRule::IdOrOthers,
            Self::RecoveryContactSet => TargetRule::IdOrEmpty,
        }
    }
}

/// A canonical step-up target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepUpTarget {
    /// `""`.
    None,
    /// A lowercase hyphenated UUID.
    Id(Uuid),
    /// The literal `others` (`sessions.end`).
    Others,
}

impl StepUpTarget {
    /// Parse `raw` for `action`: exactly the canonical form (lowercase
    /// hyphenated UUID, `others`, `""`) that the action's rule allows;
    /// anything else → 400 `VALIDATION_ERROR`.
    pub fn parse(action: StepUpAction, raw: &str) -> AuthResult<Self> {
        let invalid = || AuthError::Validation(format!("invalid target for {}", action.as_str()));
        let as_id = || {
            Uuid::parse_str(raw)
                .ok()
                .filter(|id| id.to_string() == raw)
                .map(Self::Id)
                .ok_or_else(invalid)
        };
        match (action.target_rule(), raw) {
            (TargetRule::None | TargetRule::IdOrEmpty, "") => Ok(Self::None),
            (TargetRule::None, _) => Err(invalid()),
            (TargetRule::IdOrOthers, "others") => Ok(Self::Others),
            (TargetRule::Id | TargetRule::IdOrOthers | TargetRule::IdOrEmpty, _) => as_id(),
        }
    }

    /// The canonical string (signed as `target` in `device_stepup`).
    #[must_use]
    pub fn canonical(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Id(id) => id.to_string(),
            Self::Others => "others".to_string(),
        }
    }

    /// `SHA-256(lp(target))` (C27).
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        crate::approvals::context::target_hash(&self.canonical())
    }
}

/// How a grant was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepUpVia {
    Passkey,
    Device,
}

impl StepUpVia {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::Device => "device",
        }
    }

    fn parse(s: &str) -> Self {
        if s == "passkey" {
            Self::Passkey
        } else {
            Self::Device
        }
    }
}

/// `StepUpGrant` (C27 common rules).
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct StepUpGrant {
    #[schema(value_type = String)]
    pub action: StepUpAction,
    pub target: String,
    pub via: StepUpVia,
    pub expires_at: DateTime<Utc>,
}

/// A consumed grant (for the action's audit and notice).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumedGrant {
    pub id: Uuid,
    pub via: StepUpVia,
    pub by: AuthenticatorRef,
}

/// Lock the caller's account (B.7 rule 1; a no-op when the caller already
/// holds it), then its session row, and re-check the session live and full.
/// Every revocation takes the account first too, so a grant is produced or
/// used either before a revocation commits or after it (and then sees the
/// ended session or the revoked authenticator), and the action's own rows
/// that reference the account (elevation, audit) never wait in a cycle.
async fn lock_full_session(conn: &mut PgConnection, auth: &AuthUser) -> AuthResult<Uuid> {
    let sid = auth.session_id.ok_or(AuthError::InvalidToken)?;
    crate::approvals::transition::lock_accounts(conn, &[auth.id]).await?;
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT assurance FROM sessions
          WHERE id = $1 AND user_id = $2 AND ended_at IS NULL
            AND expires_at > NOW() AND absolute_expires_at > NOW()
          FOR UPDATE",
    )
    .bind(sid)
    .bind(auth.id)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        None => Err(AuthError::InvalidToken),
        Some((a,)) if Assurance::parse(&a) != Assurance::Full => {
            Err(AuthError::FullSessionRequired)
        }
        Some(_) => Ok(sid),
    }
}

/// Store a grant for (`sid`, `action`, `target`), produced by `by` via
/// `via`. The session must be live and full and `by` active.
#[tracing::instrument(skip(conn, auth), fields(user_id = %auth.id, action = action.as_str()))]
pub async fn grant_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    action: StepUpAction,
    target: &StepUpTarget,
    via: StepUpVia,
    by: AuthenticatorRef,
) -> AuthResult<StepUpGrant> {
    let sid = lock_full_session(conn, auth).await?;
    // The producing authenticator must be active and the caller's own
    // (L1a review m7: callers are not trusted for the owner of `by`).
    if !super::strong::is_active(conn, by).await?
        || !super::strong::belongs_to(conn, auth.id, by).await?
    {
        return Err(AuthError::StepUpRequired);
    }
    let now = Utc::now();
    let expires_at = now + Duration::seconds(STEP_UP_TTL_SECS);
    sqlx::query(
        "INSERT INTO step_up_grants (id, session_id, user_id, action, target_hash, target, via,
                                     authenticator_type, authenticator_id, created_at, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(Uuid::new_v4())
    .bind(sid)
    .bind(auth.id)
    .bind(action.as_str())
    .bind(target.hash().to_vec())
    .bind(target.canonical())
    .bind(via.as_str())
    .bind(by.as_str())
    .bind(by.id)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *conn)
    .await?;
    Ok(StepUpGrant {
        action,
        target: target.canonical(),
        via,
        expires_at,
    })
}

#[derive(sqlx::FromRow)]
struct OpenGrant {
    id: Uuid,
    via: String,
    authenticator_type: String,
    authenticator_id: Uuid,
}

async fn open_grant(
    conn: &mut PgConnection,
    sid: Uuid,
    action: StepUpAction,
    target: &StepUpTarget,
    lock: bool,
) -> AuthResult<OpenGrant> {
    let sql = if lock {
        "SELECT id, via, authenticator_type, authenticator_id FROM step_up_grants
          WHERE session_id = $1 AND action = $2 AND target_hash = $3
            AND used_at IS NULL AND voided_at IS NULL AND expires_at > NOW()
          ORDER BY created_at, id LIMIT 1 FOR UPDATE"
    } else {
        "SELECT id, via, authenticator_type, authenticator_id FROM step_up_grants
          WHERE session_id = $1 AND action = $2 AND target_hash = $3
            AND used_at IS NULL AND voided_at IS NULL AND expires_at > NOW()
          ORDER BY created_at, id LIMIT 1"
    };
    let grant: OpenGrant = sqlx::query_as(sql)
        .bind(sid)
        .bind(action.as_str())
        .bind(target.hash().to_vec())
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(AuthError::StepUpRequired)?;
    // B.7 rule 4: a grant is used only while its authenticator is active.
    let kind =
        AuthenticatorKind::parse(&grant.authenticator_type).ok_or(AuthError::StepUpRequired)?;
    let by = AuthenticatorRef {
        kind,
        id: grant.authenticator_id,
    };
    if !super::strong::is_active(conn, by).await? {
        return Err(AuthError::StepUpRequired);
    }
    Ok(grant)
}

/// Check that a usable grant exists without consuming it (LP's
/// `register/start`).
pub async fn peek_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    action: StepUpAction,
    target: &StepUpTarget,
) -> AuthResult<()> {
    let sid = lock_full_session(conn, auth).await?;
    open_grant(conn, sid, action, target, false)
        .await
        .map(|_| ())
}

/// Consume the oldest usable grant for (`sid`, `action`, `target`) in the
/// caller's transaction (the action's own). None → 403 `STEP_UP_REQUIRED`.
#[tracing::instrument(skip(conn, auth), fields(user_id = %auth.id, action = action.as_str()))]
pub async fn consume_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    action: StepUpAction,
    target: &StepUpTarget,
) -> AuthResult<ConsumedGrant> {
    let sid = lock_full_session(conn, auth).await?;
    let grant = open_grant(conn, sid, action, target, true).await?;
    sqlx::query("UPDATE step_up_grants SET used_at = NOW() WHERE id = $1")
        .bind(grant.id)
        .execute(&mut *conn)
        .await?;
    let kind =
        AuthenticatorKind::parse(&grant.authenticator_type).ok_or(AuthError::StepUpRequired)?;
    Ok(ConsumedGrant {
        id: grant.id,
        via: StepUpVia::parse(&grant.via),
        by: AuthenticatorRef {
            kind,
            id: grant.authenticator_id,
        },
    })
}

/// Record the `step_up_action {action, target_label}` event of a consumed
/// grant (C28), with the session and the authenticator that produced it.
pub async fn record_action_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    grant: &ConsumedGrant,
    action: StepUpAction,
    target_label: &str,
) -> sqlx::Result<Uuid> {
    use crate::security_events::{record, Actor, EventType, NewSecurityEvent};
    let mut ev = NewSecurityEvent::new(
        auth.id,
        EventType::StepUpAction,
        Actor::User(auth.id),
        serde_json::json!({ "action": action.as_str(), "target_label": target_label }),
    );
    ev.session_id = auth.session_id;
    ev.authenticator = Some(grant.by);
    record(conn, ev).await
}

/// The display label of a target (the `step_up_action` event's
/// `target_label`; the same text a `device_stepup` signs).
pub async fn target_label_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
    action: StepUpAction,
    target: &StepUpTarget,
) -> AuthResult<String> {
    crate::approvals::handlers::target_label(conn, user_id, action, target)
        .await
        .map_err(|e| match e {
            crate::approvals::error::ApprovalError::Database(d) => AuthError::Database(d),
            other => AuthError::Internal(other.to_string()),
        })
}

/// Void the open grants of these sessions (B.7 rule 2).
pub async fn void_for_sessions(conn: &mut PgConnection, sids: &[Uuid]) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE step_up_grants SET voided_at = NOW()
          WHERE session_id = ANY($1) AND used_at IS NULL AND voided_at IS NULL",
    )
    .bind(sids)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

/// Void the open grants an authenticator produced (B.7 rule 2).
pub async fn void_for_authenticator(
    conn: &mut PgConnection,
    a: AuthenticatorRef,
) -> sqlx::Result<u64> {
    Ok(sqlx::query(
        "UPDATE step_up_grants SET voided_at = NOW()
          WHERE authenticator_type = $1 AND authenticator_id = $2
            AND used_at IS NULL AND voided_at IS NULL",
    )
    .bind(a.as_str())
    .bind(a.id)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::StepUpAction;

    #[test]
    fn every_admin_action_is_gated() {
        // Review round 3 (Fable): a new `admin.*` action cannot slip past
        // `require_admin_for_step_up` by a missing `is_admin` arm.
        assert!(StepUpAction::ALL
            .iter()
            .all(|a| a.is_admin() == a.as_str().starts_with("admin.")));
        assert_eq!(StepUpAction::ALL.iter().filter(|a| a.is_admin()).count(), 6);
    }
}
