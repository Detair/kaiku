//! SM5 transitions of a "Set up this PC" attempt, up to `code` (code entry
//! and the passkey are in [`super::enrolment`]).
//!
//! **SECURITY CRITICAL.** Lock order (spec "Locking (normative)", added to
//! B.7): every transition starts at [`lock_attempt`], which takes the
//! account, then the requesting session row, then the attempt, and re-checks
//! under those locks that the session is live, the account may approve and
//! the phone is an active key device. A stale attempt moves to `expired` with
//! a reason the PC can read and never advances, even before the sweep. The
//! phone's transitions then lock their key device and session
//! (`lock_phone`, the checks of `key_device_session` on the transition's
//! clock) and the challenge. Session-ending paths without the
//! account lock (logout, refresh) never touch attempts or challenges, so none
//! of this can form a cycle with them.
//!
//! Every check of a transition precedes its first effect, so a caller may
//! commit after any error but a database one: what it keeps are the lazy
//! marks of [`lock_attempt`] (an expiry, a claim back to `waiting`).

use chrono::{DateTime, Duration, Utc};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::{AttemptState, CHALLENGE_TTL, CODE_DIGITS, CODE_MINUTES, QR_ROTATE_SECS};
use crate::approvals::context::{sid_hash, CanonicalContext, PcSetupContext};
use crate::approvals::core::{
    check_for_answer_at, issue_until, lock_challenge_row, set_state, verify, ChallengeRow,
    ChallengeState, NewChallenge, Requester,
};
use crate::approvals::devices::{require_key_device_snapshot, KeyDeviceRow, KeyDeviceSession};
use crate::approvals::error::ApprovalError;
use crate::approvals::secrets::{ct_eq, keyed_hash, random_digits, Secret256, SecretPurpose};
use crate::approvals::signing::{Decision, Kind};
use crate::approvals::transition::{lock_accounts, LockedAccount};
use crate::approvals::types::ApprovalChallenge;
use crate::auth::assurance::Assurance;
use crate::auth::client_class::ClientClass;
use crate::auth::ctx::AuthCtx;
use crate::auth::revocation::{end_sessions_tx, EndReason};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::AuthUser;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// The shortest `pc_setup` challenge worth issuing.
const MIN_CHALLENGE_SECS: i64 = 2;

/// Bound of the place shown on the phone (as the login label).
const MAX_PLACE_CHARS: usize = 64;

/// Why an attempt ended (`pc_setup_attempts.reason`, readable by the PC and
/// the phone). Append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCause {
    /// The requesting session ended or ran out.
    SessionEnded,
    /// The scanning phone is no longer an active key device.
    DeviceRevoked,
    /// The account has no active key device left (an attempt in `waiting`).
    NoKeyDevice,
    /// `may_approve` was turned off.
    MayApproveOff,
    AccountBanned,
    /// The 10 minutes of `waiting` and `claimed` ran out.
    Timeout,
    /// The code's 5 minutes ran out.
    CodeExpired,
    /// A fresh start of the same session (Locking 6).
    Restarted,
    /// The PC cancelled.
    Cancelled,
    /// The phone said No.
    Refused,
    /// The phone said "I didn't start this".
    Reported,
    /// The third wrong code.
    TooManyCodes,
    /// The enrolment's 10 minutes ran out before the passkey was made.
    EnrollmentExpired,
}

impl EndCause {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionEnded => "session_ended",
            Self::DeviceRevoked => "device_revoked",
            Self::NoKeyDevice => "no_key_device",
            Self::MayApproveOff => "may_approve_off",
            Self::AccountBanned => "account_banned",
            Self::Timeout => "timeout",
            Self::CodeExpired => "code_expired",
            Self::Restarted => "restarted",
            Self::Cancelled => "cancelled",
            Self::Refused => "refused",
            Self::Reported => "reported",
            Self::TooManyCodes => "too_many_codes",
            Self::EnrollmentExpired => "enrollment_expired",
        }
    }
}

/// A stored attempt.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AttemptRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub state: String,
    pub reason: Option<String>,
    pub qr_token_hash: Option<Vec<u8>>,
    pub qr_rotates_at: Option<DateTime<Utc>>,
    pub challenge_id: Option<Uuid>,
    pub device_id: Option<Uuid>,
    pub code_hash: Option<Vec<u8>>,
    pub code_expires_at: Option<DateTime<Utc>>,
    pub tries: i16,
    pub enrollment_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// How the right code arrived (`web` or `desktop_flow`), see [`CodePath`].
    pub code_via: Option<String>,
    pub code_flow_id: Option<Uuid>,
    /// On the web path, the keyed hash of the typing page's proof.
    pub web_proof_hash: Option<Vec<u8>>,
}

impl AttemptRow {
    /// The SM5 state (an unknown one reads as terminal).
    #[must_use]
    pub fn state(&self) -> AttemptState {
        AttemptState::parse(&self.state).unwrap_or(AttemptState::Expired)
    }
}

/// The requesting session as it is under its row lock.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RequestingSession {
    pub id: Uuid,
    pub live: bool,
    pub assurance: String,
    pub client_class: String,
    pub user_agent: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// An attempt under the account, session and attempt locks.
#[derive(Debug, Clone)]
pub struct LockedAttempt {
    pub attempt: AttemptRow,
    pub account: LockedAccount,
    /// `None` when the session row is gone.
    pub session: Option<RequestingSession>,
    /// The attempt's enrolment, locked after the attempt (`enrolled` only).
    pub enrolment: Option<EnrolmentLock>,
    /// Set when this lock expired a stale attempt.
    pub ended_now: Option<EndCause>,
    /// The database's clock, read once after every lock of the transition
    /// is held: every liveness check and every deadline the transition
    /// issues uses this one value (Locking 7).
    pub now: DateTime<Utc>,
}

/// The attempt's enrolment row as it is under its lock.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EnrolmentLock {
    pub id: Uuid,
    pub variant: String,
    pub subject_id: Uuid,
    pub issuer_session_id: Option<Uuid>,
    pub state: String,
    pub expires_at: DateTime<Utc>,
}

/// An attempt's rows under their locks, before the clock is read.
///
/// Account → requesting session → attempt → its enrolment. A transition takes
/// its own further locks, then [`read_clock`], then [`AttemptLocks::settle`].
#[derive(Debug)]
pub struct AttemptLocks {
    pub attempt: AttemptRow,
    pub account: LockedAccount,
    session: Option<SessionRow>,
    enrolment: Option<EnrolmentLock>,
}

impl LockedAttempt {
    /// Why the attempt ended, whether this lock ended it or an earlier
    /// transition, poll or sweep did; `None` while it is live (and for a
    /// `completed` attempt, which has no reason: callers check the state).
    #[must_use]
    pub fn end_reason(&self) -> Option<&str> {
        match self.ended_now {
            Some(cause) => Some(cause.as_str()),
            None if self.attempt.state().is_terminal() => self.attempt.reason.as_deref(),
            None => None,
        }
    }
}

