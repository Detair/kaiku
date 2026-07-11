//! Database queries for the admin module.
//!
//! Functions here cover the direct SQL needs of the admin surface:
//! user/guild listings and exports, elevated session lifecycle, global bans,
//! guild suspension, audit-log reads, per-guild page limits, and the dashboard
//! stats. Cross-module helpers (OIDC providers, auth methods, audit log
//! writes, elevated session creation) already live in `crate::db` and
//! `crate::permissions::queries` and are reused directly by handlers.
//!
//! All functions take `&PgPool` and return `Result<T, AdminError>` so the
//! caller can use `?` on sqlx errors.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, QueryBuilder};
use uuid::Uuid;

use super::error::AdminError;
use crate::permissions::models::AuditLogEntry;

// ============================================================================
// Internal row types
// ============================================================================

/// Row returned when looking up the caller's most recent elevated session.
pub(super) struct ElevatedSessionExpiresAt {
    pub expires_at: DateTime<Utc>,
}

/// Full elevated session row used by the `require_elevated` middleware.
pub(super) struct ElevatedSessionRow {
    pub user_id: Uuid,
    pub elevated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub reason: Option<String>,
}

/// Row returned when loading per-guild page limits.
pub(super) type GuildPageLimitsRow = (Option<i32>, Option<i32>);

/// Row returned by the admin user listing query.
#[allow(clippy::type_complexity)]
pub(super) type UserListRow = (
    Uuid,
    String,
    String,
    Option<String>,
    Option<String>,
    DateTime<Utc>,
    bool,
);

/// Row returned by the admin guild listing query.
#[allow(clippy::type_complexity)]
pub(super) type GuildListRow = (
    Uuid,
    String,
    Uuid,
    Option<String>,
    i64,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

/// Row returned when loading detailed user information.
pub(super) struct UserDetailsRow {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub is_banned: bool,
}

/// Row returned for each guild membership in the user-details view.
pub(super) struct UserGuildMembershipRow {
    pub guild_id: Uuid,
    pub guild_name: String,
    pub guild_icon_url: Option<String>,
    pub joined_at: DateTime<Utc>,
    pub is_owner: bool,
}

/// Row returned when loading detailed guild information.
pub(super) struct GuildDetailsRow {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub owner_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub suspended_at: Option<DateTime<Utc>>,
    pub member_count: i64,
}

/// Row returned when loading a guild owner's profile.
pub(super) struct GuildOwnerRow {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
}

/// Row returned for each member in the guild-details view.
pub(super) struct GuildMemberRow {
    pub user_id: Uuid,
    pub username: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub joined_at: DateTime<Utc>,
}

/// Row returned by the user CSV export query.
pub(super) struct UserExportRow {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    #[allow(dead_code)]
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub is_banned: bool,
}

/// Row returned by the guild CSV export query.
pub(super) struct GuildExportRow {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    #[allow(dead_code)]
    pub icon_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub suspended_at: Option<DateTime<Utc>>,
    pub member_count: i64,
}

/// Dynamic filter for the audit log listing endpoint.
#[derive(Debug, Default, Clone)]
pub(super) struct AuditLogFilter<'a> {
    pub action_filter: Option<&'a str>,
    pub exact_action_match: bool,
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
}

// ============================================================================
// Elevated sessions
// ============================================================================

/// Return whether the user has an active elevated admin session.
///
/// Used as the fail-secure fallback for the Redis-cached admin elevation
/// check in `admin::is_elevated_admin`.
pub async fn has_active_elevated_session(pool: &PgPool, user_id: Uuid) -> Result<bool, AdminError> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(
            SELECT 1 FROM elevated_sessions
            WHERE user_id = $1 AND expires_at > NOW()
        ) as "exists!""#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Fetch the most recent active elevated session for a user (if any).
