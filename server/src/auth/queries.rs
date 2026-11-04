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
    // The system account (C28) is not a user: the first registered user
    // still becomes the admin.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE NOT is_system")
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
    .execute(&mut *conn)
    .await?;
    // C28: every change of the role is recorded (high).
    crate::security_events::record(
        conn,
        crate::security_events::NewSecurityEvent::new(
            user_id,
            crate::security_events::EventType::AdminRoleGranted,
            crate::security_events::Actor::System,
            serde_json::json!({ "by_user_id": null }),
        ),
    )
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
    password_hash: Option<&str>,
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

/// Whether the user has an active global ban (no expiry, or one in the future).
pub async fn is_user_globally_banned(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM global_bans
             WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW())
         )",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// One live session of a user, for `GET /auth/sessions`.
#[derive(Debug, sqlx::FromRow)]
pub struct LiveSessionRow {
    pub id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub created_at: DateTime<Utc>,
    pub assurance: String,
    pub client_class: String,
    pub auth_methods: Vec<String>,
    pub authenticator_type: Option<String>,
    pub authenticator_id: Option<Uuid>,
    /// The authenticator's current display label (C27 `authenticator.label`;
    /// key devices and passkeys).
    pub authenticator_label: Option<String>,
    pub last_used_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
}

/// List the live sessions of a user (ended and expired ones are not
/// listed), newest first.
pub async fn list_user_sessions(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<LiveSessionRow>, AuthError> {
    let sessions: Vec<LiveSessionRow> = sqlx::query_as(
        r"SELECT s.id, s.token_hash, s.expires_at, host(s.ip_address) as ip_address,
                 s.user_agent, s.city, s.country, s.created_at, s.assurance, s.client_class,
                 s.auth_methods, s.authenticator_type, s.authenticator_id,
                 COALESCE(d.label, p.label) AS authenticator_label,
                 s.last_used_at, s.absolute_expires_at
          FROM sessions s
          LEFT JOIN approval_devices d
                 ON s.authenticator_type = 'key_device' AND d.id = s.authenticator_id
                AND d.user_id = s.user_id
          LEFT JOIN passkeys p
                 ON s.authenticator_type = 'passkey' AND p.id = s.authenticator_id
                AND p.user_id = s.user_id
          WHERE s.user_id = $1 AND s.ended_at IS NULL
            AND s.expires_at > NOW() AND s.absolute_expires_at > NOW()
          ORDER BY s.created_at DESC",
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

// ============================================================================
// Password updates
// ============================================================================

/// Update the password hash for a user inside a transaction.
///
/// The user has now chosen their own password, so any pending forced
/// password change (admin-created account) is cleared as well.
pub async fn update_password_hash_tx(
    conn: &mut PgConnection,
    user_id: Uuid,
    new_hash: &str,
) -> Result<(), AuthError> {
    sqlx::query(
        "UPDATE users SET password_hash = $1, must_change_password = false, updated_at = NOW()
         WHERE id = $2",
    )
    .bind(new_hash)
    .bind(user_id)
    .execute(conn)
    .await?;
    Ok(())
}