fn token_hash(ctx: &AuthCtx<'_>, token: &[u8]) -> [u8; 32] {
    keyed_hash(ctx.hmac_key(), SecretPurpose::PcSetupToken, token)
}

/// `kaiku://pcsetup/v1?o=<percent-encoded origin>&t=<43-char b64u token>`.
#[must_use]
pub fn qr_payload(origin: &str, token: &Secret256) -> String {
    let o: String = url::form_urlencoded::byte_serialize(origin.as_bytes()).collect();
    format!("kaiku://pcsetup/v1?o={o}&t={}", token.to_b64u())
}

async fn read(conn: &mut PgConnection, id: Uuid, lock: bool) -> sqlx::Result<Option<AttemptRow>> {
    let sql = if lock {
        "SELECT * FROM pc_setup_attempts WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT * FROM pc_setup_attempts WHERE id = $1"
    };
    sqlx::query_as(sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
}

/// Lock the session row `sid` of `user` (its liveness is decided later, on
/// the transition's one clock).
async fn lock_session_row(
    conn: &mut PgConnection,
    user: Uuid,
    sid: Uuid,
) -> sqlx::Result<Option<SessionRow>> {
    sqlx::query_as(
        "SELECT id, ended_at, expires_at, absolute_expires_at,
                assurance, client_class,
                -- The User-Agent of the session's creation only: a refresh cannot
                -- change it, and a session created without one keeps none.
                created_user_agent AS user_agent,
                city, country, created_at
           FROM sessions WHERE id = $1 AND user_id = $2
            FOR UPDATE",
    )
    .bind(sid)
    .bind(user)
    .fetch_optional(&mut *conn)
    .await
}

impl SessionRow {
    fn at(self, now: DateTime<Utc>) -> RequestingSession {
        RequestingSession {
            id: self.id,
            live: self.ended_at.is_none()
                && self.expires_at > now
                && self.absolute_expires_at > now,
            assurance: self.assurance,
            client_class: self.client_class,
            user_agent: self.user_agent,
            city: self.city,
            country: self.country,
            created_at: self.created_at,
        }
    }
}

/// The database's clock now (`clock_timestamp()`, not the transaction's
/// start): read once per transition, after its last lock.
pub async fn read_clock(conn: &mut PgConnection) -> sqlx::Result<DateTime<Utc>> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *conn)
        .await
}

/// The locked session row, before its liveness is decided.
#[derive(Debug, sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    ended_at: Option<DateTime<Utc>>,
    expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
    assurance: String,
    client_class: String,
    user_agent: Option<String>,
    city: Option<String>,
    country: Option<String>,
    created_at: DateTime<Utc>,
}

/// Whether `user` has an active key device.
async fn has_active_device(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE user_id = $1 AND state = 'active')",
    )
    .bind(user)
    .fetch_one(&mut *conn)
    .await
}

/// Lock an attempt for a transition (spec Locking 2 and 3).
///
/// [`lock_attempt_rows`], then the clock, then [`AttemptLocks::settle`]: for
/// a transition that takes no further lock.
pub async fn lock_attempt(
    conn: &mut PgConnection,
    attempt_id: Uuid,
) -> Result<LockedAttempt, ApprovalError> {
    let rows = lock_attempt_rows(conn, attempt_id).await?;
    let now = read_clock(conn).await?;
    rows.settle(conn, now).await
}

/// Take an attempt's locks; nothing is decided or written yet.
///
/// The account, then the requesting session row (held through commit), then the attempt and, while
/// it is `enrolled`, its enrolment. Unknown → 404.
pub async fn lock_attempt_rows(
    conn: &mut PgConnection,
    attempt_id: Uuid,
) -> Result<AttemptLocks, ApprovalError> {
    // `user_id` and `session_id` never change: read them to learn the locks.
    let head: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT user_id, session_id FROM pc_setup_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (user, sid) = head.ok_or(ApprovalError::NotFound)?;
    let account = lock_accounts(conn, &[user])
        .await?
        .remove(&user)
        .ok_or(ApprovalError::NotFound)?;
    let session = lock_session_row(conn, user, sid).await?;
    let attempt = read(conn, attempt_id, true)
        .await?
        .ok_or(ApprovalError::NotFound)?;
    let enrolment = match attempt.enrollment_id {
        Some(e) if attempt.state() == AttemptState::Enrolled => {
            sqlx::query_as(
                "SELECT id, variant, subject_id, issuer_session_id, state, expires_at
                   FROM enrollments WHERE id = $1 FOR UPDATE",
            )
            .bind(e)
            .fetch_optional(&mut *conn)
            .await?
        }
        _ => None,
    };
    Ok(AttemptLocks {
        attempt,
        account,
        session,
        enrolment,
    })
}

impl AttemptLocks {
    /// Decide on `now` (the clock read after every lock of the transition):
    /// a live attempt whose session ended, whose account may no longer
    /// approve or is banned, whose phone was revoked (or, in `waiting`,
    /// whose account has no active key device left) or whose timer ran out
    /// moves to `expired` with that reason (and its open challenge is void);
    /// a claim whose challenge is gone returns to `waiting` without a token.
    pub async fn settle(
        self,
        conn: &mut PgConnection,
        now: DateTime<Utc>,
    ) -> Result<LockedAttempt, ApprovalError> {
        let mut locked = LockedAttempt {
            attempt: self.attempt,
            account: self.account,
            session: self.session.map(|s| s.at(now)),
            enrolment: self.enrolment,
            ended_now: None,
            now,
        };
        refresh(conn, &mut locked).await?;
        Ok(locked)
    }
}

/// Why a live attempt can no longer advance, if it cannot.
async fn stale_cause(conn: &mut PgConnection, l: &LockedAttempt) -> sqlx::Result<Option<EndCause>> {
    let a = &l.attempt;
    let state = a.state();
    if !l.session.as_ref().is_some_and(|s| s.live) || !l.account.exists {
        return Ok(Some(EndCause::SessionEnded));
    }
    if l.account.banned {
        return Ok(Some(EndCause::AccountBanned));
    }
    if !l.account.may_approve {
        return Ok(Some(EndCause::MayApproveOff));
    }
    match a.device_id {
        Some(device) => {
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM approval_devices
                                 WHERE id = $1 AND user_id = $2 AND state = 'active')",
            )
            .bind(device)
            .bind(a.user_id)
            .fetch_one(&mut *conn)
            .await?;
            if !active {
                return Ok(Some(EndCause::DeviceRevoked));
            }
        }
        None if state == AttemptState::Waiting && !has_active_device(conn, a.user_id).await? => {
            return Ok(Some(EndCause::NoKeyDevice));
        }
        None => {}
    }
    let now = l.now;
    Ok(match state {
        AttemptState::Waiting | AttemptState::Claimed if a.expires_at <= now => {
            Some(EndCause::Timeout)
        }
        AttemptState::Code if a.code_expires_at.is_none_or(|t| t <= now) => {
            Some(EndCause::CodeExpired)
        }
        AttemptState::Enrolled => {
            let live = l
                .enrolment
                .as_ref()
                .is_some_and(|e| e.state == "issued" && e.expires_at > now);
            (!live).then_some(EndCause::EnrollmentExpired)
        }
        _ => None,
    })
}