///
/// Used by the `/api/admin/status` endpoint to report the current elevation
/// state back to the client.
pub(super) async fn find_latest_elevated_session_expiry(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<ElevatedSessionExpiresAt>, AdminError> {
    let row = sqlx::query_as!(
        ElevatedSessionExpiresAt,
        r#"SELECT expires_at
           FROM elevated_sessions
           WHERE user_id = $1 AND expires_at > NOW()
           ORDER BY elevated_at DESC
           LIMIT 1"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch the most recent active elevated session with full details for the
/// `require_elevated` middleware.
pub(super) async fn find_latest_elevated_session(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<ElevatedSessionRow>, AdminError> {
    let row = sqlx::query_as!(
        ElevatedSessionRow,
        r#"SELECT user_id, elevated_at, expires_at, reason
           FROM elevated_sessions
           WHERE user_id = $1 AND expires_at > NOW()
           ORDER BY elevated_at DESC
           LIMIT 1"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Find the most recent active login session id for a user, if any.
///
/// Elevation requires an existing session row because
/// `elevated_sessions.session_id` references `sessions.id`.
pub(super) async fn find_latest_active_session_id(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<Uuid>, AdminError> {
    let id = sqlx::query_scalar!(
        r#"SELECT id
           FROM sessions
           WHERE user_id = $1 AND expires_at > NOW()
           ORDER BY created_at DESC
           LIMIT 1"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(id)
}

/// Delete all elevated sessions for a user. Returns the number removed.
pub(super) async fn delete_elevated_sessions(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<u64, AdminError> {
    let result = sqlx::query!("DELETE FROM elevated_sessions WHERE user_id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Dashboard / stats
// ============================================================================

/// Total number of users on the instance.
pub(super) async fn count_users(pool: &PgPool) -> Result<i64, AdminError> {
    let count = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!" FROM users"#)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Total number of guilds on the instance.
pub(super) async fn count_guilds(pool: &PgPool) -> Result<i64, AdminError> {
    let count = sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!" FROM guilds"#)
        .fetch_one(pool)
        .await?;
    Ok(count)
}

/// Count of currently active (non-expired) global bans.
pub(super) async fn count_active_bans(pool: &PgPool) -> Result<i64, AdminError> {
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!"
           FROM global_bans
           WHERE expires_at IS NULL OR expires_at > NOW()"#
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

// ============================================================================
// Users: listing, details, export
// ============================================================================

/// Count users, optionally filtered by a case-insensitive `LIKE` pattern over
/// `username`, `display_name`, and `email`.
pub(super) async fn count_users_filtered(
    pool: &PgPool,
    search_pattern: Option<&str>,
) -> Result<i64, AdminError> {
    let count = if let Some(pattern) = search_pattern {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!"
               FROM users u
               WHERE LOWER(u.username) LIKE $1
                  OR LOWER(u.display_name) LIKE $1
                  OR LOWER(COALESCE(u.email, '')) LIKE $1"#,
            pattern
        )
        .fetch_one(pool)
        .await?
    } else {
        sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!" FROM users"#)
            .fetch_one(pool)
            .await?
    };
    Ok(count)
}

/// List users with pagination, optional search, and ban status.
pub(super) async fn list_users_filtered(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    search_pattern: Option<&str>,
) -> Result<Vec<UserListRow>, AdminError> {
    let rows = if let Some(pattern) = search_pattern {
        sqlx::query_as::<_, UserListRow>(
            r"SELECT
                u.id,
                u.username,
                u.display_name,
                u.email,
                u.avatar_url,
                u.created_at,
                EXISTS(
                    SELECT 1 FROM global_bans gb
                    WHERE gb.user_id = u.id
                      AND (gb.expires_at IS NULL OR gb.expires_at > NOW())
                ) as is_banned
              FROM users u
              WHERE LOWER(u.username) LIKE $3
                 OR LOWER(u.display_name) LIKE $3
                 OR LOWER(COALESCE(u.email, '')) LIKE $3
              ORDER BY u.created_at DESC
              LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(pattern)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, UserListRow>(
            r"SELECT
                u.id,
                u.username,
                u.display_name,
                u.email,
                u.avatar_url,
                u.created_at,
                EXISTS(
                    SELECT 1 FROM global_bans gb
                    WHERE gb.user_id = u.id
                      AND (gb.expires_at IS NULL OR gb.expires_at > NOW())
                ) as is_banned
              FROM users u
              ORDER BY u.created_at DESC
              LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

/// Fetch user headline data for the detail view (including banned status).
pub(super) async fn get_user_details(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<UserDetailsRow>, AdminError> {
    let row = sqlx::query_as!(
        UserDetailsRow,
        r#"SELECT id, username, display_name, email, avatar_url, created_at,
                  EXISTS(
                      SELECT 1 FROM global_bans gb
                      WHERE gb.user_id = users.id
                        AND (gb.expires_at IS NULL OR gb.expires_at > NOW())
                  ) as "is_banned!"
           FROM users
           WHERE id = $1"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Return the most recent login timestamp for a user (from `sessions`).
pub(super) async fn get_user_last_login(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<DateTime<Utc>>, AdminError> {
    let last_login = sqlx::query_scalar!(
        r#"SELECT MAX(created_at) as "last_login"
           FROM sessions
           WHERE user_id = $1"#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(last_login)
}

/// List the guild memberships for a user, newest first.
pub(super) async fn list_user_guild_memberships(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<UserGuildMembershipRow>, AdminError> {
    let rows = sqlx::query_as!(
        UserGuildMembershipRow,
        r#"SELECT g.id as guild_id,
                  g.name as guild_name,
                  g.icon_url as guild_icon_url,
                  gm.joined_at,
                  (g.owner_id = $1) as "is_owner!"
           FROM guild_members gm
           JOIN guilds g ON gm.guild_id = g.id
           WHERE gm.user_id = $1
           ORDER BY gm.joined_at DESC"#,
        user_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Export up to 10 000 users matching an optional search pattern for CSV.
pub(super) async fn export_users(
    pool: &PgPool,
    search_pattern: Option<&str>,
) -> Result<Vec<UserExportRow>, AdminError> {
    let rows = sqlx::query_as!(
        UserExportRow,
        r#"SELECT u.id, u.username, u.display_name, u.email, u.avatar_url, u.created_at,
                  EXISTS(
                      SELECT 1 FROM global_bans gb
                      WHERE gb.user_id = u.id
                        AND (gb.expires_at IS NULL OR gb.expires_at > NOW())
                  ) as "is_banned!"
           FROM users u
           WHERE $1::text IS NULL
              OR LOWER(u.username) LIKE $1
              OR LOWER(u.display_name) LIKE $1
              OR LOWER(COALESCE(u.email, '')) LIKE $1
           ORDER BY u.created_at DESC
           LIMIT 10000"#,
        search_pattern
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Check whether a user exists by id.
pub(super) async fn user_exists(pool: &PgPool, user_id: Uuid) -> Result<bool, AdminError> {
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM users WHERE id = $1) as "exists!""#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

/// Return the username for a user by id (`None` if the row does not exist).
pub(super) async fn get_username(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<String>, AdminError> {
    let row = sqlx::query_scalar!("SELECT username FROM users WHERE id = $1", user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Return the `(id, username)` pair for a user, or `None` if the row does not
/// exist. Used by handlers that need to both confirm existence and record the
/// username in audit logs / broadcasts.
pub(super) async fn find_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<(Uuid, String)>, AdminError> {
    let row = sqlx::query!("SELECT id, username FROM users WHERE id = $1", user_id)
        .fetch_optional(pool)
        .await?
        .map(|r| (r.id, r.username));
    Ok(row)
}

/// Fetch username rows for the given actor ids (audit-log enrichment).
pub(super) async fn lookup_usernames(
    pool: &PgPool,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, AdminError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query!("SELECT id, username FROM users WHERE id = ANY($1)", ids)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| (r.id, r.username))
        .collect();
    Ok(rows)
}

/// Delete a user by id, cascading to memberships, messages, sessions, etc.
/// Returns the number of rows deleted (0 or 1).
pub(super) async fn delete_user(pool: &PgPool, user_id: Uuid) -> Result<u64, AdminError> {
    let result = sqlx::query!("DELETE FROM users WHERE id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Global bans
// ============================================================================

/// Check whether a user is currently globally banned.
pub(super) async fn is_user_banned(pool: &PgPool, user_id: Uuid) -> Result<bool, AdminError> {
    let banned = sqlx::query_scalar!(
        r#"SELECT EXISTS(
            SELECT 1 FROM global_bans
            WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW())
        ) as "exists!""#,
        user_id
    )
    .fetch_one(pool)
    .await?;
    Ok(banned)
}

/// Insert or update a global ban, replacing any existing ban row for the user.
pub(super) async fn upsert_global_ban(
    pool: &PgPool,
    user_id: Uuid,
    banned_by: Uuid,
    reason: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), AdminError> {
    sqlx::query!(
        r#"INSERT INTO global_bans (user_id, banned_by, reason, expires_at)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id) DO UPDATE SET
               banned_by = $2,
               reason = $3,
               expires_at = $4,
               created_at = NOW()"#,
        user_id,
        banned_by,
        reason,
        expires_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert a global ban without replacing an existing row — used by the bulk
/// ban flow which checks for an existing ban separately so that it can
/// report "already banned" counts to the caller.
pub(super) async fn insert_global_ban(
    pool: &PgPool,
    user_id: Uuid,
    banned_by: Uuid,
    reason: &str,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), AdminError> {
    sqlx::query!(
        r#"INSERT INTO global_bans (user_id, banned_by, reason, expires_at)
           VALUES ($1, $2, $3, $4)"#,
        user_id,
        banned_by,
        reason,
        expires_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a user's global ban. Returns the number of rows deleted (0 or 1).
pub(super) async fn delete_global_ban(pool: &PgPool, user_id: Uuid) -> Result<u64, AdminError> {
    let result = sqlx::query!("DELETE FROM global_bans WHERE user_id = $1", user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Guilds: listing, details, export, suspend/delete
// ============================================================================

/// Count guilds, optionally filtered by a case-insensitive `LIKE` pattern on
/// `name`.
pub(super) async fn count_guilds_filtered(
    pool: &PgPool,
    search_pattern: Option<&str>,
) -> Result<i64, AdminError> {
    let count = if let Some(pattern) = search_pattern {
        sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!"
               FROM guilds g
               WHERE LOWER(g.name) LIKE $1"#,
            pattern
        )
        .fetch_one(pool)
        .await?
    } else {
        sqlx::query_scalar!(r#"SELECT COUNT(*) as "count!" FROM guilds"#)
            .fetch_one(pool)
            .await?
    };
    Ok(count)
}

/// List guilds with pagination, optional search, and member counts.
pub(super) async fn list_guilds_filtered(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    search_pattern: Option<&str>,
) -> Result<Vec<GuildListRow>, AdminError> {
    let rows = if let Some(pattern) = search_pattern {
        sqlx::query_as::<_, GuildListRow>(
            r"SELECT
                g.id,
                g.name,
                g.owner_id,
                g.icon_url,
                COALESCE(
                    (SELECT COUNT(*) FROM guild_members gm WHERE gm.guild_id = g.id),
                    0
                ) as member_count,
                g.created_at,
                g.suspended_at
              FROM guilds g
              WHERE LOWER(g.name) LIKE $3
              ORDER BY g.created_at DESC
              LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .bind(pattern)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, GuildListRow>(
            r"SELECT
                g.id,
                g.name,
                g.owner_id,
                g.icon_url,
                COALESCE(
                    (SELECT COUNT(*) FROM guild_members gm WHERE gm.guild_id = g.id),
                    0
                ) as member_count,
                g.created_at,
                g.suspended_at
              FROM guilds g
              ORDER BY g.created_at DESC
              LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

/// Fetch the detailed view of a guild (headline fields + member count).
pub(super) async fn get_guild_details(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Option<GuildDetailsRow>, AdminError> {
    let row = sqlx::query_as!(
        GuildDetailsRow,
        r#"SELECT g.id, g.name, g.icon_url, g.owner_id, g.created_at, g.suspended_at,
                  (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) as "member_count!"
           FROM guilds g
           WHERE g.id = $1"#,
        guild_id
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch the profile fields for a guild owner (used by the detail view).
pub(super) async fn get_guild_owner(
    pool: &PgPool,
    owner_id: Uuid,
) -> Result<GuildOwnerRow, AdminError> {
    let row = sqlx::query_as!(
        GuildOwnerRow,
        r#"SELECT id, username, display_name, avatar_url
           FROM users
           WHERE id = $1"#,
        owner_id
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Fetch up to 5 most-recent guild members (excluding the owner).
pub(super) async fn list_top_guild_members(
    pool: &PgPool,
    guild_id: Uuid,
    owner_id: Uuid,
) -> Result<Vec<GuildMemberRow>, AdminError> {
    let rows = sqlx::query_as!(
        GuildMemberRow,
        r#"SELECT u.id as user_id, u.username, u.display_name, u.avatar_url, gm.joined_at
           FROM guild_members gm
           JOIN users u ON gm.user_id = u.id
           WHERE gm.guild_id = $1 AND gm.user_id != $2
           ORDER BY gm.joined_at DESC
           LIMIT 5"#,
        guild_id,
        owner_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Export up to 10 000 guilds matching an optional search pattern for CSV.
pub(super) async fn export_guilds(
    pool: &PgPool,
    search_pattern: Option<&str>,
) -> Result<Vec<GuildExportRow>, AdminError> {
    let rows = sqlx::query_as!(
        GuildExportRow,
        r#"SELECT g.id, g.name, g.owner_id, g.icon_url, g.created_at, g.suspended_at,
                  (SELECT COUNT(*) FROM guild_members WHERE guild_id = g.id) as "member_count!"
           FROM guilds g
           WHERE $1::text IS NULL OR LOWER(g.name) LIKE $1
           ORDER BY g.created_at DESC
           LIMIT 10000"#,
        search_pattern
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Return the `(id, name)` pair for a guild, or `None` if it does not exist.
pub(super) async fn find_guild(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Option<(Uuid, String)>, AdminError> {
    let row = sqlx::query!("SELECT id, name FROM guilds WHERE id = $1", guild_id)
        .fetch_optional(pool)
        .await?
        .map(|r| (r.id, r.name));
    Ok(row)
}

/// Return the name of a guild, or `None` if it does not exist.
pub(super) async fn get_guild_name(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Option<String>, AdminError> {
    let row = sqlx::query_scalar!("SELECT name FROM guilds WHERE id = $1", guild_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Return the current suspension status for a guild (used by bulk suspend).
pub(super) async fn get_guild_suspension_status(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Option<Option<DateTime<Utc>>>, AdminError> {
    let row = sqlx::query!(
        "SELECT id, suspended_at FROM guilds WHERE id = $1",
        guild_id
    )
    .fetch_optional(pool)
    .await?
    .map(|r| r.suspended_at);
    Ok(row)
}

/// Mark a guild as suspended. Returns the number of rows updated (0 if the
/// guild is already suspended or does not exist).
pub(super) async fn suspend_guild(
    pool: &PgPool,
    guild_id: Uuid,
    suspended_by: Uuid,
    reason: &str,
) -> Result<u64, AdminError> {
    let result = sqlx::query!(
        r#"UPDATE guilds SET
               suspended_at = NOW(),
               suspended_by = $2,
               suspension_reason = $3
           WHERE id = $1 AND suspended_at IS NULL"#,
        guild_id,
        suspended_by,
        reason
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Bulk-suspend path: update a guild without touching `suspended_by` (matches
/// the existing behaviour of the bulk endpoint).
pub(super) async fn bulk_suspend_guild(
    pool: &PgPool,
    guild_id: Uuid,
    reason: &str,
) -> Result<u64, AdminError> {
    let result = sqlx::query!(
        r#"UPDATE guilds SET suspended_at = NOW(), suspension_reason = $1 WHERE id = $2"#,
        reason,
        guild_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Clear a guild's suspension. Returns the number of rows updated.
pub(super) async fn unsuspend_guild(pool: &PgPool, guild_id: Uuid) -> Result<u64, AdminError> {
    let result = sqlx::query!(
        r#"UPDATE guilds SET
               suspended_at = NULL,
               suspended_by = NULL,
               suspension_reason = NULL
           WHERE id = $1 AND suspended_at IS NOT NULL"#,
        guild_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Delete a guild by id, cascading to channels, messages, roles, etc.
pub(super) async fn delete_guild(pool: &PgPool, guild_id: Uuid) -> Result<u64, AdminError> {
    let result = sqlx::query!("DELETE FROM guilds WHERE id = $1", guild_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

// ============================================================================
// Per-guild page limits
// ============================================================================

/// Load the stored per-guild page limits.
pub(super) async fn get_guild_page_limits(
    pool: &PgPool,
    guild_id: Uuid,
) -> Result<Option<GuildPageLimitsRow>, AdminError> {
    let row = sqlx::query_as::<_, GuildPageLimitsRow>(
        "SELECT max_pages, max_revisions FROM guilds WHERE id = $1",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Patch per-guild page limits using `CASE` expressions so that only the
/// fields the caller actually provided are updated.
pub(super) async fn set_guild_page_limits(
    pool: &PgPool,
    guild_id: Uuid,
    max_pages_present: bool,
    max_pages_value: Option<i32>,
    max_revisions_present: bool,
    max_revisions_value: Option<i32>,
) -> Result<Option<GuildPageLimitsRow>, AdminError> {
    let row = sqlx::query_as::<_, GuildPageLimitsRow>(
        r"UPDATE guilds
          SET max_pages = CASE WHEN $2 THEN $3 ELSE max_pages END,
              max_revisions = CASE WHEN $4 THEN $5 ELSE max_revisions END
          WHERE id = $1
          RETURNING max_pages, max_revisions",
    )
    .bind(guild_id)
    .bind(max_pages_present)
    .bind(max_pages_value)
    .bind(max_revisions_present)
    .bind(max_revisions_value)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

// ============================================================================
// System announcements
// ============================================================================

/// Insert a new system announcement.
#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_announcement(
    pool: &PgPool,
    id: Uuid,
    author_id: Uuid,
    title: &str,
    content: &str,
    severity: &str,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
) -> Result<(), AdminError> {
    sqlx::query!(
        r#"INSERT INTO system_announcements
               (id, author_id, title, content, severity, starts_at, ends_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        id,
        author_id,
        title,
        content,
        severity,
        starts_at,
        ends_at
    )
    .execute(pool)
    .await?;
    Ok(())
}

// ============================================================================
// Observability summary vital signs
// ============================================================================

/// Average p95 HTTP latency over a time window.
pub async fn summary_latency_p95(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Option<f64>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(value_p95) FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_http_request_duration_ms' \
         AND ts >= $1 AND ts <= $2",
    )
    .bind(from)
    .bind(to)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}

/// Total HTTP errors and requests in a time window, used to compute error
/// rate in the observability summary.
pub async fn summary_error_and_request_counts(
    pool: &PgPool,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Option<(Option<i64>, Option<i64>)>, sqlx::Error> {
    // SUM() over a bigint column returns NUMERIC in PostgreSQL; cast back to
    // bigint so it decodes into i64 (the sums are small request counts).
    sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
        "SELECT \
             SUM(CASE WHEN metric_name = 'kaiku_http_errors_total' THEN value_count ELSE 0 END)::bigint, \
             SUM(CASE WHEN metric_name = 'kaiku_http_requests_total' THEN value_count ELSE 0 END)::bigint \
         FROM telemetry_metric_samples \
         WHERE metric_name IN ('kaiku_http_errors_total', 'kaiku_http_requests_total') \
         AND ts >= $1 AND ts <= $2",
    )
    .bind(from)
    .bind(to)
    .fetch_optional(pool)
    .await
}

/// Most recent gauge sample for active WebSocket connections.
pub async fn summary_active_ws_connections(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<i64>>(
        "SELECT value_count FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_ws_connections_active' \
         AND ts >= $1 \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(since)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}

/// Most recent gauge sample for active voice sessions.
pub async fn summary_active_voice_sessions(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<i64>>(
        "SELECT value_count FROM telemetry_metric_samples \
         WHERE metric_name = 'kaiku_voice_sessions_active' \
         AND ts >= $1 \
         ORDER BY ts DESC LIMIT 1",
    )
    .bind(since)
    .fetch_optional(pool)
    .await
    .map(Option::flatten)
}

/// Total user count (observability summary metadata).
pub async fn summary_user_count(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
}

/// Total guild count (observability summary metadata).
pub async fn summary_guild_count(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM guilds")
        .fetch_one(pool)
        .await
}

/// Recent `ERROR`-level log count since a given timestamp.
pub async fn summary_recent_error_count(
    pool: &PgPool,
    since: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM telemetry_log_events \
         WHERE level = 'ERROR' AND ts >= $1",
    )
    .bind(since)
    .fetch_one(pool)
    .await
}

// ============================================================================
// Audit log
// ============================================================================

/// List audit-log entries with dynamic WHERE clauses built from the filter.
///
/// Returns `(entries, total_count)`. The total is computed with a matching
/// `SELECT COUNT(*)` so pagination can report an accurate total.
pub(super) async fn list_audit_log(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    filter: &AuditLogFilter<'_>,
) -> Result<(Vec<AuditLogEntry>, i64), AdminError> {
    let action_pattern = filter.action_filter.map(|a| {
        if filter.exact_action_match {
            a.to_string()
        } else {
            format!("{a}%")
        }
    });

    // Shared WHERE-clause builder used by both the count and main queries.
    let push_filters = |builder: &mut QueryBuilder<sqlx::Postgres>| {
        let mut has_condition = false;
        if let Some(ref pattern) = action_pattern {
            builder.push(" WHERE ");
            has_condition = true;
            if filter.exact_action_match {
                builder.push("action = ").push_bind(pattern.clone());
            } else {
                builder.push("action LIKE ").push_bind(pattern.clone());
            }
        }
        if let Some(from) = filter.from_date {
            builder.push(if has_condition { " AND " } else { " WHERE " });
            has_condition = true;
            builder.push("created_at >= ").push_bind(from);
        }
        if let Some(to) = filter.to_date {
            builder.push(if has_condition { " AND " } else { " WHERE " });
            let _ = has_condition;
            builder.push("created_at <= ").push_bind(to);
        }
    };

    // Count query
    let mut count_builder = QueryBuilder::new("SELECT COUNT(*) FROM system_audit_log");
    push_filters(&mut count_builder);
    let (total,): (i64,) = count_builder
        .build_query_as::<(i64,)>()
        .fetch_one(pool)
        .await?;

    // Main query
    let mut builder = QueryBuilder::new(
        "SELECT id, actor_id, action, target_type, target_id, details, \
         host(ip_address) as ip_address, created_at \
         FROM system_audit_log",
    );
    push_filters(&mut builder);
    builder
        .push(" ORDER BY created_at DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);
    let entries: Vec<AuditLogEntry> = builder
        .build_query_as::<AuditLogEntry>()
        .fetch_all(pool)
        .await?;

    Ok((entries, total))
}
