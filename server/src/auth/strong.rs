//! Strong authenticators (spec B.0): active passkeys and active key devices.
//!
//! Key devices live in `approval_devices`; LP adds the passkey arms of
//! every function here.

use uuid::Uuid;

/// The kind of a strong authenticator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthenticatorKind {
    Passkey,
    KeyDevice,
}

impl AuthenticatorKind {
    /// The stored and wire form (`authenticator_type`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::KeyDevice => "key_device",
        }
    }

    /// Parse the stored form.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "passkey" => Some(Self::Passkey),
            "key_device" => Some(Self::KeyDevice),
            _ => None,
        }
    }
}

/// One passkey or key device, by row id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuthenticatorRef {
    pub kind: AuthenticatorKind,
    pub id: Uuid,
}

impl AuthenticatorRef {
    #[must_use]
    pub const fn passkey(id: Uuid) -> Self {
        Self {
            kind: AuthenticatorKind::Passkey,
            id,
        }
    }

    #[must_use]
    pub const fn key_device(id: Uuid) -> Self {
        Self {
            kind: AuthenticatorKind::KeyDevice,
            id,
        }
    }

    /// The stored and wire form of the kind.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.kind.as_str()
    }

    /// From the stored `(authenticator_type, authenticator_id)` pair.
    #[must_use]
    pub fn from_columns(kind: Option<&str>, id: Option<Uuid>) -> Option<Self> {
        Some(Self {
            kind: AuthenticatorKind::parse(kind?)?,
            id: id?,
        })
    }
}

/// Whether the authenticator is `active` (pending and revoked ones
/// authenticate nothing, SM4): key devices and passkeys.
pub async fn is_active(conn: &mut sqlx::PgConnection, a: AuthenticatorRef) -> sqlx::Result<bool> {
    match a.kind {
        AuthenticatorKind::KeyDevice => Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE id = $1 AND state = 'active'
                            FOR SHARE)",
        )
        .bind(a.id)
        .fetch_one(&mut *conn)
        .await?),
        AuthenticatorKind::Passkey => Ok(sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE id = $1 AND state = 'active' FOR SHARE)",
        )
        .bind(a.id)
        .fetch_one(&mut *conn)
        .await?),
    }
}

/// Whether the authenticator is not revoked (`pending` or `active`): a
/// revocation would still change it.
pub async fn is_live(conn: &mut sqlx::PgConnection, a: AuthenticatorRef) -> sqlx::Result<bool> {
    let sql = match a.kind {
        AuthenticatorKind::KeyDevice => {
            "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE id = $1 AND state <> 'revoked')"
        }
        AuthenticatorKind::Passkey => {
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE id = $1 AND state <> 'revoked')"
        }
    };
    sqlx::query_scalar(sql)
        .bind(a.id)
        .fetch_one(&mut *conn)
        .await
}

/// Whether the authenticator is a row of `user_id` (any state).
pub async fn belongs_to(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    a: AuthenticatorRef,
) -> sqlx::Result<bool> {
    let sql = match a.kind {
        AuthenticatorKind::KeyDevice => {
            "SELECT EXISTS (SELECT 1 FROM approval_devices WHERE id = $1 AND user_id = $2)"
        }
        AuthenticatorKind::Passkey => {
            "SELECT EXISTS (SELECT 1 FROM passkeys WHERE id = $1 AND user_id = $2)"
        }
    };
    sqlx::query_scalar(sql)
        .bind(a.id)
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await
}

/// The account's active strong authenticators (B.0): key devices and
/// passkeys.
pub async fn active_strong_count(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM approval_devices WHERE user_id = $1 AND state = 'active')
              + (SELECT COUNT(*) FROM passkeys WHERE user_id = $1 AND state = 'active')",
    )
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await
}

/// A user cannot remove their own last strong authenticator (SM4):
/// 409 `LAST_STRONG_AUTHENTICATOR`. Removing one that is not active is
/// always allowed. Call under the account lock.
pub async fn ensure_not_last_strong(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    removing: AuthenticatorRef,
) -> crate::auth::AuthResult<()> {
    if is_active(conn, removing).await? && active_strong_count(conn, user_id).await? <= 1 {
        return Err(crate::auth::AuthError::LastStrongAuthenticator);
    }
    Ok(())
}

/// A newly added authenticator may not act against older ones.
///
/// `by` produced the step-up of the caller's action (call this in the
/// action's transaction, after the grant was consumed or peeked, under the
/// account lock). Refused with 403 `AUTHENTICATOR_TOO_NEW {allowed_at}` when
/// all hold:
///
/// 1. `by` was created less than `delay_hours` ago (`created_at`, on the database clock;
///    `allowed_at` = `created_at` + the delay);
/// 2. the account has another active strong authenticator created before `by` (an account whose
///    oldest active authenticators are this young is never held back);
/// 3. with a `target` (the removals): the target was created before `by` (removing itself or a
///    younger one stays allowed).
///
/// A `by` that is not an active strong authenticator of `user_id` is refused
/// as a missing grant (403 `STEP_UP_REQUIRED`), never treated as settled.
/// Every kind of strong authenticator is one arm of the `strong` list below.
pub async fn ensure_settled(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    by: AuthenticatorRef,
    target: Option<AuthenticatorRef>,
    delay_hours: i64,
) -> crate::auth::AuthResult<()> {
    settled_check(conn, user_id, by, target, delay_hours, None).await
}