async fn refresh(conn: &mut PgConnection, l: &mut LockedAttempt) -> Result<(), ApprovalError> {
    if l.attempt.state().is_terminal() {
        return Ok(());
    }
    if let Some(cause) = stale_cause(conn, l).await? {
        end_tx(conn, &mut l.attempt, AttemptState::Expired, cause, l.now).await?;
        l.ended_now = Some(cause);
        return Ok(());
    }
    if l.attempt.state() == AttemptState::Claimed {
        let live: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM approvals
                             WHERE id = $1 AND state = 'issued' AND expires_at > $2)",
        )
        .bind(l.attempt.challenge_id)
        .bind(l.now)
        .fetch_one(&mut *conn)
        .await?;
        if !live {
            reopen_tx(conn, &mut l.attempt).await?;
        }
    }
    Ok(())
}

/// Void the attempt's open challenge (the phone's answer then fails).
async fn void_challenge(conn: &mut PgConnection, a: &AttemptRow) -> sqlx::Result<()> {
    if let Some(c) = a.challenge_id {
        sqlx::query("UPDATE approvals SET state = 'expired' WHERE id = $1 AND state = 'issued'")
            .bind(c)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

// Every attempt UPDATE names the state it leaves. Under the locks of
// [`lock_attempt`] it always matches; a caller holding an old value instead
// gets no row, an internal error, and `settle` rolls the transition back.

/// `claimed` → `waiting` (SM5 release / challenge expiry, as in SM1): the
/// challenge is void, the claimer forgotten and the token dropped, so the
/// PC's next poll shows a new QR.
async fn reopen_tx(conn: &mut PgConnection, a: &mut AttemptRow) -> sqlx::Result<()> {
    void_challenge(conn, a).await?;
    *a = sqlx::query_as(
        "UPDATE pc_setup_attempts
            SET state = 'waiting', challenge_id = NULL, device_id = NULL,
                qr_token_hash = NULL, qr_rotates_at = NULL
          WHERE id = $1 AND state = 'claimed'
          RETURNING *",
    )
    .bind(a.id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(())
}

/// End a live attempt in `to` with `cause`.
///
/// Its token and code are dropped (every end spends the code), its open
/// challenge void and its `issued` enrolment
/// `cancelled` (a cancel or restart) or `expired` (anything else), after the
/// attempt (Locking 3). A ceremony or desktop flow of that enrolment then
/// finishes nothing: each finish re-checks the enrolment under the same
/// locks. The attempt must be locked ([`lock_attempt`]).
pub(super) async fn end_tx(
    conn: &mut PgConnection,
    a: &mut AttemptRow,
    to: AttemptState,
    cause: EndCause,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    debug_assert!(to.is_terminal());
    void_challenge(conn, a).await?;
    *a = sqlx::query_as(
        "UPDATE pc_setup_attempts
            SET state = $2, reason = $3, qr_token_hash = NULL, qr_rotates_at = NULL,
                code_hash = NULL
          WHERE id = $1 AND state IN ('waiting', 'claimed', 'code', 'enrolled')
          RETURNING *",
    )
    .bind(a.id)
    .bind(to.as_str())
    .bind(cause.as_str())
    .fetch_one(&mut *conn)
    .await?;
    if let Some(enrollment) = a.enrollment_id {
        let ended = if to == AttemptState::Cancelled {
            "cancelled"
        } else {
            "expired"
        };
        sqlx::query(
            "UPDATE enrollments SET state = $2, ended_at = $3 WHERE id = $1 AND state = 'issued'",
        )
        .bind(enrollment)
        .bind(ended)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Cancel a live attempt (the PC's cancel, or a restart, Locking 6).
///
/// The one place that ends an attempt the requesting session gave up. Its
/// `issued` enrolment is cancelled with it ([`end_tx`]), which voids that
/// enrolment's open ceremonies and desktop flows: their finish and redeem
/// take the same locks and refuse a cancelled attempt. Terminal attempts
/// stay as they are; returns whether it ended one.
pub async fn cancel_attempt_tx(
    conn: &mut PgConnection,
    locked: &mut LockedAttempt,
    cause: EndCause,
) -> Result<bool, ApprovalError> {
    if locked.attempt.state().is_terminal() {
        return Ok(false);
    }
    end_tx(
        conn,
        &mut locked.attempt,
        AttemptState::Cancelled,
        cause,
        locked.now,
    )
    .await?;
    Ok(true)
}

/// Mint a new QR token if the attempt is `waiting` and its token is due (or
/// `force`, for a PC that lost it on a reload). Rotation pauses in every
/// other state. Returns the new token.
pub async fn rotate_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    a: &mut AttemptRow,
    now: DateTime<Utc>,
    force: bool,
) -> sqlx::Result<Option<Secret256>> {
    let due = a.qr_token_hash.is_none() || a.qr_rotates_at.is_none_or(|t| t <= now);
    if a.state() != AttemptState::Waiting || !(force || due) {
        return Ok(None);
    }
    let token = Secret256::random();
    *a = sqlx::query_as(
        "UPDATE pc_setup_attempts
            SET qr_token_hash = $2, qr_rotates_at = $3
          WHERE id = $1 AND state = 'waiting'
          RETURNING *",
    )
    .bind(a.id)
    .bind(token_hash(ctx, token.as_bytes()).to_vec())
    .bind(now + Duration::seconds(QR_ROTATE_SECS))
    .fetch_one(&mut *conn)
    .await?;
    Ok(Some(token))
}

/// A SQL predicate over `a` (a `pc_setup_attempts` row): a live attempt that
/// [`lock_attempt`] would change, on the database's clock at the statement.
/// A hint for polls and the sweep, which then take the locks (the locked
/// re-check decides), and the one read of another account's code.
macro_rules! needs_lock {
    () => {
        "a.state IN ('waiting', 'claimed', 'code', 'enrolled') AND (
       (a.state IN ('waiting', 'claimed') AND a.expires_at <= clock_timestamp())
    OR (a.state = 'code' AND (a.code_expires_at IS NULL OR a.code_expires_at <= clock_timestamp()))
    OR (a.state = 'enrolled' AND NOT EXISTS (
            SELECT 1 FROM enrollments e
             WHERE e.id = a.enrollment_id AND e.state = 'issued' AND e.expires_at > clock_timestamp()))
    OR (a.state = 'claimed' AND NOT EXISTS (
            SELECT 1 FROM approvals c
             WHERE c.id = a.challenge_id AND c.state = 'issued' AND c.expires_at > clock_timestamp()))
    OR NOT EXISTS (
            SELECT 1 FROM sessions s
             WHERE s.id = a.session_id AND s.user_id = a.user_id AND s.ended_at IS NULL
               AND s.expires_at > clock_timestamp() AND s.absolute_expires_at > clock_timestamp())
    OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id = a.user_id AND u.may_approve)
    OR EXISTS (SELECT 1 FROM global_bans gb
                WHERE gb.user_id = a.user_id
                  AND (gb.expires_at IS NULL OR gb.expires_at > clock_timestamp()))
    OR (a.device_id IS NOT NULL AND NOT EXISTS (
            SELECT 1 FROM approval_devices d WHERE d.id = a.device_id AND d.state = 'active'))
    OR (a.state = 'waiting' AND NOT EXISTS (
            SELECT 1 FROM approval_devices d WHERE d.user_id = a.user_id AND d.state = 'active')))"
    };
}

/// Whether attempt `id` needs [`lock_attempt`] (a lazy transition or, with
/// `rotation`, a QR token due).
async fn needs_lock(conn: &mut PgConnection, id: Uuid, rotation: bool) -> sqlx::Result<bool> {
    sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM pc_setup_attempts a WHERE a.id = $1 AND ((",
        needs_lock!(),
        ") OR ($2 AND a.state = 'waiting'
                 AND (a.qr_token_hash IS NULL OR a.qr_rotates_at IS NULL
                      OR a.qr_rotates_at <= clock_timestamp()))))"
    ))
    .bind(id)
    .bind(rotation)
    .fetch_one(&mut *conn)
    .await
}

