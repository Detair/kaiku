//! Permission resolution logic.
//!
//! Computes effective permissions for a user in a guild/channel context.

use uuid::Uuid;

use super::guild::GuildPermissions;
use super::models::{ChannelOverride, GuildRole};

/// Compute a member's effective permissions in a guild, or in one of its
/// channels when `channel_overrides` is given.
///
/// This is the single place that resolves channel overrides; every
/// visibility and permission check goes through it. Resolution order:
///
/// 1. The guild owner has every permission; a `pending` member (screening rules not accepted yet)
///    has none, before any override is looked at.
/// 2. Guild base: `@everyone` permissions OR the permissions of every assigned role.
/// 3. The `@everyone` channel override: its allow (masked with
///    [`GuildPermissions::EVERYONE_ALLOWED`]), then its deny.
/// 4. The member's role overrides, aggregated: all role denies OR-ed and applied, then all role
///    allows OR-ed and applied. A role allow therefore beats both an `@everyone` deny and another
///    role's deny of the same bit.
///
/// The `@everyone` override is recognised by its own
/// [`ChannelOverride::everyone`] flag (loaded from the role), or by matching an
/// `is_default` entry of `user_roles`; it is applied exactly once, in step 3,
/// and never takes part in the role aggregation. Overrides only exist per role
/// (there are no per-member overrides).
pub fn compute_guild_permissions(
    user_id: Uuid,
    guild_owner_id: Uuid,
    pending: bool,
    everyone_permissions: GuildPermissions,
    user_roles: &[GuildRole],
    channel_overrides: Option<&[ChannelOverride]>,
) -> GuildPermissions {
    if guild_owner_id == user_id {
        return GuildPermissions::all();
    }
    if pending {
        return GuildPermissions::empty();
    }

    let mut perms = everyone_permissions;
    for role in user_roles {
        perms |= role.permissions;
    }

    let Some(overrides) = channel_overrides else {
        return perms;
    };

    let is_everyone = |ovr: &ChannelOverride| {
        ovr.everyone
            || user_roles
                .iter()
                .any(|r| r.is_default && r.id == ovr.role_id)
    };

    // @everyone override.
    if let Some(ovr) = overrides.iter().find(|o| is_everyone(o)) {
        perms |= ovr.allow_permissions & GuildPermissions::EVERYONE_ALLOWED;
        perms &= !ovr.deny_permissions;
    }

    // Role overrides, aggregated so the result does not depend on role order.
    let mut role_allow = GuildPermissions::empty();
    let mut role_deny = GuildPermissions::empty();
    for ovr in overrides {
        if is_everyone(ovr) || !user_roles.iter().any(|r| r.id == ovr.role_id) {
            continue;
        }
        role_allow |= ovr.allow_permissions;
        role_deny |= ovr.deny_permissions;
    }
    perms &= !role_deny;
    perms |= role_allow;

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

    /// Channel not found.
    NotFound,

    /// Invalid channel (e.g., missing `guild_id` for guild channel).
    InvalidChannel,

    /// User lacks permission (generic forbidden).
    Forbidden,
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
            Self::NotFound => write!(f, "Channel not found"),
            Self::InvalidChannel => write!(f, "Invalid channel"),
            Self::Forbidden => write!(f, "Access forbidden"),
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
        let perms = compute_guild_permissions(
            owner_id,
            owner_id,
            false,
            GuildPermissions::empty(),
            &[],
            None,
        );
        assert_eq!(perms, GuildPermissions::all());
    }

    #[test]
    fn test_everyone_permissions_applied() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let everyone = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;

        let perms = compute_guild_permissions(user_id, owner_id, false, everyone, &[], None);

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

        let perms =
            compute_guild_permissions(user_id, owner_id, false, everyone, &[mod_role], None);

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
            everyone: false,
        };

        let perms = compute_guild_permissions(
            user_id,
            owner_id,
            false,
            everyone,
            &[role],
            Some(&[override_entry]),
        );

        assert!(!perms.has(GuildPermissions::SEND_MESSAGES)); // denied by override
        assert!(perms.has(GuildPermissions::EMBED_LINKS)); // still from everyone
        assert!(perms.has(GuildPermissions::ATTACH_FILES)); // allowed by override
    }

    #[test]
    fn default_role_override_cannot_allow_forbidden_bits() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let mk_role = |is_default| GuildRole {
            id: role_id,
            guild_id: Uuid::new_v4(),
            name: "r".to_string(),
            color: None,
            permissions: GuildPermissions::empty(),
            position: 999,
            is_default,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let ovr = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id: Uuid::new_v4(),
            role_id,
            allow_permissions: GuildPermissions::ATTACH_FILES | GuildPermissions::MANAGE_WEBHOOKS,
            deny_permissions: GuildPermissions::empty(),
            everyone: false,
        };
        let everyone = GuildPermissions::EVERYONE_DEFAULT;

        let masked = compute_guild_permissions(
            user_id,
            owner_id,
            false,
            everyone,
            &[mk_role(true)],
            Some(std::slice::from_ref(&ovr)),
        );
        assert!(masked.has(GuildPermissions::ATTACH_FILES));
        assert!(!masked.has(GuildPermissions::MANAGE_WEBHOOKS));

        let normal = compute_guild_permissions(
            user_id,
            owner_id,
            false,
            everyone,
            &[mk_role(false)],
            Some(std::slice::from_ref(&ovr)),
        );
        assert!(normal.has(GuildPermissions::MANAGE_WEBHOOKS));
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
    fn test_owner_position_can_manage_any_non_owner_role() {
        let perms = GuildPermissions::MANAGE_ROLES | GuildPermissions::MANAGE_GUILD;

        assert!(can_manage_role(perms, -1, 0, None).is_ok());
        assert!(can_manage_role(perms, -1, 1, None).is_ok());
        assert!(can_manage_role(perms, -1, 999, None).is_ok());
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

        let perms =
            compute_guild_permissions(user_id, owner_id, false, everyone, &[role1, role2], None);

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

        let not_found = PermissionError::NotFound;
        assert!(not_found.to_string().contains("not found"));

        let invalid = PermissionError::InvalidChannel;
        assert!(invalid.to_string().contains("Invalid channel"));

        let forbidden = PermissionError::Forbidden;
        assert!(forbidden.to_string().contains("forbidden"));
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
            everyone: false,
        };

        let override2 = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id: role2_id,
            allow_permissions: GuildPermissions::empty(),
            deny_permissions: GuildPermissions::SEND_MESSAGES,
            everyone: false,
        };

        let perms = compute_guild_permissions(
            user_id,
            owner_id,
            false,
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

    /// Role overrides are aggregated: one role's allow beats another role's
    /// deny of the same bit, independent of the order of the roles.
    #[test]
    fn test_channel_override_role_allow_wins_regardless_of_role_order() {
        let user_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let guild_id = Uuid::new_v4();
        let channel_id = Uuid::new_v4();
        let allow_role_id = Uuid::new_v4();
        let deny_role_id = Uuid::new_v4();

        let everyone = GuildPermissions::VIEW_CHANNEL;

        let allow_role = GuildRole {
            id: allow_role_id,
            guild_id,
            name: "AllowRole".to_string(),
            color: None,
            permissions: GuildPermissions::empty(),
            position: 100,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let deny_role = GuildRole {
            id: deny_role_id,
            guild_id,
            name: "DenyRole".to_string(),
            color: None,
            permissions: GuildPermissions::empty(),
            position: 200,
            is_default: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let allow_override = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id: allow_role_id,
            allow_permissions: GuildPermissions::VIEW_CHANNEL,
            deny_permissions: GuildPermissions::empty(),
            everyone: false,
        };

        let deny_override = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id,
            role_id: deny_role_id,
            allow_permissions: GuildPermissions::empty(),
            deny_permissions: GuildPermissions::VIEW_CHANNEL,
            everyone: false,
        };

        let overrides = [allow_override, deny_override];

        // Order A: allow role first, deny role second.
        let perms_a = compute_guild_permissions(
            user_id,
            owner_id,
            false,
            everyone,
            &[allow_role.clone(), deny_role.clone()],
            Some(&overrides),
        );

        // Order B: deny role first, allow role second.
        let perms_b = compute_guild_permissions(
            user_id,
            owner_id,
            false,
            everyone,
            &[deny_role, allow_role],
            Some(&overrides),
        );

        assert!(perms_a.has(GuildPermissions::VIEW_CHANNEL));
        assert!(perms_b.has(GuildPermissions::VIEW_CHANNEL));
    }

    fn role(id: Uuid, permissions: GuildPermissions, is_default: bool) -> GuildRole {
        GuildRole {
            id,
            guild_id: Uuid::nil(),
            name: "r".to_string(),
            color: None,
            permissions,
            position: 10,
            is_default,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn ovr(role_id: Uuid, allow: GuildPermissions, deny: GuildPermissions) -> ChannelOverride {
        ChannelOverride {
            id: Uuid::new_v4(),
            channel_id: Uuid::nil(),
            role_id,
            allow_permissions: allow,
            deny_permissions: deny,
            everyone: false,
        }
    }

    /// Override of the @everyone role, flagged as loaded from the database.
    fn everyone_ovr(allow: GuildPermissions, deny: GuildPermissions) -> ChannelOverride {
        ChannelOverride {
            everyone: true,
            ..ovr(Uuid::new_v4(), allow, deny)
        }
    }

    const VIEW: GuildPermissions = GuildPermissions::VIEW_CHANNEL;
    const NONE: GuildPermissions = GuildPermissions::empty();

    fn channel_perms(
        everyone: GuildPermissions,
        roles: &[GuildRole],
        overrides: &[ChannelOverride],
    ) -> GuildPermissions {
        compute_guild_permissions(
            Uuid::new_v4(),
            Uuid::new_v4(),
            false,
            everyone,
            roles,
            Some(overrides),
        )
    }

    #[test]
    fn role_allow_beats_everyone_deny() {
        let staff = Uuid::new_v4();
        let overrides = [everyone_ovr(NONE, VIEW), ovr(staff, VIEW, NONE)];

        let with_role = channel_perms(VIEW, &[role(staff, NONE, false)], &overrides);
        assert!(with_role.has(VIEW));

        let without_role = channel_perms(VIEW, &[], &overrides);
        assert!(!without_role.has(VIEW));
    }

    #[test]
    fn everyone_deny_without_role_allow_hides() {
        let other = Uuid::new_v4();
        let overrides = [everyone_ovr(NONE, VIEW)];
        let perms = channel_perms(VIEW, &[role(other, VIEW, false)], &overrides);
        assert!(
            !perms.has(VIEW),
            "a guild-level role permission does not lift a channel deny"
        );
    }

    #[test]
    fn everyone_deny_applies_without_any_role_context() {
        // The member holds no roles and nothing else names the @everyone
        // role: the flag on the override alone keeps the deny in force.
        let overrides = [everyone_ovr(NONE, VIEW)];
        assert!(!channel_perms(VIEW, &[], &overrides).has(VIEW));
    }

    #[test]
    fn role_deny_beats_everyone_allow() {
        let muted = Uuid::new_v4();
        let overrides = [everyone_ovr(VIEW, NONE), ovr(muted, NONE, VIEW)];

        assert!(!channel_perms(NONE, &[role(muted, NONE, false)], &overrides).has(VIEW));
        assert!(channel_perms(NONE, &[], &overrides).has(VIEW));
    }

    #[test]
    fn everyone_override_allow_is_masked() {
        let overrides = [everyone_ovr(
            GuildPermissions::ATTACH_FILES | GuildPermissions::MANAGE_MESSAGES,
            NONE,
        )];
        let perms = channel_perms(GuildPermissions::EVERYONE_DEFAULT, &[], &overrides);
        assert!(perms.has(GuildPermissions::ATTACH_FILES));
        assert!(!perms.has(GuildPermissions::MANAGE_MESSAGES));
    }

    #[test]
    fn everyone_override_applies_once_when_role_is_also_assigned() {
        // Some data sets list the @everyone role among the member's roles as
        // well; its override must still be treated as the @everyone step and
        // not as a role override (no unmasked allow, deny not lifted by itself).
        let everyone_id = Uuid::new_v4();
        let staff = Uuid::new_v4();
        let roles = [role(everyone_id, VIEW, true), role(staff, NONE, false)];
        for flagged in [true, false] {
            let everyone = ChannelOverride {
                everyone: flagged,
                ..ovr(everyone_id, GuildPermissions::MANAGE_MESSAGES, VIEW)
            };
            let overrides = [everyone, ovr(staff, VIEW, NONE)];
            let perms = channel_perms(VIEW, &roles, &overrides);
            assert!(perms.has(VIEW));
            assert!(!perms.has(GuildPermissions::MANAGE_MESSAGES));

            let perms = channel_perms(VIEW, &roles[..1], &overrides);
            assert!(!perms.has(VIEW));
            assert!(!perms.has(GuildPermissions::MANAGE_MESSAGES));
        }
    }

    #[test]
    fn overrides_of_unassigned_roles_are_ignored() {
        let overrides = [ovr(Uuid::new_v4(), VIEW, NONE)];
        assert!(!channel_perms(NONE, &[], &overrides).has(VIEW));
    }
}

#[cfg(test)]
mod pending_tests {
    use super::*;

    #[test]
    fn pending_member_gets_nothing_even_with_everyone_override_allow() {
        let everyone = ChannelOverride {
            id: Uuid::new_v4(),
            channel_id: Uuid::nil(),
            role_id: Uuid::new_v4(),
            allow_permissions: GuildPermissions::VIEW_CHANNEL | GuildPermissions::SEND_MESSAGES,
            deny_permissions: GuildPermissions::empty(),
            everyone: true,
        };
        let user = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let pending = compute_guild_permissions(
            user,
            owner,
            true,
            GuildPermissions::empty(),
            &[],
            Some(std::slice::from_ref(&everyone)),
        );
        assert!(pending.is_empty());
        let active = compute_guild_permissions(
            user,
            owner,
            false,
            GuildPermissions::empty(),
            &[],
            Some(std::slice::from_ref(&everyone)),
        );
        assert!(active.has(GuildPermissions::VIEW_CHANNEL));
        // The owner is never treated as pending.
        let owner_perms =
            compute_guild_permissions(owner, owner, true, GuildPermissions::empty(), &[], None);
        assert_eq!(owner_perms, GuildPermissions::all());
    }
}
