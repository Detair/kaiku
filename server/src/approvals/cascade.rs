//! B.7 rule 2: invalidation cascades, each in the transaction of its cause.
//!
//! - ending a session voids its step-up grants and the enrolments it issued (Tasks 9 and 11 fill
//!   [`on_sessions_ended`]);
//! - revoking an authenticator ends its sessions and voids its open challenges, the grants it
//!   produced and the enrolments it issued ([`on_authenticator_revoked`]; L1b adds requests); a key
//!   device also ends the sessions it confirmed (QR, password or OIDC + phone confirmation) and the
//!   sessions whose confirming device is unknown (from before the approver was recorded);
//! - ending a factor generation ends the sessions that signed in with it, then expires the login
//!   attempts and voids the account requests that used it ([`on_generation_ended`]).
//!
//! Lock order (B.7): the callers hold the account lock; then sessions (rows ended here), then
//! grants and enrolments (the session cascade), then challenges, attempts and requests. Every
//! other path that writes these sessions either holds the same account lock (sign-in, collect,
//! the other revocations and factor changes, a logout of more than one session) or touches a
//! single session row and nothing that is locked here before sessions (refresh, a logout of one
//! session).
//!
//! The session cascade deliberately leaves challenges alone: a `device_stepup` challenge is
//! answerable only from its requesting session (re-read live under the account lock), and an
//! `enroll`/`enroll_pop` effect re-checks its enrolment's state under the lock. Paths that end
//! sessions without the account lock (refresh, a logout of one session) therefore never lock
//! challenge rows.

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
/// marked compromised: every live session of the user that signed in with
/// that password or TOTP generation ends (`factor_ended`), except `keep`
/// (the acting session of the owner rule's part 2, which has just proved a
/// fresh factor); then login attempts (Task 17) and account requests
/// (Task 21) that used `used` end.
#[tracing::instrument(skip(conn, ctx))]
pub async fn on_generation_ended(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    used: crate::auth::factors::FactorRef,
    keep: Option<Uuid>,
) -> AuthResult<CascadeOutcome> {
    use crate::auth::factors::Factor;
    let sql = match used.factor {
        Factor::Password => Some(
            "SELECT id FROM sessions
              WHERE user_id = $1 AND ended_at IS NULL AND password_generation = $2
                AND ($3::uuid IS NULL OR id <> $3)",
        ),
        Factor::Totp => Some(
            "SELECT id FROM sessions
              WHERE user_id = $1 AND ended_at IS NULL AND totp_generation = $2
                AND ($3::uuid IS NULL OR id <> $3)",
        ),
        // Recovery codes never sign in.
        Factor::RecoveryCodes => None,
    };
    let ended_sessions = match sql {
        Some(sql) => {
            let sids: Vec<Uuid> = sqlx::query_scalar(sql)
                .bind(user_id)
                .bind(used.generation)
                .bind(keep)
                .fetch_all(&mut *conn)
                .await?;
            end_sessions_tx(conn, ctx, user_id, &sids, EndReason::FactorEnded).await?
        }
        None => Vec::new(),
    };
    crate::auth::login_attempts::core::on_generation_ended(conn, user_id, used).await?;
    crate::auth::account_requests::core::on_generation_ended(conn, ctx, user_id, used).await?;
    Ok(CascadeOutcome { ended_sessions })
}

/// B.7 rule 2: the account's recovery contact changed or was removed; open
/// requests that recorded the old contact lose that route (Task 21).
#[tracing::instrument(skip_all)]
pub async fn on_contact_changed(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    account: Uuid,
) -> AuthResult<()> {
    crate::auth::account_requests::core::recheck_routes(conn, ctx, None, Some(account)).await
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

/// The cascade of a revoked authenticator of `user_id`.
///
/// Every live session it created (or that is its own device session) ends
/// with `authenticator_revoked`. A key device also ends every live session it
/// confirmed (`approved_by_id`) and, if it had ever been active
/// (`activated_at` set), every live session confirmed by an unknown key
/// device of the account (`approved_by_type` set, id `NULL`: sessions from
/// before the approver was recorded), whatever the reason of the
/// revocation. A device that was only ever pending confirmed nothing.
#[tracing::instrument(skip(conn, ctx))]
pub async fn on_authenticator_revoked(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    a: AuthenticatorRef,
) -> AuthResult<CascadeOutcome> {
    let sids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM sessions
          WHERE user_id = $1 AND ended_at IS NULL
            AND ((authenticator_type = $2 AND authenticator_id = $3)
                 OR ($2 = 'key_device' AND approved_by_type = 'key_device'
                     AND (approved_by_id = $3
                          OR (approved_by_id IS NULL
                              AND EXISTS (SELECT 1 FROM approval_devices d
                                           WHERE d.id = $3 AND d.activated_at IS NOT NULL)))))",
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
    crate::auth::account_requests::core::on_authenticator_revoked(conn, ctx, user_id, a).await?;
    Ok(CascadeOutcome { ended_sessions })
}
