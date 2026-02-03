//! Permission helper functions for API handlers.
//!
//! Provides convenience functions to load and check permissions in a single operation.

use sqlx::PgPool;
use uuid::Uuid;

use super::guild::GuildPermissions;
use super::models::GuildRole;
use super::resolver::{compute_guild_permissions, PermissionError};

/// Pre-computed permission context for a guild member.
///
/// Contains all the information needed to perform permission checks
/// without additional database queries.
#[derive(Debug, Clone)]
pub struct MemberPermissionContext {
    /// The guild owner's user ID.
    pub guild_owner_id: Uuid,

    /// Permissions from the @everyone role.
    pub everyone_permissions: GuildPermissions,

    /// All roles assigned to this member (excluding @everyone).
    pub member_roles: Vec<GuildRole>,

    /// Pre-computed permissions for this member.
    pub computed_permissions: GuildPermissions,

    /// The highest role position (lowest number = highest rank).
    /// `None` if the member has no assigned roles.
    pub highest_role_position: Option<i32>,

    /// Whether this member is the guild owner.
    pub is_owner: bool,
}

impl MemberPermissionContext {
    /// Check if the member has the specified permission.
    #[must_use]
    pub const fn has_permission(&self, permission: GuildPermissions) -> bool {
        self.computed_permissions.has(permission)
    }

    /// Require that the member has the specified permission.
    ///
    /// Returns `Ok(())` if the permission is present, or `Err(PermissionError::MissingPermission)`.
    pub const fn require_permission(
        &self,
        permission: GuildPermissions,
    ) -> Result<(), PermissionError> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(PermissionError::MissingPermission(permission))
        }
    }
}

