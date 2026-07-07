//! Self-assignable (reaction) role bindings.
//!
//! Admins bind an emoji on a message to a role; members react to grant/revoke
//! it themselves. Creation is gated by `MANAGE_ROLES` + a hierarchy guard +
//! a "not dangerous" guard; reaction-time self-assign is intentionally
//! unprivileged (that is the feature).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

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

/// Body for `POST /api/guilds/{id}/reaction-roles`.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateReactionRoleRequest {
    pub channel_id: Uuid,
    pub message_id: Uuid,
    #[validate(length(min = 1, max = 128))]
    pub emoji: String,
    pub role_id: Uuid,
    /// "toggle" (default) or "unique".
    #[serde(default = "default_mode")]
    #[validate(custom(function = "validate_mode"))]
    pub mode: String,
    #[validate(length(max = 64))]
    pub group_key: Option<String>,
}

fn default_mode() -> String {
    "toggle".to_string()
}

fn validate_mode(mode: &str) -> Result<(), validator::ValidationError> {
    if mode == "toggle" || mode == "unique" {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_mode"))
    }
}

/// A reaction-role binding as returned by the API.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReactionRoleResponse {
    pub id: Uuid,
    pub guild_id: Uuid,
    pub channel_id: Uuid,
    pub message_id: Uuid,
    pub emoji: String,
    pub role_id: Uuid,
    pub group_key: Option<String>,
    pub mode: String,
    pub created_at: DateTime<Utc>,
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
