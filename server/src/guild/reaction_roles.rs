//! Self-assignable (reaction) role bindings.
//!
//! Admins bind an emoji on a message to a role; members react to grant/revoke
//! it themselves. Creation is gated by `MANAGE_ROLES` + a hierarchy guard +
//! a "not dangerous" guard; reaction-time self-assign is intentionally
//! unprivileged (that is the feature).

use crate::permissions::GuildPermissions;

/// Reject binding a role that carries any permission a member must never be
/// able to self-grant. Reuses the canonical `EVERYONE_FORBIDDEN` deny-list so
/// this stays in lockstep with the @everyone guard.
///
/// Returns `true` if the role is safe to make self-assignable.
#[must_use]
pub fn is_role_self_assignable(role_permissions: GuildPermissions) -> bool {
    !role_permissions.intersects(GuildPermissions::EVERYONE_FORBIDDEN)
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn safe_roles_are_self_assignable() {
        let safe = GuildPermissions::SEND_MESSAGES
            | GuildPermissions::VOICE_CONNECT
            | GuildPermissions::ADD_REACTIONS;
        assert!(is_role_self_assignable(safe));
        // A pure cosmetic role (no perms) is fine.
        assert!(is_role_self_assignable(GuildPermissions::empty()));
    }

    #[test]
    fn dangerous_roles_are_not_self_assignable() {
        for perm in [
            GuildPermissions::MANAGE_ROLES,
            GuildPermissions::MANAGE_GUILD,
            GuildPermissions::BAN_MEMBERS,
            GuildPermissions::KICK_MEMBERS,
            GuildPermissions::MANAGE_CHANNELS,
        ] {
            let perms = GuildPermissions::SEND_MESSAGES | perm;
            assert!(
                !is_role_self_assignable(perms),
                "{perm:?} must make a role non-self-assignable"
            );
        }
    }
}
