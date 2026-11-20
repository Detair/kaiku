//! SM5 from `code`: the code typed on the PC, the `pc_setup` enrolment (SM2)
//! it creates and the passkey that completes it (spec Step 5).
//!
//! **SECURITY CRITICAL.** Every function here runs on an attempt locked by
//! [`lock_attempt`](super::transition::lock_attempt) (account → requesting
//! session row → attempt) and locks the enrolment last (Locking 3), so a
//! concurrent session end or device revocation either commits first and is
//! seen here, or comes after and its cascade sees the new enrolment or the
//! `done` state. There is no setup token: the requesting session itself
//! registers the passkey, on the web through [`super::handlers`] and on the
//! desktop through its F1 flow (Task 4), both calling [`complete_enrolment`].

use rand::rngs::OsRng;
use rand::RngCore;
use sqlx::PgConnection;
use uuid::Uuid;

use super::transition::{audit, describe, end_tx, EndCause, LockedAttempt};
use super::{AttemptState, CODE_DIGITS, ENROLLMENT_MINUTES, MAX_CODE_TRIES};
use crate::approvals::error::ApprovalError;
use crate::approvals::secrets::{ct_eq, keyed_hash, SecretPurpose};
use crate::auth::ctx::AuthCtx;
use crate::auth::passkeys::queries::{PasskeyRow, PasskeyState};
use crate::auth::passkeys::register::{complete_recorded_with, RegistrationVia};
use crate::auth::passkeys::rp::VerifiedRegistration;
use crate::auth::passkeys::MAX_ACTIVE_PASSKEYS;
use crate::auth::AuthError;
use crate::security_events::{record, Actor, EventType, NewSecurityEvent};

/// Exactly one row changed. Under the attempt's locks the state predicate
/// always matches; no row means the caller's value was stale, an internal
/// error that rolls the whole transition back (never a committed refusal).
fn one_row(r: sqlx::postgres::PgQueryResult) -> Result<(), ApprovalError> {
    if r.rows_affected() == 1 {
        Ok(())
    } else {
        Err(ApprovalError::Database(sqlx::Error::RowNotFound))
    }
}

/// Where the right code was typed: the only way its passkey may be registered.
///
/// Spec Step 5: the code proves that whoever registers is with the person
/// holding the phone, so registration takes the same path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePath {
    /// The requesting session's own route (the page of the browser client).
    Web,
    /// The desktop flow with this id; its browser holds the browser secret.
    DesktopFlow(Uuid),
}

impl CodePath {
    const fn columns(self) -> (&'static str, Option<Uuid>) {
        match self {
            Self::Web => ("web", None),
            Self::DesktopFlow(id) => ("desktop_flow", Some(id)),
        }
    }

    /// Whether `a`'s right code came this way.
    fn matches(self, a: &super::transition::AttemptRow) -> bool {
        let (via, flow) = self.columns();
        a.code_via.as_deref() == Some(via) && a.code_flow_id == flow
    }
}

/// What a typed code did. A wrong code and the third wrong code are
/// outcomes, not errors: their effects (the try, the failed attempt, its
/// event) are committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeOutcome {
    /// `code → enrolled`: the enrolment the passkey completes.
    Enrolled { enrollment_id: Uuid },
    /// A wrong code; the attempt stays in `code`.
    Wrong { tries_left: i16 },
    /// The third wrong code: `code → failed` (`too_many_codes`), with
    /// `pc_setup_failed` recorded for delivery.
    Failed,
}

/// The refusal of an attempt that cannot take this step: past its code's
/// time → 410, any other ended or moved-on attempt → 409 (the PC reads the
/// reason from its poll).
fn not_live(locked: &LockedAttempt, want: AttemptState) -> Option<ApprovalError> {
    // Also when a poll or the sweep ended the attempt before this request.
    match locked.end_reason() {
        Some(r) if r == EndCause::CodeExpired.as_str() => Some(ApprovalError::Expired),
        Some(_) => Some(ApprovalError::State),
        None if locked.attempt.state() != want => Some(ApprovalError::State),
        None => None,
    }
}

