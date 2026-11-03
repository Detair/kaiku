//! The `passkeys` table, the `WebAuthn` user handle and revocation.
//!
//! B.7 rule 1: callers that change passkeys lock the account first
//! (`approvals::transition::lock_accounts`); this module adds no lock.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;
use webauthn_rs::prelude::Passkey;

use super::rp::VerifiedRegistration;
use crate::auth::ctx::AuthCtx;
use crate::auth::AuthResult;

/// SM4 state of a passkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PasskeyState {
    Pending,
    Active,
    Revoked,
}

impl PasskeyState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }
}

impl TryFrom<String> for PasskeyState {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        match s.as_str() {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            _ => Err(format!("unknown passkey state {s}")),
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PasskeyRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub credential_id: Vec<u8>,
    pub credential: sqlx::types::Json<Passkey>,
    pub label: String,
    pub aaguid: Option<Uuid>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub transports: Vec<String>,
    #[sqlx(try_from = "String")]
    pub state: PasskeyState,
    pub ready_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

pub struct NewPasskey<'a> {
    pub user_id: Uuid,
    pub verified: &'a VerifiedRegistration,
    pub label: &'a str,
    pub state: PasskeyState,
    pub ready_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub enum InsertError {
    Duplicate,
    Db(sqlx::Error),
}

/// Only this constant is interpolated into SQL (every value is bound).
const COLS: &str = "id, user_id, credential_id, credential, label, aaguid, backup_eligible,
                    backup_state, transports, state, ready_at, created_at, last_used_at";

/// The account's `WebAuthn` user handle: a random v4 UUID set at first use
/// (never `users.id`), stable afterwards.
pub async fn user_handle(db: impl PgExecutor<'_> + Copy, user_id: Uuid) -> sqlx::Result<Uuid> {
    // Read first: once set, the handle never changes and the account row is
    // not touched (no row lock behind `lock_accounts`, no dead tuple).
    let existing: Option<Option<Uuid>> =
        sqlx::query_scalar("SELECT webauthn_user_handle FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(db)
            .await?;
    if let Some(Some(handle)) = existing {
        return Ok(handle);
    }
    sqlx::query_scalar(
        "UPDATE users SET webauthn_user_handle = COALESCE(webauthn_user_handle, $2)
          WHERE id = $1 RETURNING webauthn_user_handle",
    )
    .bind(user_id)
    .bind(Uuid::new_v4())
    .fetch_one(db)
    .await
}

pub async fn user_id_for_handle(
    db: impl PgExecutor<'_>,
    handle: Uuid,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM users WHERE webauthn_user_handle = $1")
        .bind(handle)
        .fetch_optional(db)
        .await
}

/// Insert a verified credential. An `active` row marks the account's first
/// activity (B.0). A credential id already stored → `Duplicate`.
pub async fn insert(
    conn: &mut PgConnection,
    new: &NewPasskey<'_>,
) -> Result<PasskeyRow, InsertError> {
    let credential_id = new.verified.passkey.cred_id().as_ref().to_vec();
    let row: PasskeyRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO passkeys (id, user_id, credential_id, credential, label, aaguid,
                               backup_eligible, backup_state, transports, state, ready_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING {COLS}"
    )))
    .bind(Uuid::now_v7())
    .bind(new.user_id)
    .bind(&credential_id)
    .bind(sqlx::types::Json(&new.verified.passkey))
    .bind(new.label)
    .bind(new.verified.aaguid)
    .bind(new.verified.backup_eligible)
    .bind(new.verified.backup_state)
    .bind(&new.verified.transports)
    .bind(new.state.as_str())
    .bind(new.ready_at)
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => InsertError::Duplicate,
        _ => InsertError::Db(e),
    })?;
    if row.state == PasskeyState::Active {
        crate::auth::strong::mark_first_activity(conn, new.user_id)
            .await
            .map_err(InsertError::Db)?;
    }
    Ok(row)
}

/// `pending` → `active` (L1b's "Finish"); marks the first activity. False
/// if the row was not pending; 409 `PASSKEY_LIMIT` at 10 active passkeys
/// (the caller holds the account lock, B.7 rule 1).
pub async fn activate(conn: &mut PgConnection, user_id: Uuid, id: Uuid) -> AuthResult<bool> {
    if active_count(&mut *conn, user_id).await? >= super::MAX_ACTIVE_PASSKEYS as i64 {
        return Err(crate::auth::AuthError::PasskeyLimit);
    }
    let done = sqlx::query(
        "UPDATE passkeys SET state = 'active', ready_at = NULL
          WHERE id = $1 AND user_id = $2 AND state = 'pending'",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut *conn)
    .await?
    .rows_affected()
        == 1;
    if done {
        crate::auth::strong::mark_first_activity(conn, user_id).await?;
    }
    Ok(done)
}

pub async fn find_by_credential_id(
    db: impl PgExecutor<'_>,
    credential_id: &[u8],
) -> sqlx::Result<Option<PasskeyRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM passkeys WHERE credential_id = $1"
    )))
    .bind(credential_id)
    .fetch_optional(db)
    .await
}

