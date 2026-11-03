//! B.7 rule 1: one transaction per transition, serialised per account.
//!
//! A transition locks, in **one statement and in ascending id order**, the
//! `users` rows of every account it changes or takes authority from: the
//! subject, the acting admin or contact, and the accounts whose open
//! requests record an affected canceller (read that set, lock, re-check,
//! retry if it changed). Under those locks it re-checks every precondition
//! (account state, roles, `may_approve`, the authorising session, grant,
//! authenticator and factor generation) and then applies its effect. Two
//! transitions that depend on each other share a lock, so the first to
//! commit wins; ascending order means they never deadlock.

use std::collections::BTreeMap;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::strong::AuthenticatorRef;

/// An account as it is under its row lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct LockedAccount {
    pub id: Uuid,
    /// False for an id that names no account (nothing was locked for it).
    pub exists: bool,
    pub banned: bool,
    pub is_system: bool,
    pub is_admin: bool,
    pub may_approve: bool,
}

/// Lock the accounts `ids` (deduplicated) in ascending id order with ONE
/// statement, then read their bans and roles under the locks.
#[tracing::instrument(skip(conn))]
pub async fn lock_accounts(
    conn: &mut PgConnection,
    ids: &[Uuid],
) -> sqlx::Result<BTreeMap<Uuid, LockedAccount>> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let rows: Vec<(Uuid, bool, bool)> = sqlx::query_as(
        "SELECT id, is_system, may_approve FROM users
          WHERE id = ANY($1)
          ORDER BY id
          FOR UPDATE",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    let state: Vec<(Uuid, bool, bool)> = sqlx::query_as(
        "SELECT u.id,
                EXISTS (SELECT 1 FROM global_bans gb
                         WHERE gb.user_id = u.id
                           AND (gb.expires_at IS NULL OR gb.expires_at > NOW())),
                EXISTS (SELECT 1 FROM system_admins sa WHERE sa.user_id = u.id)
           FROM users u WHERE u.id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *conn)
    .await?;
    let mut out: BTreeMap<Uuid, LockedAccount> = ids
        .iter()
        .map(|&id| {
            (
                id,
                LockedAccount {
                    id,
                    exists: false,
                    banned: false,
                    is_system: false,
                    is_admin: false,
                    may_approve: false,
                },
            )
        })
        .collect();
    for (id, is_system, may_approve) in rows {
        if let Some(a) = out.get_mut(&id) {
            a.exists = true;
            a.is_system = is_system;
            a.may_approve = may_approve;
        }
    }
    for (id, banned, is_admin) in state {
        if let Some(a) = out.get_mut(&id) {
            a.banned = banned;
            a.is_admin = is_admin;
        }
    }
    Ok(out)
}

/// A route by which someone can cancel an account request; a transition
/// that changes one must also lock the accounts whose requests record it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffectedRoute {
    /// An admin's role or strong authenticators.
    Admin(Uuid),
    /// A recovery contact's link or strong authenticators.
    Contact(Uuid),
    /// One strong authenticator.
    Authenticator(AuthenticatorRef),
}

/// What a transition locks.
#[derive(Debug, Clone, Default)]
pub struct LockScope {
    pub subject: Uuid,
    pub actors: Vec<Uuid>,
    pub affected: Vec<AffectedRoute>,
}

/// Why a transition did not happen.
#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    /// A precondition no longer holds under the lock (the caller maps it to
    /// its own error).
    #[error("precondition failed: {0}")]
    Precondition(&'static str),
    /// The set of accounts to lock kept changing (retry later; 503).
    #[error("too much contention, try again")]
    Contended,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// How often [`lock_scope`] re-reads the recorded set before giving up.
const MAX_SCOPE_ATTEMPTS: usize = 3;

/// Lock subject ∪ actors ∪ the accounts whose open requests record an
/// affected route; re-read that set under the locks and retry (≤ 3) if it
/// changed, else [`TransitionError::Contended`].
pub async fn lock_scope(
    conn: &mut PgConnection,
    scope: &LockScope,
) -> Result<BTreeMap<Uuid, LockedAccount>, TransitionError> {
    let mut recording = accounts_recording(conn, &scope.affected).await?;
    for _ in 0..MAX_SCOPE_ATTEMPTS {
        let mut ids = vec![scope.subject];
        ids.extend(scope.actors.iter().copied());
        ids.extend(recording.iter().copied());
        let locked = lock_accounts(conn, &ids).await?;
        let again = accounts_recording(conn, &scope.affected).await?;
        if again.iter().all(|id| locked.contains_key(id)) {
            return Ok(locked);
        }
        recording = again;
    }
    Err(TransitionError::Contended)
}

/// The accounts whose open account requests record one of `affected` as an
/// independent canceller. L1a has no account requests yet: always empty
/// (L1b Task 21 implements it).
pub async fn accounts_recording(
    _conn: &mut PgConnection,
    _affected: &[AffectedRoute],
) -> sqlx::Result<Vec<Uuid>> {
    Ok(Vec::new())
}
