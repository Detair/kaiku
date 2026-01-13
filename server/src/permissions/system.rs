//! System-level permissions for administrative actions.
//!
//! These permissions are for platform-wide administrative operations,
//! distinct from guild-specific permissions.

/// System-level permission for administrative actions.
///
/// These permissions control access to platform-wide administrative functions
/// and are typically assigned only to system administrators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemPermission {
    /// View all guilds on the platform (admin dashboard)
    ViewAllGuilds,
    /// Suspend a guild (disables access without deletion)
    SuspendGuild,
    /// Permanently delete a guild
    DeleteGuild,
    /// Assign or transfer guild ownership
    AssignGuildOwner,
    /// View all users on the platform
    ViewAllUsers,
    /// Ban a user globally (all guilds)
    GlobalBanUser,
    /// View system-wide audit log
    ViewAuditLog,
    /// Send platform-wide announcements
    SendAnnouncement,
    /// Manage system settings and configuration
    ManageSettings,
    /// Use break-glass emergency access
    UseBreakGlass,
    /// Review and approve break-glass access requests
    ReviewBreakGlass,
}

impl SystemPermission {
    /// Returns the action name for audit logging.
    ///
    /// These names are used in audit log entries to identify the action performed.
    ///
    /// # Examples
    ///
    /// ```
    /// use vc_server::permissions::SystemPermission;
    ///
    /// let perm = SystemPermission::SuspendGuild;
    /// assert_eq!(perm.action_name(), "suspend_guild");
    /// ```
    #[must_use]
    pub const fn action_name(&self) -> &'static str {
        match self {
            Self::ViewAllGuilds => "view_all_guilds",
            Self::SuspendGuild => "suspend_guild",
            Self::DeleteGuild => "delete_guild",
            Self::AssignGuildOwner => "assign_guild_owner",
            Self::ViewAllUsers => "view_all_users",
            Self::GlobalBanUser => "global_ban_user",
            Self::ViewAuditLog => "view_audit_log",
            Self::SendAnnouncement => "send_announcement",
            Self::ManageSettings => "manage_settings",
            Self::UseBreakGlass => "use_break_glass",
            Self::ReviewBreakGlass => "review_break_glass",
        }
    }

    /// Returns all system permissions as a slice.
    ///
    /// Useful for iteration and validation.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::ViewAllGuilds,
            Self::SuspendGuild,
            Self::DeleteGuild,
            Self::AssignGuildOwner,
            Self::ViewAllUsers,
            Self::GlobalBanUser,
            Self::ViewAuditLog,
            Self::SendAnnouncement,
            Self::ManageSettings,
            Self::UseBreakGlass,
            Self::ReviewBreakGlass,
        ]
    }

    /// Returns a human-readable description of the permission.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::ViewAllGuilds => "View all guilds on the platform",
            Self::SuspendGuild => "Suspend guilds (disable access)",
            Self::DeleteGuild => "Permanently delete guilds",
            Self::AssignGuildOwner => "Assign or transfer guild ownership",
            Self::ViewAllUsers => "View all platform users",
            Self::GlobalBanUser => "Ban users from entire platform",
            Self::ViewAuditLog => "View system-wide audit log",
            Self::SendAnnouncement => "Send platform announcements",
            Self::ManageSettings => "Manage system settings",
            Self::UseBreakGlass => "Use emergency break-glass access",
            Self::ReviewBreakGlass => "Review break-glass requests",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_names_are_snake_case() {
        for perm in SystemPermission::all() {
            let name = perm.action_name();
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "Action name '{}' should be snake_case",
                name
            );
        }
    }

    #[test]
    fn test_action_names_are_unique() {
        let all_perms = SystemPermission::all();
        let names: Vec<&str> = all_perms.iter().map(|p| p.action_name()).collect();

        for (i, name) in names.iter().enumerate() {
            for (j, other_name) in names.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        name, other_name,
                        "Duplicate action name found: {}",
                        name
                    );
                }
            }
        }
    }

    #[test]
    fn test_all_returns_all_variants() {
        let all = SystemPermission::all();
        assert_eq!(all.len(), 11);

        // Verify each variant is present
        assert!(all.contains(&SystemPermission::ViewAllGuilds));
        assert!(all.contains(&SystemPermission::SuspendGuild));
        assert!(all.contains(&SystemPermission::DeleteGuild));
        assert!(all.contains(&SystemPermission::AssignGuildOwner));
        assert!(all.contains(&SystemPermission::ViewAllUsers));
        assert!(all.contains(&SystemPermission::GlobalBanUser));
        assert!(all.contains(&SystemPermission::ViewAuditLog));
        assert!(all.contains(&SystemPermission::SendAnnouncement));
        assert!(all.contains(&SystemPermission::ManageSettings));
        assert!(all.contains(&SystemPermission::UseBreakGlass));
        assert!(all.contains(&SystemPermission::ReviewBreakGlass));
    }

    #[test]
    fn test_view_all_guilds_action_name() {
        assert_eq!(
            SystemPermission::ViewAllGuilds.action_name(),
            "view_all_guilds"
        );
    }

    #[test]
    fn test_suspend_guild_action_name() {
        assert_eq!(
            SystemPermission::SuspendGuild.action_name(),
            "suspend_guild"
        );
    }

    #[test]
    fn test_delete_guild_action_name() {
        assert_eq!(
            SystemPermission::DeleteGuild.action_name(),
            "delete_guild"
        );
    }

    #[test]
    fn test_assign_guild_owner_action_name() {
        assert_eq!(
            SystemPermission::AssignGuildOwner.action_name(),
            "assign_guild_owner"
        );
    }

    #[test]
    fn test_view_all_users_action_name() {
        assert_eq!(
            SystemPermission::ViewAllUsers.action_name(),
            "view_all_users"
        );
    }

    #[test]
    fn test_global_ban_user_action_name() {
        assert_eq!(
            SystemPermission::GlobalBanUser.action_name(),
            "global_ban_user"
        );
    }

    #[test]
    fn test_view_audit_log_action_name() {
        assert_eq!(
            SystemPermission::ViewAuditLog.action_name(),
            "view_audit_log"
        );
    }

    #[test]
    fn test_send_announcement_action_name() {
        assert_eq!(
            SystemPermission::SendAnnouncement.action_name(),
            "send_announcement"
        );
    }

    #[test]
    fn test_manage_settings_action_name() {
        assert_eq!(
            SystemPermission::ManageSettings.action_name(),
            "manage_settings"
        );
    }

    #[test]
    fn test_use_break_glass_action_name() {
        assert_eq!(
            SystemPermission::UseBreakGlass.action_name(),
            "use_break_glass"
        );
    }

    #[test]
    fn test_review_break_glass_action_name() {
        assert_eq!(
            SystemPermission::ReviewBreakGlass.action_name(),
            "review_break_glass"
        );
    }

    #[test]
    fn test_descriptions_are_not_empty() {
        for perm in SystemPermission::all() {
            let desc = perm.description();
            assert!(
                !desc.is_empty(),
                "Description for {:?} should not be empty",
                perm
            );
        }
    }

    #[test]
    fn test_clone() {
        let original = SystemPermission::SuspendGuild;
        let cloned = original;
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_debug_format() {
        let perm = SystemPermission::GlobalBanUser;
        let debug_str = format!("{:?}", perm);
        assert_eq!(debug_str, "GlobalBanUser");
    }

    #[test]
    fn test_equality() {
        assert_eq!(SystemPermission::ViewAllGuilds, SystemPermission::ViewAllGuilds);
        assert_ne!(SystemPermission::ViewAllGuilds, SystemPermission::ViewAllUsers);
    }

    #[test]
    fn test_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(SystemPermission::ViewAllGuilds);
        set.insert(SystemPermission::SuspendGuild);
        set.insert(SystemPermission::ViewAllGuilds); // Duplicate

        assert_eq!(set.len(), 2);
    }

    // === Serde Tests ===

    #[test]
    fn test_serialize_permission() {
        let perm = SystemPermission::SuspendGuild;
        let json = serde_json::to_string(&perm).unwrap();
        assert_eq!(json, "\"suspend_guild\"");
    }

    #[test]
    fn test_serialize_all_permissions_snake_case() {
        // Verify all permissions serialize to snake_case
        for perm in SystemPermission::all() {
            let json = serde_json::to_string(perm).unwrap();
            let json_str = json.trim_matches('"');
            assert!(
                json_str.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "Serialized permission '{}' should be snake_case",
                json_str
            );
        }
    }

    #[test]
    fn test_deserialize_permission() {
        let json = "\"suspend_guild\"";
        let perm: SystemPermission = serde_json::from_str(json).unwrap();
        assert_eq!(perm, SystemPermission::SuspendGuild);
    }

    #[test]
    fn test_serde_roundtrip() {
        for original in SystemPermission::all() {
            let json = serde_json::to_string(original).unwrap();
            let restored: SystemPermission = serde_json::from_str(&json).unwrap();
            assert_eq!(*original, restored);
        }
    }

    #[test]
    fn test_serde_matches_action_name() {
        // Verify serialized form matches action_name()
        for perm in SystemPermission::all() {
            let json = serde_json::to_string(perm).unwrap();
            let expected = format!("\"{}\"", perm.action_name());
            assert_eq!(
                json, expected,
                "Serialized form should match action_name() for {:?}",
                perm
            );
        }
    }
}