/// The caller's passkey `id` in any state.
pub async fn find_own(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<PasskeyRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM passkeys WHERE id = $1 AND user_id = $2"
    )))
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// `pending` and `active` passkeys, oldest first.
pub async fn list_live(db: impl PgExecutor<'_>, user_id: Uuid) -> sqlx::Result<Vec<PasskeyRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM passkeys WHERE user_id = $1 AND state <> 'revoked'
          ORDER BY created_at, id"
    )))
    .bind(user_id)
    .fetch_all(db)
    .await
}

pub async fn list_active(db: impl PgExecutor<'_>, user_id: Uuid) -> sqlx::Result<Vec<PasskeyRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM passkeys WHERE user_id = $1 AND state = 'active'
          ORDER BY created_at, id"
    )))
    .bind(user_id)
    .fetch_all(db)
    .await
}

pub async fn active_count(db: impl PgExecutor<'_>, user_id: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = $1 AND state = 'active'")
        .bind(user_id)
        .fetch_one(db)
        .await
}

/// Whether passkey `id` is `active` (row share-locked for the transaction).
pub async fn is_active(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<bool> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT state FROM passkeys WHERE id = $1 FOR SHARE")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .is_some_and(|s| s == "active"),
    )
}

/// After a verified assertion: the counter high-water mark, BS and last use.
///
/// One atomic statement on the row (`GREATEST`: a late or regressed
/// assertion never lowers the stored counter). Nothing else changes (no
/// owner evidence, CC4).
pub async fn record_use(
    db: impl PgExecutor<'_>,
    id: Uuid,
    received_counter: u32,
    backup_state: bool,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE passkeys SET
             credential = jsonb_set(
                 jsonb_set(credential, '{cred,counter}',
                           to_jsonb(GREATEST(COALESCE((credential #>> '{cred,counter}')::bigint, 0),
                                             $2::bigint))),
                 '{cred,backup_state}', to_jsonb($3::boolean)),
             backup_state = $3,
             last_used_at = NOW()
          WHERE id = $1",
    )
    .bind(id)
    .bind(i64::from(received_counter))
    .bind(backup_state)
    .execute(db)
    .await?;
    Ok(())
}

/// Rename the caller's live passkey; `None` if it is not one.
pub async fn rename(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    id: Uuid,
    label: &str,
) -> sqlx::Result<Option<PasskeyRow>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE passkeys SET label = $3 WHERE id = $1 AND user_id = $2 AND state <> 'revoked'
         RETURNING {COLS}"
    )))
    .bind(id)
    .bind(user_id)
    .bind(label)
    .fetch_optional(db)
    .await
}

/// Revoke the caller's active passkey and run the revocation cascade in the
/// same transaction (its sessions end, its grants are voided; B.7 rule 2).
/// `None` if it is not an active passkey of `user_id`.
pub async fn revoke(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    user_id: Uuid,
    id: Uuid,
    reason: &str,
) -> AuthResult<Option<PasskeyRow>> {
    let row: Option<PasskeyRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE passkeys SET state = 'revoked', revoked_at = NOW(), revoke_reason = $3
          WHERE id = $1 AND user_id = $2 AND state = 'active'
         RETURNING {COLS}"
    )))
    .bind(id)
    .bind(user_id)
    .bind(reason)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(row) = &row {
        crate::approvals::cascade::on_authenticator_revoked(
            conn,
            ctx,
            user_id,
            crate::auth::strong::AuthenticatorRef::passkey(row.id),
        )
        .await?;
    }
    Ok(row)
}

/// Revoke every live (pending and active) passkey of every account, each
/// through the cascade (RP-ID change, DC5). Locks the affected accounts
/// first (B.7 rule 1).
pub async fn revoke_all_live(
    conn: &mut PgConnection,
    ctx: &AuthCtx<'_>,
    reason: &str,
) -> AuthResult<u64> {
    let users: Vec<Uuid> =
        sqlx::query_scalar("SELECT DISTINCT user_id FROM passkeys WHERE state <> 'revoked'")
            .fetch_all(&mut *conn)
            .await?;
    crate::approvals::transition::lock_accounts(conn, &users).await?;
    // Only the accounts locked above (a passkey added meanwhile waits for the
    // next start).
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE passkeys SET state = 'revoked', revoked_at = NOW(), revoke_reason = $1,
                             ready_at = NULL
          WHERE state <> 'revoked' AND user_id = ANY($2)
         RETURNING id, user_id",
    )
    .bind(reason)
    .bind(&users)
    .fetch_all(&mut *conn)
    .await?;
    for (id, user_id) in &rows {
        crate::approvals::cascade::on_authenticator_revoked(
            conn,
            ctx,
            *user_id,
            crate::auth::strong::AuthenticatorRef::passkey(*id),
        )
        .await?;
    }
    Ok(rows.len() as u64)
}