// ============================================================================
// PC side
// ============================================================================

/// What [`start_tx`] (under the locks) and the security overview's
/// `can_set_up_this_pc` (without them) decide on: one predicate, so the
/// button shows exactly when a start would be accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Eligibility {
    pub probation_session: bool,
    pub banned: bool,
    pub may_approve: bool,
    pub active_key_device: bool,
}

impl Eligibility {
    #[must_use]
    pub const fn allows_start(self) -> bool {
        self.probation_session && !self.banned && self.may_approve && self.active_key_device
    }
}

/// The request's session if it is a real session with a probation snapshot
/// (a legacy request without one never starts an attempt).
#[must_use]
pub fn probation_sid(auth: &AuthUser) -> Option<Uuid> {
    match (auth.session_id, auth.session.as_ref()) {
        (Some(sid), Some(s)) if s.assurance == Assurance::Probation => Some(sid),
        _ => None,
    }
}

/// `can_set_up_this_pc`: whether [`start_tx`] would accept `auth` now (read
/// without locks; the start re-checks under them).
pub async fn can_start(conn: &mut PgConnection, auth: &AuthUser) -> sqlx::Result<bool> {
    if probation_sid(auth).is_none() {
        return Ok(false);
    }
    let (banned, may_approve): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM global_bans gb
                         WHERE gb.user_id = u.id
                           AND (gb.expires_at IS NULL OR gb.expires_at > clock_timestamp())),
                u.may_approve
           FROM users u WHERE u.id = $1",
    )
    .bind(auth.id)
    .fetch_optional(&mut *conn)
    .await?
    .unwrap_or((true, false));
    Ok(Eligibility {
        probation_session: true,
        banned,
        may_approve,
        active_key_device: has_active_device(conn, auth.id).await?,
    }
    .allows_start())
}

/// A new attempt and its first QR token.
#[derive(Debug)]
pub struct Started {
    pub attempt: AttemptRow,
    pub token: Secret256,
}

/// `start` (SM5 `– → waiting`).
///
/// Needs a live probation session of an account with an active key device
/// and `may_approve` on, else 403 `PC_SETUP_UNAVAILABLE`. A live attempt of
/// the same session is cancelled (`restarted`) in the same transaction
/// (Locking 6).
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn start_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
) -> Result<Started, ApprovalError> {
    let sid = probation_sid(auth).ok_or(ApprovalError::PcSetupUnavailable)?;
    let account = lock_accounts(conn, &[auth.id])
        .await?
        .remove(&auth.id)
        .ok_or(ApprovalError::PcSetupUnavailable)?;
    let session = lock_session_row(conn, auth.id, sid).await?;
    // The session's live attempt, locked too: every lock before the clock.
    let previous: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM pc_setup_attempts
          WHERE session_id = $1 AND state IN ('waiting', 'claimed', 'code', 'enrolled')",
    )
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let previous = match previous {
        Some(id) => Some(lock_attempt_rows(conn, id).await?),
        None => None,
    };
    let now = read_clock(conn).await?;
    let Some(session) = session.map(|s| s.at(now)).filter(|s| s.live) else {
        return Err(crate::auth::AuthError::InvalidToken.into());
    };
    let eligible = Eligibility {
        probation_session: Assurance::parse(&session.assurance) == Assurance::Probation,
        banned: account.banned,
        may_approve: account.may_approve,
        active_key_device: has_active_device(conn, auth.id).await?,
    };
    if !eligible.allows_start() {
        return Err(ApprovalError::PcSetupUnavailable);
    }
    // Bounds on starts per session and per account, counted under the
    // account lock. `Retry-After` is when the oldest counted start leaves the
    // window.
    let (in_session, in_account, oldest_session, oldest_account): (
        i64,
        i64,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    ) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE session_id = $2), COUNT(*),
                MIN(created_at) FILTER (WHERE session_id = $2), MIN(created_at)
           FROM pc_setup_attempts
          WHERE user_id = $1 AND created_at > $3",
    )
    .bind(auth.id)
    .bind(sid)
    .bind(now - Duration::minutes(super::START_WINDOW_MINUTES))
    .fetch_one(&mut *conn)
    .await?;
    let full = if in_session >= super::MAX_STARTS {
        Some(oldest_session)
    } else if in_account >= super::MAX_ACCOUNT_STARTS {
        Some(oldest_account)
    } else {
        None
    };
    if let Some(oldest) = full {
        let window = Duration::minutes(super::START_WINDOW_MINUTES);
        let free_at = oldest.map_or(now + window, |t| t + window);
        let secs = (free_at - now).num_seconds().clamp(1, window.num_seconds());
        return Err(crate::auth::AuthError::TooManyAttempts {
            retry_after: u64::try_from(secs).unwrap_or(600),
        }
        .into());
    }
    if let Some(previous) = previous {
        let mut old = previous.settle(conn, now).await?;
        cancel_attempt_tx(conn, &mut old, EndCause::Restarted).await?;
    }
    let token = Secret256::random();
    let attempt: AttemptRow = sqlx::query_as(
        "INSERT INTO pc_setup_attempts
            (id, user_id, session_id, state, qr_token_hash, qr_rotates_at, created_at, expires_at)
         VALUES ($1, $2, $3, 'waiting', $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(auth.id)
    .bind(sid)
    .bind(token_hash(ctx, token.as_bytes()).to_vec())
    .bind(now + Duration::seconds(QR_ROTATE_SECS))
    .bind(now)
    .bind(now + Duration::minutes(super::ATTEMPT_MINUTES))
    .fetch_one(&mut *conn)
    .await?;
    audit(conn, auth.id, "auth.pc_setup.start", attempt.id).await?;
    Ok(Started { attempt, token })
}

/// An audit row of the attempt (never a token or code).
pub(super) async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    attempt: Uuid,
) -> sqlx::Result<()> {
    crate::permissions::write_audit_log(
        &mut *conn,
        actor,
        action,
        Some("pc_setup_attempt"),
        Some(attempt),
        Some(serde_json::json!({ "attempt_id": attempt })),
        None,
    )
    .await?;
    Ok(())
}