/// Load permission context for a guild member.
///
/// Performs a single query to fetch:
/// - Guild owner ID
/// - @everyone role permissions
/// - All roles assigned to the member
///
/// Returns `None` if the user is not a member of the guild.
///
/// # Example
///
/// ```ignore
/// let ctx = get_member_permission_context(&pool, guild_id, user_id).await?;
/// if let Some(ctx) = ctx {
///     if ctx.has_permission(GuildPermissions::MANAGE_MESSAGES) {
///         // User can manage messages
///     }
/// }
/// ```
#[tracing::instrument(skip(pool))]
pub async fn get_member_permission_context(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
) -> sqlx::Result<Option<MemberPermissionContext>> {
    // First, verify the user is a member of the guild and get guild info
    let guild_info: Option<GuildInfo> = sqlx::query_as(
        r"
        SELECT g.owner_id as guild_owner_id
        FROM guilds g
        INNER JOIN guild_members gm ON gm.guild_id = g.id
        WHERE g.id = $1 AND gm.user_id = $2
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    let Some(guild_info) = guild_info else {
        return Ok(None);
    };

    let is_owner = guild_info.guild_owner_id == user_id;

    // Get @everyone role permissions
    let everyone_role: Option<GuildRole> = sqlx::query_as(
        r"
        SELECT id, guild_id, name, color, permissions, position, is_default, created_at, updated_at
        FROM guild_roles
        WHERE guild_id = $1 AND is_default = true
        ",
    )
    .bind(guild_id)
    .fetch_optional(pool)
    .await?;

    let everyone_permissions = everyone_role.map(|r| r.permissions).unwrap_or_default();

    // Get all roles assigned to the member
    let member_roles: Vec<GuildRole> = sqlx::query_as(
        r"
        SELECT r.id, r.guild_id, r.name, r.color, r.permissions, r.position, r.is_default, r.created_at, r.updated_at
        FROM guild_roles r
        INNER JOIN guild_member_roles gmr ON gmr.role_id = r.id
        WHERE gmr.guild_id = $1 AND gmr.user_id = $2
        ORDER BY r.position ASC
        ",
    )
    .bind(guild_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    // Compute highest role position (lowest number = highest rank)
    let highest_role_position = member_roles.iter().map(|r| r.position).min();

    // Compute permissions
    let computed_permissions = compute_guild_permissions(
        user_id,
        guild_info.guild_owner_id,
        everyone_permissions,
        &member_roles,
        None, // No channel overrides for guild-level context
    );

    Ok(Some(MemberPermissionContext {
        guild_owner_id: guild_info.guild_owner_id,
        everyone_permissions,
        member_roles,
        computed_permissions,
        highest_role_position,
        is_owner,
    }))
}

/// Load permission context and require a specific permission.
///
/// Convenience function that combines `get_member_permission_context`
/// with a permission check. Returns an error if:
/// - The user is not a member of the guild (`NotGuildMember`)
/// - The user lacks the required permission (`MissingPermission`)
///
/// # Example
///
/// ```ignore
/// // In an API handler:
/// let ctx = require_guild_permission(&pool, guild_id, user_id, GuildPermissions::MANAGE_CHANNELS)
///     .await
///     .map_err(|e| (StatusCode::FORBIDDEN, e.to_string()))?;
/// ```
#[tracing::instrument(skip(pool))]
pub async fn require_guild_permission(
    pool: &PgPool,
    guild_id: Uuid,
    user_id: Uuid,
    required_permission: GuildPermissions,
) -> Result<MemberPermissionContext, PermissionError> {
    let ctx = get_member_permission_context(pool, guild_id, user_id)
        .await
        .map_err(|e| PermissionError::DatabaseError(e.to_string()))?
        .ok_or(PermissionError::NotGuildMember)?;

    ctx.require_permission(required_permission)?;

    Ok(ctx)
}

/// Check if member can view a specific channel.
/// Returns the member's permission context if authorized, or error if forbidden.
///
/// **Resolution order:**
/// 1. Guild owner → full access
/// 2. DM channels → always accessible to participants
/// 3. Guild channels → `VIEW_CHANNEL` permission required
///
/// # Errors
///
/// Returns:
/// - `PermissionError::NotFound` if channel doesn't exist
/// - `PermissionError::InvalidChannel` if guild channel missing `guild_id`
/// - `PermissionError::Forbidden` if user is not DM participant
/// - `PermissionError::NotGuildMember` if user not in guild
/// - `PermissionError::MissingPermission` if user lacks `VIEW_CHANNEL`
/// - `PermissionError::DatabaseError` on database errors
#[tracing::instrument(skip(pool))]
pub async fn require_channel_access(
    pool: &PgPool,
    user_id: Uuid,
    channel_id: Uuid,
) -> Result<MemberPermissionContext, PermissionError> {
    // Get channel info
    let channel = crate::db::get_channel_by_id(pool, channel_id)
        .await
        .map_err(|e| PermissionError::DatabaseError(e.to_string()))?
        .ok_or(PermissionError::NotFound)?;

    // DM channels: check participation
    if channel.channel_type == crate::db::ChannelType::Dm {
        let is_participant = crate::db::is_dm_participant(pool, channel_id, user_id)
            .await
            .map_err(|e| PermissionError::DatabaseError(e.to_string()))?;

        if !is_participant {
            return Err(PermissionError::Forbidden);
        }

        // DM participants always have access
        // Return a minimal context for DMs (no guild, no roles)
        return Ok(MemberPermissionContext {
            guild_owner_id: Uuid::nil(), // No owner for DMs
            everyone_permissions: GuildPermissions::empty(),
            member_roles: vec![],
            computed_permissions: GuildPermissions::all(), // Full access to participant
            highest_role_position: None,
            is_owner: false,
        });
    }

    // Guild channels: check VIEW_CHANNEL permission
    let guild_id = channel.guild_id.ok_or(PermissionError::InvalidChannel)?;

    let ctx = get_member_permission_context(pool, guild_id, user_id)
        .await
        .map_err(|e| PermissionError::DatabaseError(e.to_string()))?
        .ok_or(PermissionError::NotGuildMember)?;

    // Owner bypass
    if ctx.is_owner {
        return Ok(ctx);
    }

    // Get channel overrides
    let overrides = crate::db::get_channel_overrides(pool, channel_id)
        .await
        .map_err(|e| PermissionError::DatabaseError(e.to_string()))?;

    // Compute channel-specific permissions
    let perms = compute_guild_permissions(
        user_id,
        ctx.guild_owner_id,
        ctx.everyone_permissions,
        &ctx.member_roles,
        Some(&overrides),
    );

    if !perms.has(GuildPermissions::VIEW_CHANNEL) {
        return Err(PermissionError::MissingPermission(
            GuildPermissions::VIEW_CHANNEL,
        ));
    }

    // Return context with updated permissions that include channel overrides
    Ok(MemberPermissionContext {
        computed_permissions: perms,
        ..ctx
    })
}

/// Internal struct for guild membership query.
#[derive(Debug, sqlx::FromRow)]
struct GuildInfo {
    guild_owner_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_permission_context_has_permission() {
        let ctx = MemberPermissionContext {
            guild_owner_id: Uuid::new_v4(),
            everyone_permissions: GuildPermissions::SEND_MESSAGES,
            member_roles: vec![],
            computed_permissions: GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT,
            highest_role_position: None,
            is_owner: false,
        };

        assert!(ctx.has_permission(GuildPermissions::SEND_MESSAGES));
        assert!(ctx.has_permission(GuildPermissions::VOICE_CONNECT));
        assert!(!ctx.has_permission(GuildPermissions::BAN_MEMBERS));
    }

    #[test]
    fn test_member_permission_context_require_permission_success() {
        let ctx = MemberPermissionContext {
            guild_owner_id: Uuid::new_v4(),
            everyone_permissions: GuildPermissions::SEND_MESSAGES,
            member_roles: vec![],
            computed_permissions: GuildPermissions::SEND_MESSAGES | GuildPermissions::MANAGE_ROLES,
            highest_role_position: Some(50),
            is_owner: false,
        };

        assert!(ctx
            .require_permission(GuildPermissions::SEND_MESSAGES)
            .is_ok());
        assert!(ctx
            .require_permission(GuildPermissions::MANAGE_ROLES)
            .is_ok());
    }

    #[test]
    fn test_member_permission_context_require_permission_failure() {
        let ctx = MemberPermissionContext {
            guild_owner_id: Uuid::new_v4(),
            everyone_permissions: GuildPermissions::SEND_MESSAGES,
            member_roles: vec![],
            computed_permissions: GuildPermissions::SEND_MESSAGES,
            highest_role_position: None,
            is_owner: false,
        };

        let result = ctx.require_permission(GuildPermissions::BAN_MEMBERS);
        assert!(matches!(result, Err(PermissionError::MissingPermission(_))));
    }

    #[test]
    fn test_owner_context() {
        let owner_id = Uuid::new_v4();
        let ctx = MemberPermissionContext {
            guild_owner_id: owner_id,
            everyone_permissions: GuildPermissions::SEND_MESSAGES,
            member_roles: vec![],
            computed_permissions: GuildPermissions::all(), // Owners have all permissions
            highest_role_position: None,
            is_owner: true,
        };

        assert!(ctx.is_owner);
        assert!(ctx.has_permission(GuildPermissions::TRANSFER_OWNERSHIP));
        assert!(ctx.has_permission(GuildPermissions::MANAGE_GUILD));
    }

    #[test]
    fn test_highest_role_position() {
        // No roles assigned
        let ctx_no_roles = MemberPermissionContext {
            guild_owner_id: Uuid::new_v4(),
            everyone_permissions: GuildPermissions::empty(),
            member_roles: vec![],
            computed_permissions: GuildPermissions::empty(),
            highest_role_position: None,
            is_owner: false,
        };
        assert_eq!(ctx_no_roles.highest_role_position, None);

        // With roles
        let ctx_with_roles = MemberPermissionContext {
            guild_owner_id: Uuid::new_v4(),
            everyone_permissions: GuildPermissions::empty(),
            member_roles: vec![], // Roles would be here but we just test the position
            computed_permissions: GuildPermissions::KICK_MEMBERS,
            highest_role_position: Some(50),
            is_owner: false,
        };
        assert_eq!(ctx_with_roles.highest_role_position, Some(50));
    }

    // === Tests for require_channel_access ===
    // Note: These are integration tests that require a database connection.
    // They should be run in server/tests/ with a test database.
    //
    // Test scenarios to implement:
    //
    // 1. test_require_channel_access_owner()
    //    - Guild owner can view any channel
    //
    // 2. test_require_channel_access_allowed()
    //    - User with VIEW_CHANNEL permission can access
    //
    // 3. test_require_channel_access_denied()
    //    - User without VIEW_CHANNEL gets PermissionError::MissingPermission
    //
    // 4. test_require_channel_access_dm_participant()
    //    - DM participant can access DM channel
    //    - Returns context with full permissions
    //
    // 5. test_require_channel_access_dm_non_participant()
    //    - Non-participant cannot access DM
    //    - Gets PermissionError::Forbidden
    //
    // 6. test_require_channel_access_channel_override_deny()
    //    - User with role permission but channel deny → Forbidden
    //    - Channel overrides take precedence
    //
    // 7. test_require_channel_access_channel_override_allow()
    //    - User without role permission but channel allow → Success
    //    - Channel overrides grant access
    //
    // 8. test_require_channel_access_not_found()
    //    - Non-existent channel returns PermissionError::NotFound
    //
    // 9. test_require_channel_access_invalid_channel()
    //    - Guild channel without guild_id returns PermissionError::InvalidChannel
    //
    // 10. test_require_channel_access_not_guild_member()
    //    - User not in guild gets PermissionError::NotGuildMember
}
