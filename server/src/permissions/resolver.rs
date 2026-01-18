//! Permission resolution logic.
//!
//! Computes effective permissions for a user in a guild/channel context.

use uuid::Uuid;

use super::guild::GuildPermissions;
use super::models::{ChannelOverride, GuildRole};

/// Compute guild permissions for a user.
///
/// Resolution order:
/// 1. Guild owner has all permissions
/// 2. Start with @everyone role permissions
/// 3. Add permissions from assigned roles (by position)
/// 4. Apply channel overrides if channel context provided
pub fn compute_guild_permissions(
    user_id: Uuid,
    guild_owner_id: Uuid,
    everyone_permissions: GuildPermissions,
    user_roles: &[GuildRole],
    channel_overrides: Option<&[ChannelOverride]>,
) -> GuildPermissions {
    // Guild owner has everything
    if guild_owner_id == user_id {
        return GuildPermissions::all();
    }

    // Start with @everyone permissions
    let mut perms = everyone_permissions;

    // Add role permissions (sorted by position, lower number = higher rank)
    let mut sorted_roles: Vec<_> = user_roles.iter().collect();
    sorted_roles.sort_by_key(|r| r.position);

    for role in sorted_roles {
        perms |= role.permissions;
    }

    // Apply channel overrides if provided
    if let Some(overrides) = channel_overrides {
        for role in user_roles {
            if let Some(ovr) = overrides.iter().find(|o| o.role_id == role.id) {
                perms |= ovr.allow_permissions;
                perms &= !ovr.deny_permissions; // Deny wins
            }
        }
    }

    perms
}

/// Check if a user can manage a target role.
///
/// Rules:
/// 1. Must have `MANAGE_ROLES` permission
/// 2. Cannot edit roles at or above your position
/// 3. Cannot grant permissions you don't have
pub fn can_manage_role(
    actor_permissions: GuildPermissions,
    actor_highest_position: i32,
    target_role_position: i32,
    new_permissions: Option<GuildPermissions>,
) -> Result<(), PermissionError> {
    // Must have MANAGE_ROLES
    if !actor_permissions.has(GuildPermissions::MANAGE_ROLES) {
        return Err(PermissionError::MissingPermission(
            GuildPermissions::MANAGE_ROLES,
        ));
    }

    // Cannot edit roles at or above your position (lower number = higher rank)
    if target_role_position <= actor_highest_position {
        return Err(PermissionError::RoleHierarchy {
            actor_position: actor_highest_position,
            target_position: target_role_position,
        });
    }

    // Cannot grant permissions you don't have
    if let Some(new_perms) = new_permissions {
        let escalation = new_perms & !actor_permissions;
        if !escalation.is_empty() {
            return Err(PermissionError::CannotEscalate(escalation));
        }
    }

    Ok(())
}

/// Check if a user can moderate a target member.
///
/// Rules:
/// 1. Cannot moderate guild owner
/// 2. Cannot moderate someone with higher/equal role
pub const fn can_moderate_member(
    actor_highest_position: i32,
    target_highest_position: i32,
    target_is_owner: bool,
) -> Result<(), PermissionError> {
    // Cannot moderate guild owner
    if target_is_owner {
        return Err(PermissionError::CannotModerateOwner);
    }

    // Cannot moderate someone with higher/equal role (lower number = higher rank)
    if target_highest_position <= actor_highest_position {
        return Err(PermissionError::RoleHierarchy {
            actor_position: actor_highest_position,
            target_position: target_highest_position,
        });
    }

    Ok(())
}

/// Permission check errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionError {
    /// User lacks required permission.
    MissingPermission(GuildPermissions),

    /// Role hierarchy violation.
    RoleHierarchy {
        actor_position: i32,
        target_position: i32,
    },

    /// Attempted to grant permissions not held.
    CannotEscalate(GuildPermissions),

    /// Attempted to moderate guild owner.
    CannotModerateOwner,

    /// User is not a member of the guild.
    NotGuildMember,

    /// Action requires elevated session.
    ElevationRequired,

    /// User is not a system admin.
    NotSystemAdmin,

    /// Database error occurred.
    DatabaseError(String),
}