/// The attempt `id` if it belongs to `auth`'s session, else 404 (another
/// session of the same account included). No lock.
pub async fn of_session(
    conn: &mut PgConnection,
    auth: &AuthUser,
    id: Uuid,
) -> Result<AttemptRow, ApprovalError> {
    let row = read(conn, id, false)
        .await?
        .ok_or(ApprovalError::NotFound)?;
    if row.user_id != auth.id || Some(row.session_id) != auth.session_id {
        return Err(ApprovalError::NotFound);
    }
    Ok(row)
}

/// The live attempt of `auth`'s session, if any. No lock.
pub async fn current_of_session(
    conn: &mut PgConnection,
    auth: &AuthUser,
) -> Result<Uuid, ApprovalError> {
    let sid = auth.session_id.ok_or(ApprovalError::NotFound)?;
    sqlx::query_scalar(
        "SELECT id FROM pc_setup_attempts
          WHERE session_id = $1 AND user_id = $2
            AND state IN ('waiting', 'claimed', 'code', 'enrolled')",
    )
    .bind(sid)
    .bind(auth.id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(ApprovalError::NotFound)
}

/// Lock the attempt for its requesting session and rotate its token (if due,
/// or `force`). Returns the attempt and a new token.
pub async fn refresh_for_pc_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
    id: Uuid,
    force: bool,
) -> Result<(AttemptRow, Option<Secret256>), ApprovalError> {
    of_session(conn, auth, id).await?;
    let mut locked = lock_attempt(conn, id).await?;
    let token = rotate_tx(conn, ctx, &mut locked.attempt, locked.now, force).await?;
    Ok((locked.attempt, token))
}

/// `DELETE`: the requesting session cancels its attempt.
pub async fn cancel_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    id: Uuid,
) -> Result<AttemptRow, ApprovalError> {
    of_session(conn, auth, id).await?;
    let mut locked = lock_attempt(conn, id).await?;
    cancel_attempt_tx(conn, &mut locked, EndCause::Cancelled).await?;
    Ok(locked.attempt)
}

/// One poll step: lock (in its own transaction) only when something is due.
pub async fn poll_step(
    db: &PgPool,
    ctx: &AuthCtx<'_>,
    id: Uuid,
    rotation: bool,
) -> Result<(AttemptRow, Option<Secret256>), ApprovalError> {
    let mut conn = db.acquire().await?;
    if needs_lock(&mut conn, id, rotation).await? {
        drop(conn);
        let mut tx = db.begin().await?;
        let mut locked = lock_attempt(&mut tx, id).await?;
        let token = if rotation {
            rotate_tx(&mut tx, ctx, &mut locked.attempt, locked.now, false).await?
        } else {
            None
        };
        tx.commit().await?;
        return Ok((locked.attempt, token));
    }
    let row = read(&mut conn, id, false)
        .await?
        .ok_or(ApprovalError::NotFound)?;
    Ok((row, None))
}

// ============================================================================
// Phone side
// ============================================================================

