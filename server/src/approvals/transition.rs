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
///
/// `FOR NO KEY UPDATE`: transitions serialise on it (and on deletes), but
/// it does not conflict with the `FOR KEY SHARE` locks that inserts
/// referencing `users` take (notices, messages, events), so a notice
/// delivery that inserts rows for several accounts cannot deadlock with a
/// transition holding two account locks (found in L1b Task 21).
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
          FOR NO KEY UPDATE",
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
    lock_scope_with(conn, scope, |c, a| Box::pin(accounts_recording(c, a))).await
}

/// [`lock_scope`] with the reader of the recorded set given (a seam for
/// tests; production uses [`accounts_recording`]).
pub async fn lock_scope_with<F>(
    conn: &mut PgConnection,
    scope: &LockScope,
    mut accounts_recording: F,
) -> Result<BTreeMap<Uuid, LockedAccount>, TransitionError>
where
    F: for<'c> FnMut(
        &'c mut PgConnection,
        &'c [AffectedRoute],
    ) -> futures::future::BoxFuture<'c, sqlx::Result<Vec<Uuid>>>,
{
    let mut recording = accounts_recording(conn, &scope.affected).await?;
    // Each attempt runs after a savepoint: a retry first rolls back to it,
    // which releases the attempt's row locks, so the grown set is again
    // locked in one statement in ascending order (never a lower account
    // while a higher one is held). Locks held before the call stay.
    sqlx::query("SAVEPOINT kaiku_lock_scope")
        .execute(&mut *conn)
        .await?;
    for _ in 0..MAX_SCOPE_ATTEMPTS {
        let mut ids = vec![scope.subject];
        ids.extend(scope.actors.iter().copied());
        ids.extend(recording.iter().copied());
        let locked = lock_accounts(conn, &ids).await?;
        let again = accounts_recording(conn, &scope.affected).await?;
        if again.iter().all(|id| locked.contains_key(id)) {
            sqlx::query("RELEASE SAVEPOINT kaiku_lock_scope")
                .execute(&mut *conn)
                .await?;
            return Ok(locked);
        }
        sqlx::query("ROLLBACK TO SAVEPOINT kaiku_lock_scope")
            .execute(&mut *conn)
            .await?;
        recording = again;
    }
    sqlx::query("RELEASE SAVEPOINT kaiku_lock_scope")
        .execute(&mut *conn)
        .await?;
    Err(TransitionError::Contended)
}

/// Lock a change of `user`'s routes (an authenticator removed, the role gone).
///
/// `user` and `actors` are locked together with every account whose open
/// requests record `user` as admin or contact or one of `authenticators`,
/// in one ascending statement, re-read under the locks and retried if the
/// set grew (B.7 cross-account serialisation, Codex M7). Call it first in
/// the transaction.
pub async fn lock_route_change(
    conn: &mut PgConnection,
    user: Uuid,
    actors: &[Uuid],
    authenticators: &[AuthenticatorRef],
) -> Result<BTreeMap<Uuid, LockedAccount>, TransitionError> {
    let mut affected = vec![AffectedRoute::Admin(user), AffectedRoute::Contact(user)];
    affected.extend(
        authenticators
            .iter()
            .copied()
            .map(AffectedRoute::Authenticator),
    );
    lock_scope(
        conn,
        &LockScope {
            subject: user,
            actors: actors.to_vec(),
            affected,
        },
    )
    .await
}

/// Lock a request start with every account that could cancel it.
///
/// The subject (and `actors`) together with each admin and the subject's
/// contact, re-read
/// under the locks and retried if that set changed (Codex M7). Call it
/// first in the transaction.
pub async fn lock_for_start(
    conn: &mut PgConnection,
    subject: Uuid,
    actors: &[Uuid],
) -> Result<BTreeMap<Uuid, LockedAccount>, TransitionError> {
    lock_scope_with(
        conn,
        &LockScope {
            subject,
            actors: actors.to_vec(),
            affected: Vec::new(),
        },
        move |c, _| Box::pin(potential_cancellers(c, subject)),
    )
    .await
}

pub async fn potential_cancellers(
    conn: &mut PgConnection,
    subject: Uuid,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT user_id FROM system_admins
         UNION SELECT recovery_contact_id FROM users
                WHERE id = $1 AND recovery_contact_id IS NOT NULL",
    )
    .bind(subject)
    .fetch_all(&mut *conn)
    .await
}

/// The accounts whose open account requests record one of `affected` as an
/// independent canceller (GIN containment on `cancellers`).
pub async fn accounts_recording(
    conn: &mut PgConnection,
    affected: &[AffectedRoute],
) -> sqlx::Result<Vec<Uuid>> {
    let mut out = Vec::new();
    for a in affected {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT user_id FROM account_requests
              WHERE state IN ('waiting', 'ready') AND cancellers @> $1",
        )
        .bind(crate::auth::account_requests::core::CancellerRoute::containment(a))
        .fetch_all(&mut *conn)
        .await?;
        out.extend(ids);
        // Codex r2 M7: also the `admin` requests the admin (or the admin's
        // device) started; a demotion or revocation voids them.
        let started: Vec<Uuid> = match a {
            AffectedRoute::Admin(admin) => {
                sqlx::query_scalar(
                    "SELECT DISTINCT user_id FROM account_requests
                      WHERE state IN ('waiting', 'ready') AND kind = 'admin' AND started_by = $1",
                )
                .bind(admin)
                .fetch_all(&mut *conn)
                .await?
            }
            AffectedRoute::Authenticator(r) => {
                sqlx::query_scalar(
                    "SELECT DISTINCT user_id FROM account_requests
                      WHERE state IN ('waiting', 'ready') AND kind = 'admin'
                        AND started_by_device = $1",
                )
                .bind(r.id)
                .fetch_all(&mut *conn)
                .await?
            }
            AffectedRoute::Contact(_) => Vec::new(),
        };
        out.extend(started);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}
