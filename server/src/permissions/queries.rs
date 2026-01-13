//! Database queries for the permission system.
//!
//! Provides async functions for managing:
//! - System admin records
//! - Elevated sessions (sudo-style)
//! - Guild roles and member assignments
//! - Channel permission overrides
//! - Audit logging

use chrono::{Duration, Utc};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use uuid::Uuid;

use super::guild::GuildPermissions;
use super::models::{AuditLogEntry, ChannelOverride, ElevatedSession, GuildRole, SystemAdmin};

// ============================================================================
// System Admin Queries
// ============================================================================

/// Check if a user is a system admin.
pub async fn is_system_admin(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool> {
    let result: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM system_admins WHERE user_id = $1)")
            .bind(user_id)
            .fetch_one(pool)
            .await?;

    Ok(result.0)
}

/// Get system admin record for a user.
pub async fn get_system_admin(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Option<SystemAdmin>> {
    sqlx::query_as::<_, SystemAdmin>(
        r"
        SELECT user_id, granted_by, granted_at
        FROM system_admins
        WHERE user_id = $1
        ",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// List all system admins.
pub async fn list_system_admins(pool: &PgPool) -> sqlx::Result<Vec<SystemAdmin>> {
    sqlx::query_as::<_, SystemAdmin>(
        r"
        SELECT user_id, granted_by, granted_at
        FROM system_admins
        ORDER BY granted_at ASC
        ",
    )
    .fetch_all(pool)
    .await
}

/// Grant system admin privileges to a user.
pub async fn grant_system_admin(
    pool: &PgPool,
    user_id: Uuid,
    granted_by: Option<Uuid>,
) -> sqlx::Result<SystemAdmin> {
    sqlx::query_as::<_, SystemAdmin>(
        r"
        INSERT INTO system_admins (user_id, granted_by)
        VALUES ($1, $2)
        RETURNING user_id, granted_by, granted_at
        ",
    )
    .bind(user_id)
    .bind(granted_by)
    .fetch_one(pool)
    .await
}

/// Revoke system admin privileges from a user.
///
/// Returns `true` if an admin was revoked, `false` if user was not an admin.
pub async fn revoke_system_admin(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM system_admins WHERE user_id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Elevated Session Queries
// ============================================================================

/// Get an elevated session by session ID.
pub async fn get_elevated_session(
    pool: &PgPool,
    session_id: Uuid,
) -> sqlx::Result<Option<ElevatedSession>> {
    sqlx::query_as::<_, ElevatedSession>(
        r"
        SELECT
            id,
            user_id,
            session_id,
            host(ip_address) as ip_address,
            elevated_at,
            expires_at,
            reason
        FROM elevated_sessions
        WHERE session_id = $1
          AND expires_at > NOW()
        ",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
}

/// Create or update an elevated session.
///
/// Uses ON CONFLICT UPDATE to extend an existing session's expiration.
pub async fn create_elevated_session(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
    ip_address: &str,
    duration_minutes: i64,
    reason: Option<&str>,
) -> sqlx::Result<ElevatedSession> {
    let expires_at = Utc::now() + Duration::minutes(duration_minutes);

    sqlx::query_as::<_, ElevatedSession>(
        r"
        INSERT INTO elevated_sessions (user_id, session_id, ip_address, expires_at, reason)
        VALUES ($1, $2, $3::inet, $4, $5)
        ON CONFLICT (session_id) DO UPDATE
        SET expires_at = EXCLUDED.expires_at,
            ip_address = EXCLUDED.ip_address,
            reason = EXCLUDED.reason
        RETURNING
            id,
            user_id,
            session_id,
            host(ip_address) as ip_address,
            elevated_at,
            expires_at,
            reason
        ",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(ip_address)
    .bind(expires_at)
    .bind(reason)
    .fetch_one(pool)
    .await
}

/// Delete an elevated session.
///
/// Returns `true` if a session was deleted, `false` if no session existed.
pub async fn delete_elevated_session(pool: &PgPool, session_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM elevated_sessions WHERE session_id = $1")
        .bind(session_id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Check if a session is currently elevated.
pub async fn is_session_elevated(pool: &PgPool, session_id: Uuid) -> sqlx::Result<bool> {
    let result: (bool,) = sqlx::query_as(
        r"
        SELECT EXISTS(
            SELECT 1 FROM elevated_sessions
            WHERE session_id = $1
              AND expires_at > NOW()
        )
        ",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;

    Ok(result.0)
}

// ============================================================================
// Guild Role Queries
// ============================================================================

/// Get all roles for a guild, ordered by position (ascending).
pub async fn get_guild_roles(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<Vec<GuildRole>> {
    sqlx::query_as::<_, GuildRole>(
        r"
        SELECT id, guild_id, name, color, permissions, position, is_default, created_at, updated_at
        FROM guild_roles
        WHERE guild_id = $1
        ORDER BY position ASC
        ",
    )
    .bind(guild_id)
    .fetch_all(pool)
    .await
}

/// Get a specific guild role by ID.
pub async fn get_guild_role(pool: &PgPool, role_id: Uuid) -> sqlx::Result<Option<GuildRole>> {
    sqlx::query_as::<_, GuildRole>(
        r"
        SELECT id, guild_id, name, color, permissions, position, is_default, created_at, updated_at
        FROM guild_roles
        WHERE id = $1
        ",
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await
}

/// Get the @everyone role for a guild.
pub async fn get_everyone_role(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<Option<GuildRole>> {
    sqlx::query_as::<_, GuildRole>(
        r"
        SELECT id, guild_id, name, color, permissions, position, is_default, created_at, updated_at
        FROM guild_roles
        WHERE guild_id = $1
          AND is_default = true
        ",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await
}

/// Create a new guild role.
pub async fn create_guild_role(
    pool: &PgPool,
    guild_id: Uuid,
    name: &str,
    color: Option<&str>,
    permissions: GuildPermissions,
    position: i32,
    is_default: bool,
) -> sqlx::Result<GuildRole> {
    sqlx::query_as::<_, GuildRole>(
        r"
        INSERT INTO guild_roles (guild_id, name, color, permissions, position, is_default)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id, guild_id, name, color, permissions, position, is_default, created_at, updated_at
        ",
    )
    .bind(guild_id)
    .bind(name)
    .bind(color)
    .bind(permissions.to_db())
    .bind(position)
    .bind(is_default)
    .fetch_one(pool)
    .await
}

/// Update a guild role.
///
/// Uses COALESCE to only update provided fields.
pub async fn update_guild_role(
    pool: &PgPool,
    role_id: Uuid,
    name: Option<&str>,
    color: Option<&str>,
    permissions: Option<GuildPermissions>,
    position: Option<i32>,
) -> sqlx::Result<Option<GuildRole>> {
    let permissions_db = permissions.map(|p| p.to_db());

    sqlx::query_as::<_, GuildRole>(
        r"
        UPDATE guild_roles
        SET name = COALESCE($2, name),
            color = COALESCE($3, color),
            permissions = COALESCE($4, permissions),
            position = COALESCE($5, position),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, guild_id, name, color, permissions, position, is_default, created_at, updated_at
        ",
    )
    .bind(role_id)
    .bind(name)
    .bind(color)
    .bind(permissions_db)
    .bind(position)
    .fetch_optional(pool)
    .await
}

/// Delete a guild role.
///
/// Cannot delete the default (@everyone) role.
/// Returns `true` if a role was deleted, `false` if not found or is default.
pub async fn delete_guild_role(pool: &PgPool, role_id: Uuid) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r"
        DELETE FROM guild_roles
        WHERE id = $1
          AND is_default = false
        ",
    )
    .bind(role_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Create default roles for a new guild.
///
/// Creates:
/// - @everyone (position 1000, is_default = true)
/// - Moderator (position 100)
/// - Officer (position 50)
pub async fn create_default_roles(pool: &PgPool, guild_id: Uuid) -> sqlx::Result<()> {
    // Create @everyone role
    sqlx::query(
        r"
        INSERT INTO guild_roles (guild_id, name, color, permissions, position, is_default)
        VALUES ($1, '@everyone', NULL, $2, 1000, true)
        ",
    )
    .bind(guild_id)
    .bind(GuildPermissions::EVERYONE_DEFAULT.to_db())
    .execute(pool)
    .await?;

    // Create Moderator role
    sqlx::query(
        r"
        INSERT INTO guild_roles (guild_id, name, color, permissions, position, is_default)
        VALUES ($1, 'Moderator', '#3498db', $2, 100, false)
        ",
    )
    .bind(guild_id)
    .bind(GuildPermissions::MODERATOR_DEFAULT.to_db())
    .execute(pool)
    .await?;

    // Create Officer role
    sqlx::query(
        r"
        INSERT INTO guild_roles (guild_id, name, color, permissions, position, is_default)
        VALUES ($1, 'Officer', '#e74c3c', $2, 50, false)
        ",
    )
    .bind(guild_id)
    .bind(GuildPermissions::OFFICER_DEFAULT.to_db())
    .execute(pool)
    .await?;

    Ok(())
}

// ============================================================================
// Guild Member Role Queries
// ============================================================================

/// Get all roles for a guild member.
pub async fn get_member_roles(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Vec<GuildRole>> {
    sqlx::query_as::<_, GuildRole>(
        r"
        SELECT r.id, r.guild_id, r.name, r.color, r.permissions, r.position, r.is_default, r.created_at, r.updated_at
        FROM guild_roles r
        INNER JOIN guild_member_roles gmr ON gmr.role_id = r.id
        WHERE gmr.guild_id = $1
          AND gmr.user_id = $2
        ORDER BY r.position ASC
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// Assign a role to a guild member.
///
/// Uses ON CONFLICT DO NOTHING to silently ignore duplicate assignments.
pub async fn assign_member_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
    assigned_by: Option<Uuid>,
) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO guild_member_roles (guild_id, user_id, role_id, assigned_by)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (guild_id, user_id, role_id) DO NOTHING
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .bind(assigned_by)
    .execute(pool)
    .await?;

    Ok(())
}

/// Remove a role from a guild member.
///
/// Returns `true` if a role was removed, `false` if the member didn't have the role.
pub async fn remove_member_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    role_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r"
        DELETE FROM guild_member_roles
        WHERE guild_id = $1
          AND user_id = $2
          AND role_id = $3
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .bind(role_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Get the highest (lowest position number) role for a member.
///
/// Returns `None` if the member has no roles.
pub async fn get_member_highest_role(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Option<i32>> {
    let result: Option<i32> = sqlx::query_scalar(
        r"
        SELECT MIN(r.position)
        FROM guild_roles r
        INNER JOIN guild_member_roles gmr ON gmr.role_id = r.id
        WHERE gmr.guild_id = $1
          AND gmr.user_id = $2
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(result)
}

// ============================================================================
// Channel Override Queries
// ============================================================================

/// Get all permission overrides for a channel.
pub async fn get_channel_overrides(
    pool: &PgPool,
    channel_id: Uuid,
) -> sqlx::Result<Vec<ChannelOverride>> {
    sqlx::query_as::<_, ChannelOverride>(
        r"
        SELECT id, channel_id, role_id, allow_permissions, deny_permissions
        FROM channel_overrides
        WHERE channel_id = $1
        ",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await
}

/// Set or update a channel permission override.
///
/// Uses ON CONFLICT UPDATE to upsert the override.
pub async fn set_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
    allow: GuildPermissions,
    deny: GuildPermissions,
) -> sqlx::Result<ChannelOverride> {
    sqlx::query_as::<_, ChannelOverride>(
        r"
        INSERT INTO channel_overrides (channel_id, role_id, allow_permissions, deny_permissions)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (channel_id, role_id) DO UPDATE
        SET allow_permissions = EXCLUDED.allow_permissions,
            deny_permissions = EXCLUDED.deny_permissions
        RETURNING id, channel_id, role_id, allow_permissions, deny_permissions
        ",
    )
    .bind(channel_id)
    .bind(role_id)
    .bind(allow.to_db())
    .bind(deny.to_db())
    .fetch_one(pool)
    .await
}

/// Delete a channel permission override.
///
/// Returns `true` if an override was deleted, `false` if not found.
pub async fn delete_channel_override(
    pool: &PgPool,
    channel_id: Uuid,
    role_id: Uuid,
) -> sqlx::Result<bool> {
    let result = sqlx::query(
        r"
        DELETE FROM channel_overrides
        WHERE channel_id = $1
          AND role_id = $2
        ",
    )
    .bind(channel_id)
    .bind(role_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

// ============================================================================
// Audit Log Queries
// ============================================================================

/// Write an entry to the system audit log.
pub async fn write_audit_log(
    pool: &PgPool,
    actor_id: Uuid,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<Uuid>,
    details: Option<JsonValue>,
    ip_address: Option<&str>,
) -> sqlx::Result<AuditLogEntry> {
    sqlx::query_as::<_, AuditLogEntry>(
        r"
        INSERT INTO system_audit_log (actor_id, action, target_type, target_id, details, ip_address)
        VALUES ($1, $2, $3, $4, $5, $6::inet)
        RETURNING
            id,
            actor_id,
            action,
            target_type,
            target_id,
            details,
            host(ip_address) as ip_address,
            created_at
        ",
    )
    .bind(actor_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(details)
    .bind(ip_address)
    .fetch_one(pool)
    .await
}

/// Get audit log entries with pagination and optional action filter.
///
/// If `action_filter` is provided, only entries with actions starting with that prefix are returned.
pub async fn get_audit_log(
    pool: &PgPool,
    limit: i64,
    offset: i64,
    action_filter: Option<&str>,
) -> sqlx::Result<Vec<AuditLogEntry>> {
    if let Some(filter) = action_filter {
        let pattern = format!("{}%", filter);
        sqlx::query_as::<_, AuditLogEntry>(
            r"
            SELECT
                id,
                actor_id,
                action,
                target_type,
                target_id,
                details,
                host(ip_address) as ip_address,
                created_at
            FROM system_audit_log
            WHERE action LIKE $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            ",
        )
        .bind(pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, AuditLogEntry>(
            r"
            SELECT
                id,
                actor_id,
                action,
                target_type,
                target_id,
                details,
                host(ip_address) as ip_address,
                created_at
            FROM system_audit_log
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            ",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests verify SQL syntax at compile time for runtime queries
    // Integration tests with a real database would be in server/tests/

    #[test]
    fn test_guild_permissions_to_db() {
        // Verify permission conversion works correctly
        let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
        let db_value = perms.to_db();
        assert!(db_value > 0);

        let restored = GuildPermissions::from_db(db_value);
        assert_eq!(perms, restored);
    }

    #[test]
    fn test_everyone_default_to_db() {
        let db_value = GuildPermissions::EVERYONE_DEFAULT.to_db();
        assert!(db_value > 0);
    }

    #[test]
    fn test_moderator_default_to_db() {
        let db_value = GuildPermissions::MODERATOR_DEFAULT.to_db();
        assert!(db_value > 0);
    }

    #[test]
    fn test_officer_default_to_db() {
        let db_value = GuildPermissions::OFFICER_DEFAULT.to_db();
        assert!(db_value > 0);
    }
}