/// Type the phone's code on the PC (SM5 `code → enrolled | failed`).
///
/// The right code creates the `pc_setup` enrolment (10 minutes, recording
/// the requesting session and the approving key device, no setup token) in
/// this transaction and moves the attempt to `enrolled`; the code is spent.
/// A wrong one spends a try; the third ends the attempt. A malformed code is
/// refused (400) without spending one.
#[tracing::instrument(skip_all, fields(attempt_id = %locked.attempt.id))]
pub async fn enter_code(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    locked: &LockedAttempt,
    code: &str,
    path: CodePath,
) -> Result<CodeOutcome, ApprovalError> {
    if let Some(e) = not_live(locked, AttemptState::Code) {
        return Err(e);
    }
    if code.len() != CODE_DIGITS || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApprovalError::Validation("code must be 6 digits".into()));
    }
    let a = &locked.attempt;
    let (Some(stored), Some(device)) = (a.code_hash.as_deref(), a.device_id) else {
        return Err(ApprovalError::State);
    };
    let typed = keyed_hash(ctx.hmac_key(), SecretPurpose::PcSetupCode, code.as_bytes());
    // Every check is done: the effects follow.
    if ct_eq(&typed, stored) {
        let enrollment_id = Uuid::new_v4();
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        sqlx::query(
            "INSERT INTO enrollments (id, variant, subject_id, issuer_kind, issuer_session_id,
                 issuer_device_id, setup_token_hash, nonce, authenticator_type, state,
                 created_at, expires_at)
             VALUES ($1, 'pc_setup', $2, 'pc_setup', $3, $4, NULL, $5, 'passkey', 'issued',
                     NOW(), NOW() + make_interval(mins => $6))",
        )
        .bind(enrollment_id)
        .bind(a.user_id)
        .bind(a.session_id)
        .bind(device)
        .bind(nonce.to_vec())
        .bind(i32::try_from(ENROLLMENT_MINUTES).unwrap_or(10))
        .execute(&mut *conn)
        .await?;
        let (via, flow) = path.columns();
        sqlx::query(
            "UPDATE pc_setup_attempts
                SET state = 'enrolled', enrollment_id = $2, code_hash = NULL,
                    code_via = $3, code_flow_id = $4
              WHERE id = $1 AND state = 'code'",
        )
        .bind(a.id)
        .bind(enrollment_id)
        .bind(via)
        .bind(flow)
        .execute(&mut *conn)
        .await
        .map(one_row)??;
        audit(conn, a.user_id, "auth.pc_setup.enrol", a.id).await?;
        return Ok(CodeOutcome::Enrolled { enrollment_id });
    }
    let tries = a.tries + 1;
    if tries < MAX_CODE_TRIES {
        sqlx::query("UPDATE pc_setup_attempts SET tries = $2 WHERE id = $1 AND state = 'code'")
            .bind(a.id)
            .bind(tries)
            .execute(&mut *conn)
            .await
            .map(one_row)??;
        return Ok(CodeOutcome::Wrong {
            tries_left: MAX_CODE_TRIES - tries,
        });
    }
    let mut ended = a.clone();
    sqlx::query(
        "UPDATE pc_setup_attempts SET tries = $2, code_hash = NULL WHERE id = $1 AND state = 'code'",
    )
    .bind(a.id)
    .bind(tries)
    .execute(&mut *conn)
    .await
    .map(one_row)??;
    end_tx(
        conn,
        &mut ended,
        AttemptState::Failed,
        EndCause::TooManyCodes,
    )
    .await?;
    // The account only (normal severity, B.5 coalescing).
    let mut ev = NewSecurityEvent::new(
        a.user_id,
        EventType::PcSetupFailed,
        Actor::User(a.user_id),
        serde_json::json!({ "attempt_id": a.id }),
    );
    ev.session_id = Some(a.session_id);
    record(conn, ev).await?;
    Ok(CodeOutcome::Failed)
}

/// The attempt's live enrolment, locked after the attempt (Locking 3).
/// Anything but the attempt's own `issued`, unexpired `pc_setup` enrolment
/// of the requesting session → 409.
async fn lock_enrolment(
    conn: &mut PgConnection,
    locked: &LockedAttempt,
) -> Result<Uuid, ApprovalError> {
    let a = &locked.attempt;
    let id = a.enrollment_id.ok_or(ApprovalError::State)?;
    let live: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM enrollments
          WHERE id = $1 AND variant = 'pc_setup' AND subject_id = $2 AND issuer_session_id = $3
            AND state = 'issued' AND expires_at > clock_timestamp()
            FOR UPDATE",
    )
    .bind(id)
    .bind(a.user_id)
    .bind(a.session_id)
    .fetch_optional(&mut *conn)
    .await?;
    live.ok_or(ApprovalError::State)
}

/// Whether the account can take one more active passkey (409
/// `PASSKEY_LIMIT` otherwise). Read before the ceremony starts, and again
/// in [`complete_enrolment`] under the account lock.
async fn passkey_room(conn: &mut PgConnection, user_id: Uuid) -> Result<(), ApprovalError> {
    let active = crate::auth::passkeys::queries::active_count(&mut *conn, user_id).await?;
    if active >= i64::try_from(MAX_ACTIVE_PASSKEYS).unwrap_or(10) {
        return Err(AuthError::PasskeyLimit.into());
    }
    Ok(())
}

