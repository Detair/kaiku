//! Database queries for the auth module.
//!
//! Centralizes the inline `sqlx::query*` calls that previously lived in
//! `handlers.rs`. Functions that participate in a transaction take a
//! `&mut PgConnection` so the caller can keep the transaction boundary
//! in the handler — see `register` / `refresh_token` for examples.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::error::AuthError;
use crate::db::{Session, User};

// ============================================================================
// Setup / first-user serialization
// ============================================================================

/// Acquire a `FOR UPDATE` lock on the `setup_complete` config row to serialize
/// concurrent registrations. Must be called inside a transaction.
///
/// The lock is held until the transaction commits or rolls back, preventing
/// the race condition where two concurrent registrations both observe
/// `user_count = 0` and both try to grant admin to the first user.
pub async fn lock_setup_complete(conn: &mut PgConnection) -> Result<(), AuthError> {
    sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT value FROM server_config WHERE key = 'setup_complete' FOR UPDATE",
    )
    .fetch_one(conn)
    .await?;
    Ok(())
}

/// Count all users (used for first-user detection inside a registration tx).
pub async fn count_users(conn: &mut PgConnection) -> Result<i64, AuthError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(conn)
        .await?;
    Ok(count)
}

/// Grant the system admin role to the first registered user.
pub async fn grant_first_user_admin(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<(), AuthError> {
    sqlx::query!(
        "INSERT INTO system_admins (user_id, granted_by) VALUES ($1, $1)",
        user_id
    )
    .execute(conn)
    .await?;
    Ok(())
}

// ============================================================================
// User creation (transactional)
// ============================================================================

/// Insert a new local-auth user row inside a transaction.
pub async fn insert_local_user(
    conn: &mut PgConnection,
    username: &str,
    display_name: &str,
    email: Option<&str>,
    password_hash: &str,
) -> Result<User, AuthError> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (username, display_name, email, password_hash, auth_method)
         VALUES ($1, $2, $3, $4, 'local')
         RETURNING *",
    )
    .bind(username)
    .bind(display_name)
    .bind(email)
    .bind(password_hash)
    .fetch_one(conn)
    .await?;
    Ok(user)
}

/// Insert a new OIDC user row inside a transaction. Returns `Ok(None)` if
/// the unique username constraint is violated so the caller can retry with
/// a suffixed username.
pub async fn insert_oidc_user(
    conn: &mut PgConnection,
    username: &str,
    display_name: &str,
    email: Option<&str>,
    external_id: &str,
    avatar_url: Option<&str>,
) -> Result<Option<User>, AuthError> {
    let result = sqlx::query_as::<_, User>(
        "INSERT INTO users (username, display_name, email, auth_method, external_id, avatar_url)
         VALUES ($1, $2, $3, 'oidc', $4, $5)
         RETURNING *",
    )
    .bind(username)
    .bind(display_name)
    .bind(email)
    .bind(external_id)
    .bind(avatar_url)
    .fetch_one(conn)
    .await;

    match result {
        Ok(user) => Ok(Some(user)),
        Err(sqlx::Error::Database(ref db_err)) if db_err.is_unique_violation() => Ok(None),
        Err(e) => Err(AuthError::Database(e)),
    }
}

// ============================================================================
// Sessions
// ============================================================================

/// Parameters for inserting a new session row.
pub struct InsertSessionParams<'a> {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: &'a str,
    pub expires_at: DateTime<Utc>,
    pub ip_address: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub city: Option<&'a str>,
    pub country: Option<&'a str>,
}

/// Insert a session row inside a transaction.
pub async fn insert_session_tx(
    conn: &mut PgConnection,
    params: &InsertSessionParams<'_>,
) -> Result<(), AuthError> {
    sqlx::query(
        r"INSERT INTO sessions (id, user_id, token_hash, expires_at, ip_address, user_agent, city, country)
          VALUES ($1, $2, $3, $4, $5::inet, $6, $7, $8)",
    )
    .bind(params.id)
    .bind(params.user_id)
    .bind(params.token_hash)
    .bind(params.expires_at)
    .bind(params.ip_address)
    .bind(params.user_agent)
    .bind(params.city)
    .bind(params.country)
    .execute(conn)
    .await?;
    Ok(())
}

/// Lock a session row by `token_hash` for token rotation. Returns `None`
/// when the session does not exist or has expired.
pub async fn lock_session_by_token_hash_tx(
    conn: &mut PgConnection,
    token_hash: &str,
) -> Result<Option<Session>, AuthError> {
    let session: Option<Session> = sqlx::query_as(
        r"
        SELECT id, user_id, token_hash, expires_at, host(ip_address) as ip_address, user_agent, city, country, created_at
        FROM sessions
        WHERE token_hash = $1 AND expires_at > NOW()
        FOR UPDATE
        ",
    )
    .bind(token_hash)
    .fetch_optional(conn)
    .await?;
    Ok(session)
}

/// Delete the session identified by `token_hash` inside a transaction.
pub async fn delete_session_by_token_hash_tx(
    conn: &mut PgConnection,
    token_hash: &str,
) -> Result<(), AuthError> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
        .bind(token_hash)
        .execute(conn)
        .await?;
    Ok(())
}

/// List all non-expired sessions belonging to a user.
pub async fn list_user_sessions(pool: &PgPool, user_id: Uuid) -> Result<Vec<Session>, AuthError> {
    let sessions: Vec<Session> = sqlx::query_as(
        r"SELECT id, user_id, token_hash, expires_at, host(ip_address) as ip_address,
                 user_agent, city, country, created_at
          FROM sessions
          WHERE user_id = $1 AND expires_at > NOW()
          ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(sessions)
}

/// Find a session by id (does not enforce ownership).
pub async fn find_session_by_id(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<Session>, AuthError> {
    let session: Option<Session> = sqlx::query_as(
        "SELECT id, user_id, token_hash, expires_at, host(ip_address) as ip_address, user_agent, city, country, created_at
         FROM sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(session)
}

/// Delete a session by id.
pub async fn delete_session_by_id(pool: &PgPool, session_id: Uuid) -> Result<(), AuthError> {
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete all sessions for a user other than the one identified by
/// `current_token_hash`. Returns the number of deleted rows.
pub async fn delete_other_user_sessions(
    pool: &PgPool,
    user_id: Uuid,
    current_token_hash: &str,
) -> Result<u64, AuthError> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash != $2")
        .bind(user_id)
        .bind(current_token_hash)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Delete all sessions for a user other than `current_token_hash` inside a
/// transaction. Returns the number of deleted rows.
pub async fn delete_other_user_sessions_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
    current_token_hash: &str,
) -> Result<u64, AuthError> {
    let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND token_hash != $2")
        .bind(user_id)
        .bind(current_token_hash)
        .execute(conn)
        .await?;
    Ok(result.rows_affected())
}

/// Delete all sessions for a user inside a transaction.
pub async fn delete_all_user_sessions_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<(), AuthError> {
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(())
}

// ============================================================================
// Password updates
// ============================================================================

/// Update the password hash for a user inside a transaction.
pub async fn update_password_hash_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
    new_hash: &str,
) -> Result<(), AuthError> {
    sqlx::query("UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2")
        .bind(new_hash)
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(())
}

// ============================================================================
// Password reset tokens
// ============================================================================

/// Mark a password reset token as consumed inside a transaction.
pub async fn mark_reset_token_used_tx(
    conn: &mut PgConnection,
    token_id: Uuid,
) -> Result<(), AuthError> {
    sqlx::query("UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(conn)
        .await?;
    Ok(())
}