/// [`ensure_settled`] at a fixed instant instead of the database's clock
/// (tests of the exact boundary only).
#[doc(hidden)]
pub async fn ensure_settled_at(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    by: AuthenticatorRef,
    target: Option<AuthenticatorRef>,
    delay_hours: i64,
    at: chrono::DateTime<chrono::Utc>,
) -> crate::auth::AuthResult<()> {
    settled_check(conn, user_id, by, target, delay_hours, Some(at)).await
}

async fn settled_check(
    conn: &mut sqlx::PgConnection,
    user_id: Uuid,
    by: AuthenticatorRef,
    target: Option<AuthenticatorRef>,
    delay_hours: i64,
    at: Option<chrono::DateTime<chrono::Utc>>,
) -> crate::auth::AuthResult<()> {
    let row: Option<(chrono::DateTime<chrono::Utc>, bool)> = sqlx::query_as(
        "WITH strong AS (
             SELECT 'key_device'::text AS kind, id, state, created_at
               FROM approval_devices WHERE user_id = $1
             UNION ALL
             SELECT 'passkey'::text, id, state, created_at
               FROM passkeys WHERE user_id = $1
         ), producer AS (
             SELECT created_at, created_at + make_interval(hours => $4) AS allowed_at
               FROM strong WHERE kind = $2 AND id = $3 AND state = 'active'
         )
         SELECT p.allowed_at,
                p.allowed_at > COALESCE($7, clock_timestamp())
                AND EXISTS (SELECT 1 FROM strong o
                             WHERE o.state = 'active' AND o.created_at < p.created_at)
                AND ($5::text IS NULL
                     OR EXISTS (SELECT 1 FROM strong t
                                 WHERE t.kind = $5 AND t.id = $6
                                   AND t.created_at < p.created_at))
           FROM producer p",
    )
    .bind(user_id)
    .bind(by.as_str())
    .bind(by.id)
    .bind(i32::try_from(delay_hours).unwrap_or(i32::MAX))
    .bind(target.map(|t| t.as_str()))
    .bind(target.map(|t| t.id))
    .bind(at)
    .fetch_optional(&mut *conn)
    .await?;
    match row {
        None => Err(crate::auth::AuthError::StepUpRequired),
        Some((allowed_at, true)) => Err(crate::auth::AuthError::AuthenticatorTooNew { allowed_at }),
        Some((_, false)) => Ok(()),
    }
}

/// B.0 "new account".
///
/// Never an active authenticator, a password, TOTP, recovery codes or a
/// session. `first_activity_at` is the marker; the
/// credentials and rows themselves count too, so an account that has one
/// without the marker (a registration with a password whose ticket is
/// still unbound, rows written outside the usual paths) is existing
/// (Flash r5 part 1 B1). Every caller that decides "new" uses this.
pub async fn is_new_account(conn: &mut sqlx::PgConnection, user_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT u.first_activity_at IS NULL
                AND u.password_hash IS NULL
                AND u.mfa_secret IS NULL
                AND NOT EXISTS (SELECT 1 FROM approval_devices d
                                 WHERE d.user_id = u.id AND d.state <> 'pending')
                AND NOT EXISTS (SELECT 1 FROM passkeys p
                                 WHERE p.user_id = u.id AND p.state <> 'pending')
                AND NOT EXISTS (SELECT 1 FROM recovery_codes rc WHERE rc.user_id = u.id)
                AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.user_id = u.id)
                AND NOT EXISTS (SELECT 1 FROM user_identities i WHERE i.user_id = u.id)
           FROM users u WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_one(&mut *conn)
    .await
}

/// Set `first_activity_at` once (B.0 "new account"); never cleared.
pub async fn mark_first_activity(conn: &mut sqlx::PgConnection, user_id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE users SET first_activity_at = COALESCE(first_activity_at, NOW()) WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The display name of an authenticator whose label is unknown (its row is
/// gone: a pending passkey deleted with its request). C27/C28 labels are
/// strings, never null.
#[must_use]
pub fn fallback_label(kind: &str) -> &'static str {
    match kind {
        "key_device" => "Key device",
        _ => "Passkey",
    }
}

/// The current display label of an authenticator (the key device's model or
/// the passkey's name), `None` if unknown.
pub async fn authenticator_label(
    conn: &mut sqlx::PgConnection,
    a: AuthenticatorRef,
) -> sqlx::Result<Option<String>> {
    match a.kind {
        AuthenticatorKind::KeyDevice => {
            sqlx::query_scalar("SELECT label FROM approval_devices WHERE id = $1")
                .bind(a.id)
                .fetch_optional(&mut *conn)
                .await
        }
        AuthenticatorKind::Passkey => {
            sqlx::query_scalar("SELECT label FROM passkeys WHERE id = $1")
                .bind(a.id)
                .fetch_optional(&mut *conn)
                .await
        }
    }
}