/// The checks of `passkey/start` under the locks; returns the enrolment id.
///
/// The attempt is `enrolled` through `path` (the code was typed the way the
/// passkey is now registered), its enrolment live and the account has room.
/// The ceremony is bound to the returned enrolment.
pub async fn registration_open(
    conn: &mut PgConnection,
    locked: &LockedAttempt,
    path: CodePath,
) -> Result<Uuid, ApprovalError> {
    if let Some(e) = not_live(locked, AttemptState::Enrolled) {
        return Err(e);
    }
    if !path.matches(&locked.attempt) {
        return Err(ApprovalError::State);
    }
    let enrollment_id = lock_enrolment(conn, locked).await?;
    passkey_room(conn, locked.attempt.user_id).await?;
    Ok(enrollment_id)
}

/// A verified registration and the label to store it under.
pub struct Credential<'a> {
    pub verified: &'a VerifiedRegistration,
    pub label: &'a str,
}

/// Complete the attempt with its passkey (SM5 `enrolled → completed`, SM2
/// `issued → done`).
///
/// Re-checks under the locks that the attempt is `enrolled` and its
/// enrolment live, and that the credential is new and the account has room
/// (409 `PASSKEY_ALREADY_REGISTERED` / `PASSKEY_LIMIT`, before any effect:
/// the attempt stays `enrolled`, so the PC can show the error and leave and
/// the phone never says "Done"). Then the `active` passkey with the
/// enrolment's id, the enrolment `done`, the attempt `completed`, and
/// `authenticator_added` (high) with `via: pc_setup`, the requesting `sid`
/// ("That wasn't me" ends it), the PC's description and the approving
/// phone. The event is delivered after commit by the caller (it is in the
/// outbox, `security_events::notices::spawn_pending_delivery`).
#[tracing::instrument(skip_all, fields(attempt_id = %locked.attempt.id))]
pub async fn complete_enrolment(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    locked: &LockedAttempt,
    path: CodePath,
    credential: Credential<'_>,
) -> Result<PasskeyRow, ApprovalError> {
    let enrollment_id = registration_open(conn, locked, path).await?;
    let a = &locked.attempt;
    let known: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM passkeys WHERE credential_id = $1 AND state <> 'revoked')",
    )
    .bind(credential.verified.passkey.cred_id().as_ref().to_vec())
    .fetch_one(&mut *conn)
    .await?;
    if known {
        return Err(AuthError::PasskeyAlreadyRegistered.into());
    }
    let (device_id, device_label): (Uuid, String) =
        sqlx::query_as("SELECT id, label FROM approval_devices WHERE id = $1")
            .bind(a.device_id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(ApprovalError::State)?;
    let client_description = locked
        .session
        .as_ref()
        .map(|s| describe(s).0)
        .unwrap_or_default();
    let mut details = serde_json::Map::new();
    details.insert("attempt_id".into(), serde_json::json!(a.id));
    details.insert("requesting_sid".into(), serde_json::json!(a.session_id));
    details.insert(
        "client_description".into(),
        serde_json::json!(client_description),
    );
    details.insert("approving_device_id".into(), serde_json::json!(device_id));
    details.insert(
        "approving_device_label".into(),
        serde_json::json!(device_label),
    );
    // Every check is done: the effects follow.
    let (row, _event) = complete_recorded_with(
        conn,
        a.user_id,
        credential.label,
        credential.verified,
        PasskeyState::Active,
        None,
        RegistrationVia::PcSetup,
        Actor::User(a.user_id),
        details,
    )
    .await?;
    sqlx::query("UPDATE passkeys SET enrollment_id = $2 WHERE id = $1")
        .bind(row.id)
        .bind(enrollment_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE enrollments SET state = 'done', done_at = NOW(), ended_at = NOW()
          WHERE id = $1 AND state = 'issued'",
    )
    .bind(enrollment_id)
    .execute(&mut *conn)
    .await
    .map(one_row)??;
    sqlx::query(
        "UPDATE pc_setup_attempts SET state = 'completed' WHERE id = $1 AND state = 'enrolled'",
    )
    .bind(a.id)
    .execute(&mut *conn)
    .await
    .map(one_row)??;
    // A strong authenticator became active (as an SM2 `done`).
    crate::auth::strong::mark_first_activity(conn, a.user_id).await?;
    crate::auth::account_requests::core::on_strong_activated(conn, ctx, a.user_id, None).await?;
    audit(conn, a.user_id, "auth.pc_setup.complete", a.id).await?;
    Ok(row)
}