impl std::fmt::Display for PermissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPermission(p) => write!(f, "Missing permission: {p:?}"),
            Self::RoleHierarchy {
                actor_position,
                target_position,
            } => write!(
                f,
                "Cannot modify role at position {target_position} (your position: {actor_position})"
            ),
            Self::CannotEscalate(p) => {
                write!(f, "Cannot grant permissions you don't have: {p:?}")
            }
            Self::CannotModerateOwner => write!(f, "Cannot moderate guild owner"),
            Self::NotGuildMember => write!(f, "User is not a member of this guild"),
            Self::ElevationRequired => {
                write!(f, "This action requires an elevated session")
            }
            Self::NotSystemAdmin => write!(f, "User is not a system admin"),
            Self::DatabaseError(msg) => write!(f, "Database error: {msg}"),
        }
    }
}

impl std::error::Error for PermissionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_owner_has_all_permissions() {
        let owner_id = Uuid::new_v4();
        let perms =
            compute_guild_permissions(owner_id, owner_id, GuildPermissions::empty(), &[], None);
        assert_eq!(perms, GuildPermissions::all());
    }

    #[test]
    fn test_everyone_permissions_applied() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let everyone = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;

        let perms = compute_guild_permissions(user_id, owner_id, everyone, &[], None);

        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(perms.has(GuildPermissions::VOICE_CONNECT));
        assert!(!perms.has(GuildPermissions::KICK_MEMBERS));
    }

    #[test]
    fn test_role_permissions_combined() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let everyone = GuildPermissions::SEND_MESSAGES;

        let mod_role = GuildRole {
            id: Uuid::new_v4(),
            guild_id: Uuid::new_v4(),
            name: "Moderator".to_string(),
            color: None,
            permissions: GuildPermissions::MANAGE_MESSAGES | GuildPermissions::TIMEOUT_MEMBERS,
            position: 100,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let perms = compute_guild_permissions(user_id, owner_id, everyone, &[mod_role], None);

        assert!(perms.has(GuildPermissions::SEND_MESSAGES)); // from everyone
        assert!(perms.has(GuildPermissions::MANAGE_MESSAGES)); // from role
        assert!(perms.has(GuildPermissions::TIMEOUT_MEMBERS)); // from role
    }

    #[test]
    fn test_channel_override_deny_wins() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();

        let everyone = GuildPermissions::SEND_MESSAGES | GuildPermissions::EMBED_LINKS;

        let role = GuildRole {
            id: role_id,
            guild_id: Uuid::new_v4(),
            name: "Member".to_string(),
            color: None,
            permissions: GuildPermissions::empty(),
            position: 999,
            is_default: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let override_entry = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id,
            allow_permissions: GuildPermissions::ATTACH_FILES,
            deny_permissions: GuildPermissions::SEND_MESSAGES,
        };

        let perms = compute_guild_permissions(
            user_id,
            owner_id,
            everyone,
            &[role],
            Some(&[override_entry]),
        );

        assert!(!perms.has(GuildPermissions::SEND_MESSAGES)); // denied by override
        assert!(perms.has(GuildPermissions::EMBED_LINKS)); // still from everyone
        assert!(perms.has(GuildPermissions::ATTACH_FILES)); // allowed by override
    }

    #[test]
    fn test_can_manage_role_hierarchy() {
        let perms = GuildPermissions::MANAGE_ROLES | GuildPermissions::KICK_MEMBERS;

        // Can manage lower role
        assert!(can_manage_role(perms, 50, 100, None).is_ok());

        // Cannot manage equal position
        assert!(can_manage_role(perms, 50, 50, None).is_err());

        // Cannot manage higher role
        assert!(can_manage_role(perms, 50, 10, None).is_err());
    }

    #[test]
    fn test_cannot_escalate_permissions() {
        let actor_perms = GuildPermissions::MANAGE_ROLES | GuildPermissions::KICK_MEMBERS;
        let new_perms = GuildPermissions::KICK_MEMBERS | GuildPermissions::BAN_MEMBERS;

        // Actor doesn't have BAN_MEMBERS, so this should fail
        let result = can_manage_role(actor_perms, 50, 100, Some(new_perms));
        assert!(matches!(result, Err(PermissionError::CannotEscalate(_))));
    }

    #[test]
    fn test_cannot_moderate_owner() {
        let result = can_moderate_member(50, 1, true);
        assert!(matches!(result, Err(PermissionError::CannotModerateOwner)));
    }

    #[test]
    fn test_can_moderate_lower_ranked_member() {
        // Actor at position 50 can moderate member at position 100 (lower rank)
        let result = can_moderate_member(50, 100, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cannot_moderate_equal_ranked_member() {
        let result = can_moderate_member(50, 50, false);
        assert!(matches!(result, Err(PermissionError::RoleHierarchy { .. })));
    }

    #[test]
    fn test_cannot_moderate_higher_ranked_member() {
        let result = can_moderate_member(50, 10, false);
        assert!(matches!(result, Err(PermissionError::RoleHierarchy { .. })));
    }

    #[test]
    fn test_missing_manage_roles_permission() {
        let perms = GuildPermissions::KICK_MEMBERS; // No MANAGE_ROLES

        let result = can_manage_role(perms, 50, 100, None);
        assert!(matches!(result, Err(PermissionError::MissingPermission(_))));
    }

    #[test]
    fn test_multiple_roles_permissions_combined() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let guild_id = Uuid::new_v4();
        let everyone = GuildPermissions::empty();

        let role1 = GuildRole {
            id: Uuid::new_v4(),
            guild_id,
            name: "Role1".to_string(),
            color: None,
            permissions: GuildPermissions::SEND_MESSAGES,
            position: 100,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let role2 = GuildRole {
            id: Uuid::new_v4(),
            guild_id,
            name: "Role2".to_string(),
            color: None,
            permissions: GuildPermissions::VOICE_CONNECT,
            position: 50,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let perms = compute_guild_permissions(user_id, owner_id, everyone, &[role1, role2], None);

        // Should have permissions from both roles
        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(perms.has(GuildPermissions::VOICE_CONNECT));
    }

    #[test]
    fn test_permission_error_display() {
        let missing = PermissionError::MissingPermission(GuildPermissions::MANAGE_ROLES);
        assert!(missing.to_string().contains("Missing permission"));

        let hierarchy = PermissionError::RoleHierarchy {
            actor_position: 50,
            target_position: 10,
        };
        assert!(hierarchy.to_string().contains("position"));

        let escalate = PermissionError::CannotEscalate(GuildPermissions::BAN_MEMBERS);
        assert!(escalate.to_string().contains("Cannot grant"));

        let owner = PermissionError::CannotModerateOwner;
        assert!(owner.to_string().contains("guild owner"));

        let not_member = PermissionError::NotGuildMember;
        assert!(not_member.to_string().contains("not a member"));

        let elevation = PermissionError::ElevationRequired;
        assert!(elevation.to_string().contains("elevated session"));

        let not_admin = PermissionError::NotSystemAdmin;
        assert!(not_admin.to_string().contains("not a system admin"));

        let db_error = PermissionError::DatabaseError("connection refused".to_string());
        assert!(db_error.to_string().contains("Database error"));
        assert!(db_error.to_string().contains("connection refused"));
    }

    #[test]
    fn test_can_grant_permissions_you_have() {
        let actor_perms = GuildPermissions::MANAGE_ROLES
            | GuildPermissions::KICK_MEMBERS
            | GuildPermissions::BAN_MEMBERS;
        let new_perms = GuildPermissions::KICK_MEMBERS | GuildPermissions::BAN_MEMBERS;

        // Actor has all permissions being granted, so this should succeed
        let result = can_manage_role(actor_perms, 50, 100, Some(new_perms));
        assert!(result.is_ok());
    }

    #[test]
    fn test_channel_override_multiple_roles() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let guild_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let role1_id = Uuid::new_v4();
        let role2_id = Uuid::new_v4();

        let everyone = GuildPermissions::SEND_MESSAGES;

        let role1 = GuildRole {
            id: role1_id,
            guild_id,
            name: "Role1".to_string(),
            color: None,
            permissions: GuildPermissions::VOICE_CONNECT,
            position: 100,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let role2 = GuildRole {
            id: role2_id,
            guild_id,
            name: "Role2".to_string(),
            color: None,
            permissions: GuildPermissions::EMBED_LINKS,
            position: 50,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let override1 = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id: role1_id,
            allow_permissions: GuildPermissions::ATTACH_FILES,
            deny_permissions: GuildPermissions::empty(),
        };

        let override2 = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id: role2_id,
            allow_permissions: GuildPermissions::empty(),
            deny_permissions: GuildPermissions::SEND_MESSAGES,
        };

        let perms = compute_guild_permissions(
            user_id,
            owner_id,
            everyone,
            &[role1, role2],
            Some(&[override1, override2]),
        );

        // SEND_MESSAGES denied by role2's override
        assert!(!perms.has(GuildPermissions::SEND_MESSAGES));
        // ATTACH_FILES allowed by role1's override
        assert!(perms.has(GuildPermissions::ATTACH_FILES));
        // VOICE_CONNECT from role1
        assert!(perms.has(GuildPermissions::VOICE_CONNECT));
        // EMBED_LINKS from role2
        assert!(perms.has(GuildPermissions::EMBED_LINKS));
    }
}