/// The description and place of the requesting session, as stored when it
/// was created (never from a request of the attempt).
pub(super) fn describe(s: &RequestingSession) -> (String, String) {
    let class = ClientClass::parse(&s.client_class).unwrap_or(ClientClass::Web);
    let description =
        crate::auth::login_attempts::core::client_label(class, s.user_agent.as_deref());
    let place: String = [s.city.as_deref(), s.country.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(MAX_PLACE_CHARS)
        .collect();
    (description, place)
}

/// The phone's key device and session under their locks, its liveness
/// left to the transition's one clock ([`PhoneLocks::live_at`]).
struct PhoneLocks {
    dev: KeyDeviceSession,
    expires_at: DateTime<Utc>,
    absolute_expires_at: DateTime<Utc>,
}

impl PhoneLocks {
    /// The session is live at `now`, else 401 `INVALID_TOKEN`.
    fn live_at(self, now: DateTime<Utc>) -> Result<KeyDeviceSession, ApprovalError> {
        if self.expires_at > now && self.absolute_expires_at > now {
            Ok(self.dev)
        } else {
            Err(crate::auth::AuthError::InvalidToken.into())
        }
    }
}

/// The checks of `key_device_session` for `pc_setup`, with the session's
/// expiry read by the locking statement and compared later, on the
/// transition's clock (the shared helper, used by every other kind, stays
/// as it is). Account lock (a no-op here), a banned account → 403, the
/// device row (`active`, the caller's, else 403 `KEY_DEVICE_REQUIRED`), the
/// session row (missing or ended → 401, not a full session of that device
/// → 403 `KEY_DEVICE_REQUIRED`).
async fn lock_phone(conn: &mut PgConnection, auth: &AuthUser) -> Result<PhoneLocks, ApprovalError> {
    let a = require_key_device_snapshot(auth)?;
    let sid = auth.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let locked = lock_accounts(conn, &[auth.id]).await?;
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
    #[allow(clippy::type_complexity)]
    let session: Option<(
        String,
        Option<String>,
        Option<Uuid>,
        DateTime<Utc>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        "SELECT assurance, authenticator_type, authenticator_id, expires_at, absolute_expires_at
               FROM sessions WHERE id = $1 AND user_id = $2 AND ended_at IS NULL
                FOR UPDATE",
    )
    .bind(sid)
    .bind(auth.id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((assurance, kind, authenticator_id, expires_at, absolute_expires_at)) = session else {
        return Err(crate::auth::AuthError::InvalidToken.into());
    };
    if assurance != "full"
        || kind.as_deref() != Some(crate::auth::strong::AuthenticatorKind::KeyDevice.as_str())
        || authenticator_id != Some(device.id)
    {
        return Err(ApprovalError::KeyDeviceRequired);
    }
    Ok(PhoneLocks {
        dev: KeyDeviceSession {
            auth: auth.clone(),
            device,
        },
        expires_at,
        absolute_expires_at,
    })
}

/// The scanning device, read without a lock before the token is looked at
/// (C26 order): not banned (403 `ACCOUNT_BANNED`), an active key device (403
/// `KEY_DEVICE_REQUIRED`), its session live (401 `INVALID_TOKEN`) and bound to
/// it, then the account's `may_approve` (403 `APPROVAL_NOT_ALLOWED`). The
/// own-account path re-checks all of it under the locks.
async fn scanner_ready(conn: &mut PgConnection, auth: &AuthUser) -> Result<(), ApprovalError> {
    let device = require_key_device_snapshot(auth)?;
    let sid = auth.session_id.ok_or(ApprovalError::KeyDeviceRequired)?;
    let row: Option<(bool, bool, bool, Option<bool>, bool)> = sqlx::query_as(
        "SELECT u.may_approve,
                EXISTS (SELECT 1 FROM global_bans gb
                         WHERE gb.user_id = u.id
                           AND (gb.expires_at IS NULL OR gb.expires_at > clock_timestamp())),
                EXISTS (SELECT 1 FROM approval_devices d
                         WHERE d.id = $2 AND d.user_id = u.id AND d.state = 'active'),
                (SELECT s.ended_at IS NULL AND s.expires_at > clock_timestamp()
                        AND s.absolute_expires_at > clock_timestamp()
                   FROM sessions s WHERE s.id = $3 AND s.user_id = u.id),
                EXISTS (SELECT 1 FROM sessions s
                         WHERE s.id = $3 AND s.user_id = u.id AND s.assurance = 'full'
                           AND s.authenticator_type = $4 AND s.authenticator_id = $2)
           FROM users u WHERE u.id = $1",
    )
    .bind(auth.id)
    .bind(device.id)
    .bind(sid)
    .bind(crate::auth::strong::AuthenticatorKind::KeyDevice.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((may_approve, banned, device_active, session_live, bound)) = row else {
        return Err(crate::auth::AuthError::InvalidToken.into());
    };
    if banned {
        return Err(ApprovalError::AccountBanned);
    }
    if !device_active {
        return Err(ApprovalError::KeyDeviceRequired);
    }
    if session_live != Some(true) {
        return Err(crate::auth::AuthError::InvalidToken.into());
    }
    if !bound {
        return Err(ApprovalError::KeyDeviceRequired);
    }
    if !may_approve {
        return Err(ApprovalError::NotAllowed);
    }
    Ok(())
}

/// `POST /api/pc-setups/claim`: the attempt by the token's keyed hash and a
/// `pc_setup` challenge (2 min, never past the attempt's 10 minutes) for the
/// scanning key device (SM5 `waiting → claimed`).
///
/// The scanner is checked first ([`scanner_ready`]); then unknown, rotated
/// or not live → 404 `LOGIN_CODE_STALE`; another account's live attempt →
/// 409 `PC_SETUP_OTHER_ACCOUNT` (no name, nothing changes).
#[tracing::instrument(skip_all, fields(user_id = %auth.id))]
pub async fn claim_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
    token: &str,
) -> Result<ApprovalChallenge, ApprovalError> {
    require_key_device_snapshot(auth)?;
    // The scanner first: its refusal is the same whatever the token (its
    // shape too), and nothing is written for it.
    scanner_ready(conn, auth).await?;
    let token = crate::auth::login_attempts::core::decode_token(token)
        .ok_or(ApprovalError::LoginCodeStale)?;
    let hash = token_hash(ctx, &token);
    // One read: the token's attempt and whether it is a live code a scan
    // could claim (state, timers, session, account, phone).
    let seen: Option<(Uuid, Uuid, bool)> = sqlx::query_as(concat!(
        "SELECT a.id, a.user_id,
                a.state IN ('waiting', 'claimed')
                AND (a.state = 'claimed' OR a.qr_rotates_at > clock_timestamp())
                AND NOT (",
        needs_lock!(),
        ") FROM pc_setup_attempts a WHERE a.qr_token_hash = $1"
    ))
    .bind(hash.to_vec())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, owner, live)) = seen else {
        return Err(ApprovalError::LoginCodeStale);
    };
    if owner != auth.id {
        // Another account's attempt: never locked or changed from here.
        return Err(if live {
            ApprovalError::PcSetupOtherAccount
        } else {
            ApprovalError::LoginCodeStale
        });
    }
    // Every lock (the attempt's, then the phone's device and session), then
    // the one clock.
    let rows = lock_attempt_rows(conn, id).await?;
    let phone = lock_phone(conn, auth).await?;
    let now = read_clock(conn).await?;
    let dev = phone.live_at(now)?;
    if !rows.account.may_approve {
        return Err(ApprovalError::NotAllowed);
    }
    let mut locked = rows.settle(conn, now).await?;
    // The attempt's state, also when a poll or the sweep ended it before
    // this request.
    if locked.end_reason() == Some(EndCause::MayApproveOff.as_str()) {
        return Err(ApprovalError::NotAllowed);
    }
    if locked.end_reason().is_some() {
        return Err(ApprovalError::LoginCodeStale);
    }
    let a = &locked.attempt;
    let current = a.qr_token_hash.as_deref().is_some_and(|h| ct_eq(h, &hash));
    if a.state() == AttemptState::Claimed && current {
        return Err(ApprovalError::LoginAttemptClaimed);
    }
    let fresh = a.qr_rotates_at.is_some_and(|t| t > now);
    if a.state() != AttemptState::Waiting || !current || !fresh {
        return Err(ApprovalError::LoginCodeStale);
    }
    let session = locked
        .session
        .as_ref()
        .ok_or(ApprovalError::LoginCodeStale)?;
    let (client_description, place) = describe(session);
    let context = CanonicalContext::PcSetup(PcSetupContext {
        attempt_id: a.id,
        sid_hash: sid_hash(a.session_id),
        client_description,
        session_created_at: session.created_at.timestamp(),
        place,
    });
    let ttl_end = (now + CHALLENGE_TTL).min(a.expires_at);
    // An attempt in its last seconds is as good as over: no challenge a
    // phone could answer fits (the challenge's end is whole seconds).
    if ttl_end - now < Duration::seconds(MIN_CHALLENGE_SECS) {
        return Err(ApprovalError::LoginCodeStale);
    }
    let challenge = issue_until(
        conn,
        ctx,
        NewChallenge {
            context,
            approver_id: auth.id,
            subject_id: auth.id,
            device_id: dev.device.device_id,
            requester: Requester::PcSetupAttempt(a.id),
            display: serde_json::json!({}),
            transfer_hash: Some(hash),
            ttl: CHALLENGE_TTL,
        },
        now,
        a.expires_at,
    )
    .await
    .map_err(|e| match e {
        ApprovalError::Expired => ApprovalError::LoginCodeStale,
        other => other,
    })?;
    locked.attempt = sqlx::query_as(
        "UPDATE pc_setup_attempts SET state = 'claimed', challenge_id = $2, device_id = $3
          WHERE id = $1 AND state = 'waiting'
          RETURNING *",
    )
    .bind(id)
    .bind(challenge.challenge_id)
    .bind(dev.device.id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(challenge)
}

/// A phone transition on a `pc_setup` challenge, under every lock.
pub struct PhoneLocked {
    pub locked: LockedAttempt,
    pub dev: KeyDeviceSession,
    pub challenge: ChallengeRow,
}

/// Lock a `pc_setup` challenge for its phone.
///
/// The attempt first ([`lock_attempt`]: account → requesting session →
/// attempt), then the key device and its session, then the challenge (bound
/// to this device's account and install id). Another account's or kind's challenge → 404; a
/// stale attempt → 403 `APPROVAL_NOT_ALLOWED` (`may_approve` off) or 409; a
/// challenge past due → 410 (its mark is kept by the caller's commit).
pub async fn lock_for_phone(
    conn: &mut PgConnection,
    auth: &AuthUser,
    challenge_id: Uuid,
) -> Result<PhoneLocked, ApprovalError> {
    // Anyone but the device the challenge was issued to learns nothing and
    // changes nothing (404 before any lock), a session without a key device
    // too.
    let caller = require_key_device_snapshot(auth).map_err(|_| ApprovalError::NotFound)?;
    let head: Option<(String, Uuid, String, Uuid, bool)> = sqlx::query_as(
        "SELECT a.kind, a.approver_id, a.requester_kind, a.requester_id,
                EXISTS (SELECT 1 FROM approval_devices d
                         WHERE d.id = $2 AND d.user_id = a.approver_id AND d.device_id = a.device_id)
           FROM approvals a WHERE a.id = $1",
    )
    .bind(challenge_id)
    .bind(caller.id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((kind, approver, requester_kind, attempt_id, own_device)) = head else {
        return Err(ApprovalError::NotFound);
    };
    if Kind::parse(&kind) != Some(Kind::PcSetup)
        || requester_kind != "pc_setup_attempt"
        || approver != auth.id
        || !own_device
    {
        return Err(ApprovalError::NotFound);
    }
    // Every lock (attempt, key device and its session, challenge), then the
    // one clock.
    let rows = lock_attempt_rows(conn, attempt_id).await?;
    // The device's own refusal still gives the attempt its lazy transitions
    // (a revoked phone ends it), on the clock read then.
    let phone = match lock_phone(conn, auth).await {
        Ok(phone) => phone,
        Err(e) if !matches!(e, ApprovalError::Database(_)) => {
            let now = read_clock(conn).await?;
            rows.settle(conn, now).await?;
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    let challenge = lock_challenge_row(
        conn,
        challenge_id,
        phone.dev.device.user_id,
        phone.dev.device.device_id,
    )
    .await?;
    let now = read_clock(conn).await?;
    let dev = match phone.live_at(now) {
        Ok(dev) => dev,
        Err(e) => {
            rows.settle(conn, now).await?;
            return Err(e);
        }
    };
    let locked = rows.settle(conn, now).await?;
    // The caller's live key-device session first (a banned account, a
    // revoked device, an ended session got their own answers), then the
    // attempt's end, also when a poll or the sweep ended it earlier.
    if !locked.account.may_approve || locked.end_reason() == Some(EndCause::MayApproveOff.as_str())
    {
        return Err(ApprovalError::NotAllowed);
    }
    if locked.end_reason().is_some() {
        return Err(ApprovalError::State);
    }
    check_for_answer_at(conn, &challenge, now).await?;
    let a = &locked.attempt;
    if a.state() != AttemptState::Claimed
        || a.challenge_id != Some(challenge.id)
        || a.device_id != Some(dev.device.id)
    {
        set_state(
            conn,
            challenge.id,
            &[ChallengeState::Issued],
            ChallengeState::Expired,
        )
        .await?;
        return Err(ApprovalError::State);
    }
    Ok(PhoneLocked {
        locked,
        dev,
        challenge,
    })
}

/// The result of a signed answer. `Debug` never shows the code.
pub enum Answered {
    /// `claimed → code`: the code goes to the phone only.
    Approved {
        attempt_id: Uuid,
        code: String,
        code_expires_at: DateTime<Utc>,
    },
    /// `claimed → reported`: the requesting session ended; deliver `events`.
    Reported { attempt_id: Uuid, events: Vec<Uuid> },
}

impl std::fmt::Debug for Answered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approved {
                attempt_id,
                code_expires_at,
                ..
            } => f
                .debug_struct("Approved")
                .field("attempt_id", attempt_id)
                .field("code_expires_at", code_expires_at)
                .finish_non_exhaustive(),
            Self::Reported { attempt_id, events } => f
                .debug_struct("Reported")
                .field("attempt_id", attempt_id)
                .field("events", events)
                .finish(),
        }
    }
}

/// Answer a `pc_setup` challenge with a UV signature (B.2: the response is
/// the scanned token, compared first; then the signature).
///
/// Approve consumes the challenge and moves the attempt to `code` with a
/// fresh 6-digit code (5 min, only its keyed hash stored). Report ends the
/// attempt and, in this transaction under the account lock and after the
/// challenge and attempt locks (Locking 5), the requesting session only. It
/// marks no credential.
#[tracing::instrument(skip_all, fields(user_id = %auth.id, challenge_id = %challenge_id))]
pub async fn answer_tx(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    auth: &AuthUser,
    challenge_id: Uuid,
    decision: Decision,
    response: &[u8],
    signature: &str,
) -> Result<Answered, ApprovalError> {
    let PhoneLocked {
        mut locked,
        dev,
        challenge,
    } = lock_for_phone(conn, auth, challenge_id).await?;
    let transfer = challenge
        .transfer_hash
        .as_deref()
        .ok_or(ApprovalError::NotFound)?;
    if !ct_eq(&token_hash(ctx, response), transfer) {
        return Err(ApprovalError::ResponseMismatch);
    }
    verify(
        &challenge,
        ctx.origin(),
        &dev.device.public_key,
        decision,
        response,
        signature,
    )?;
    let user = auth.id;
    let attempt_id = locked.attempt.id;
    // Every check is done: the effects follow.
    let out = match decision {
        Decision::Approve => {
            set_state(
                conn,
                challenge.id,
                &[ChallengeState::Issued],
                ChallengeState::Consumed,
            )
            .await?;
            let code = random_digits(CODE_DIGITS);
            let hash = keyed_hash(ctx.hmac_key(), SecretPurpose::PcSetupCode, code.as_bytes());
            let code_expires_at: DateTime<Utc> = sqlx::query_scalar(
                "UPDATE pc_setup_attempts
                    SET state = 'code', code_hash = $2, tries = 0, qr_token_hash = NULL,
                        qr_rotates_at = NULL, code_expires_at = $3
                  WHERE id = $1 AND state = 'claimed'
                  RETURNING code_expires_at",
            )
            .bind(attempt_id)
            .bind(hash.to_vec())
            .bind(locked.now + Duration::minutes(CODE_MINUTES))
            .fetch_one(&mut *conn)
            .await?;
            audit(conn, user, "auth.pc_setup.approve", attempt_id).await?;
            Answered::Approved {
                attempt_id,
                code,
                code_expires_at,
            }
        }
        Decision::Report => {
            set_state(
                conn,
                challenge.id,
                &[ChallengeState::Issued],
                ChallengeState::Reported,
            )
            .await?;
            end_tx(
                conn,
                &mut locked.attempt,
                AttemptState::Reported,
                EndCause::Reported,
                locked.now,
            )
            .await?;
            let sid = locked.attempt.session_id;
            end_sessions_tx(conn, ctx, user, &[sid], EndReason::Revoked).await?;
            let description = locked
                .session
                .as_ref()
                .map(|s| describe(s).0)
                .unwrap_or_default();
            let mut ev = NewSecurityEvent::new(
                user,
                EventType::PcSetupReported,
                Actor::User(user),
                // The requesting session: its sign-in carries "That wasn't me".
                serde_json::json!({
                    "attempt_id": attempt_id,
                    "requesting_sid": sid,
                    "client_description": description,
                }),
            );
            ev.authenticator = Some(AuthenticatorRef::key_device(dev.device.id));
            ev.session_id = dev.auth.session_id;
            let event = record(conn, ev).await?;
            audit(conn, user, "auth.pc_setup.report", attempt_id).await?;
            Answered::Reported {
                attempt_id,
                events: vec![event],
            }
        }
        Decision::Deny => {
            return Err(ApprovalError::Validation(
                "decision must be approve or report".into(),
            ))
        }
    };
    sqlx::query("UPDATE approval_devices SET last_used_at = $2 WHERE id = $1")
        .bind(dev.device.id)
        .bind(locked.now)
        .execute(&mut *conn)
        .await?;
    Ok(out)
}

/// The unsigned "No" (SM5 `claimed → refused`): the challenge is denied and
/// only the attempt ends.
#[tracing::instrument(skip_all, fields(user_id = %auth.id, challenge_id = %challenge_id))]
pub async fn refuse_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    challenge_id: Uuid,
) -> Result<(), ApprovalError> {
    let PhoneLocked {
        mut locked,
        challenge,
        ..
    } = lock_for_phone(conn, auth, challenge_id).await?;
    set_state(
        conn,
        challenge.id,
        &[ChallengeState::Issued],
        ChallengeState::Denied,
    )
    .await?;
    end_tx(
        conn,
        &mut locked.attempt,
        AttemptState::Refused,
        EndCause::Refused,
        locked.now,
    )
    .await?;
    audit(conn, auth.id, "auth.pc_setup.refuse", locked.attempt.id).await?;
    Ok(())
}

/// Back / dismissed (SM5 `claimed → waiting`, as SM1's release).
#[tracing::instrument(skip_all, fields(user_id = %auth.id, challenge_id = %challenge_id))]
pub async fn release_tx(
    conn: &mut PgConnection,
    auth: &AuthUser,
    challenge_id: Uuid,
) -> Result<(), ApprovalError> {
    let PhoneLocked { mut locked, .. } = lock_for_phone(conn, auth, challenge_id).await?;
    reopen_tx(conn, &mut locked.attempt).await?;
    Ok(())
}

/// The attempt `id` if `auth`'s key device scanned it, else 404. No lock.
pub async fn of_scanner(
    conn: &mut PgConnection,
    auth: &AuthUser,
    id: Uuid,
) -> Result<AttemptRow, ApprovalError> {
    let device = require_key_device_snapshot(auth)?;
    let row = read(conn, id, false)
        .await?
        .ok_or(ApprovalError::NotFound)?;
    if row.user_id != auth.id || row.device_id != Some(device.id) {
        return Err(ApprovalError::NotFound);
    }
    Ok(row)
}

// ============================================================================
// Sweep
// ============================================================================

/// The minute sweep (B.7 rule 3).
///
/// Every live attempt a transition would change goes through
/// [`lock_attempt`] in its own transaction, so the sweep takes the same locks
/// in the same order. Returns how many it ended.
pub async fn expire_due(db: &PgPool) -> Result<u64, ApprovalError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(concat!(
        "SELECT a.id FROM pc_setup_attempts a WHERE ",
        needs_lock!(),
        " ORDER BY a.id LIMIT 500"
    ))
    .fetch_all(db)
    .await?;
    let mut ended = 0;
    for id in ids {
        match sweep_one(db, id).await {
            Ok(true) => ended += 1,
            Ok(false) => {}
            // One row's failure never stops the others; the next minute retries.
            Err(e) => tracing::warn!(attempt_id = %id, error = %e, "pc setup sweep step failed"),
        }
    }
    Ok(ended)
}

