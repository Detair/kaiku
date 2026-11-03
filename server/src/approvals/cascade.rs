//! B.7 rule 2: invalidation cascades, each in the transaction of its cause.
//!
//! - ending a session voids its step-up grants and the enrolments it issued (Tasks 9 and 11 fill
//!   [`on_sessions_ended`]);
//! - revoking an authenticator ends its sessions and voids its open challenges, the grants it
//!   produced and the enrolments it issued ([`on_authenticator_revoked`]; L1b adds requests).
//!
//! The session cascade deliberately leaves challenges alone: a `device_stepup` challenge is
//! answerable only from its requesting session (re-read live under the account lock), and an
//! `enroll`/`enroll_pop` effect re-checks its enrolment's state under the lock. Paths that end
//! sessions without the account lock (logout, refresh) therefore never lock challenge rows.

use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::ctx::AuthCtx;
use crate::auth::revocation::{end_sessions_tx, EndReason};
use crate::auth::strong::AuthenticatorRef;
use crate::auth::AuthResult;

/// What a revocation cascade voided (later tasks add their objects).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CascadeOutcome {
    pub ended_sessions: Vec<Uuid>,
}

/// The cascade of an ended factor generation (B.7 rule 2).
///
/// A password, TOTP secret or recovery-code set was replaced, removed or
/// marked compromised: login attempts (Task 17) and account requests
/// (Task 21) that used `used` end here.
#[tracing::instrument(skip_all)]
pub async fn on_generation_ended(
    _conn: &mut PgConnection,
    _ctx: &AuthCtx<'_>,
    _user_id: Uuid,
    _used: crate::auth::factors::FactorRef,
) -> AuthResult<()> {
    Ok(())
}

/// The cascade of ended sessions (called by `end_sessions_tx`).
pub async fn on_sessions_ended(
    conn: &mut PgConnection,
    _ctx: &AuthCtx<'_>,
    sids: &[Uuid],
) -> AuthResult<()> {
    if sids.is_empty() {
        return Ok(());
    }
    crate::auth::step_up::void_for_sessions(conn, sids).await?;
    super::enrollments::expire_for_sessions(conn, sids).await?;
    Ok(())
}

/// The cascade of a revoked authenticator of `user_id`: every live session
/// it created (or that is its own device session) ends with
/// `authenticator_revoked`.
#[tracing::instrument(skip(conn, ctx))]
pub async fn on_authenticator_revoked(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    a: AuthenticatorRef,
) -> AuthResult<CascadeOutcome> {
    let sids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sessions
          WHERE user_id = $1 AND authenticator_type = $2 AND authenticator_id = $3
            AND ended_at IS NULL",
    )
    .bind(user_id)
    .bind(a.as_str())
    .bind(a.id)
    .fetch_all(&mut *conn)
    .await?;
    let ended_sessions =
        end_sessions_tx(conn, ctx, user_id, &sids, EndReason::AuthenticatorRevoked).await?;
    crate::auth::step_up::void_for_authenticator(conn, a).await?;
    if a.kind == crate::auth::strong::AuthenticatorKind::KeyDevice {
        super::enrollments::expire_for_device(conn, a.id).await?;
        // Its open challenges (they name the device by its install id).
        let install: Option<Uuid> =
            sqlx::query_scalar("SELECT device_id FROM approval_devices WHERE id = $1")
                .bind(a.id)
                .fetch_optional(&mut *conn)
                .await?;
        if let Some(install) = install {
            super::core::void_for_device(conn, user_id, install).await?;
        }
    }
    Ok(CascadeOutcome { ended_sessions })
}
