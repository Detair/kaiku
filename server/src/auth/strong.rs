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