/// One attempt of the sweep in its own transaction; whether it ended now.
async fn sweep_one(db: &PgPool, id: Uuid) -> Result<bool, ApprovalError> {
    let mut tx = db.begin().await?;
    let locked = match lock_attempt(&mut tx, id).await {
        Ok(locked) => locked,
        // Its account was deleted in between.
        Err(ApprovalError::NotFound) => return Ok(false),
        Err(e) => return Err(e),
    };
    tx.commit().await?;
    Ok(locked.ended_now.is_some())
}

/// The rotation deadline of a freshly minted token, for the wire.
#[must_use]
pub fn rotates_at(a: &AttemptRow) -> DateTime<Utc> {
    a.qr_rotates_at
        .unwrap_or_else(|| Utc::now() + Duration::seconds(QR_ROTATE_SECS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answered_debug_never_shows_the_code() {
        let a = Answered::Approved {
            attempt_id: Uuid::nil(),
            code: "493817".into(),
            code_expires_at: Utc::now(),
        };
        let shown = format!("{a:?}");
        assert!(!shown.contains("493817"), "{shown}");
        assert!(shown.contains("Approved"), "{shown}");
    }

    #[test]
    fn eligibility_needs_every_condition() {
        let ok = Eligibility {
            probation_session: true,
            banned: false,
            may_approve: true,
            active_key_device: true,
        };
        assert!(ok.allows_start());
        for e in [
            Eligibility {
                probation_session: false,
                ..ok
            },
            Eligibility { banned: true, ..ok },
            Eligibility {
                may_approve: false,
                ..ok
            },
            Eligibility {
                active_key_device: false,
                ..ok
            },
        ] {
            assert!(!e.allows_start(), "{e:?}");
        }
    }
}
