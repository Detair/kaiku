//! Guild-level permissions using bitflags.
//!
//! Permissions are organized into categories:
//! - Content (bits 0-4): Message and media permissions
//! - Voice (bits 5-9): Voice channel permissions
//! - Moderation (bits 10-13): Member management permissions
//! - Guild Management (bits 14-18): Administrative permissions
//! - Invites (bits 19-20): Invite-related permissions
//! - Pages (bit 21): Information page management
//! - Screen Sharing (bit 22): Screen sharing in voice channels

use bitflags::bitflags;

bitflags! {
    /// Guild permissions represented as a 64-bit bitfield.
    ///
    /// Stored as BIGINT in PostgreSQL for efficient database operations.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    #[serde(transparent)]
    pub struct GuildPermissions: u64 {
        // === Content (bits 0-4) ===
        /// Permission to send text messages in channels
        const SEND_MESSAGES      = 1 << 0;
        /// Permission to embed links in messages (auto-preview)
        const EMBED_LINKS        = 1 << 1;
        /// Permission to attach files to messages
        const ATTACH_FILES       = 1 << 2;
        /// Permission to use custom emoji
        const USE_EMOJI          = 1 << 3;
        /// Permission to add reactions to messages
        const ADD_REACTIONS      = 1 << 4;

        // === Voice (bits 5-9) ===
        /// Permission to connect to voice channels
        const VOICE_CONNECT      = 1 << 5;
        /// Permission to speak in voice channels
        const VOICE_SPEAK        = 1 << 6;
        /// Permission to mute other members in voice channels
        const VOICE_MUTE_OTHERS  = 1 << 7;
        /// Permission to deafen other members in voice channels
        const VOICE_DEAFEN_OTHERS = 1 << 8;
        /// Permission to move members between voice channels
        const VOICE_MOVE_MEMBERS = 1 << 9;

        // === Moderation (bits 10-13) ===
        /// Permission to delete messages from other members
        const MANAGE_MESSAGES    = 1 << 10;
        /// Permission to timeout members (temporary mute)
        const TIMEOUT_MEMBERS    = 1 << 11;
        /// Permission to kick members from the guild
        const KICK_MEMBERS       = 1 << 12;
        /// Permission to ban members from the guild
        const BAN_MEMBERS        = 1 << 13;

        // === Guild Management (bits 14-18) ===
        /// Permission to create, edit, and delete channels
        const MANAGE_CHANNELS    = 1 << 14;
        /// Permission to create, edit, and delete roles
        const MANAGE_ROLES       = 1 << 15;
        /// Permission to view the guild audit log
        const VIEW_AUDIT_LOG     = 1 << 16;
        /// Permission to modify guild settings
        const MANAGE_GUILD       = 1 << 17;
        /// Permission to transfer guild ownership (owner only)
        const TRANSFER_OWNERSHIP = 1 << 18;

        // === Invites (bits 19-20) ===
        /// Permission to create invite links
        const CREATE_INVITE      = 1 << 19;
        /// Permission to manage (revoke) invite links
        const MANAGE_INVITES     = 1 << 20;

        // === Pages (bit 21) ===
        /// Permission to create, edit, delete, and reorder guild information pages
        const MANAGE_PAGES       = 1 << 21;

        // === Screen Sharing (bit 22) ===
        /// Permission to start screen sharing in voice channels
        const SCREEN_SHARE       = 1 << 22;

        // === Mentions (bit 23) ===
        /// Permission to mention @everyone and @here
        const MENTION_EVERYONE   = 1 << 23;

        // === Channel Visibility (bit 24) ===
        /// Permission to view a channel and read its message history
        const VIEW_CHANNEL       = 1 << 24;
    }
}

impl GuildPermissions {
    // === Preset Combinations ===

    /// Default permissions for the @everyone role.
    ///
    /// Includes basic content and voice permissions that all members should have.
    pub const EVERYONE_DEFAULT: Self = Self::SEND_MESSAGES
        .union(Self::EMBED_LINKS)
        .union(Self::ATTACH_FILES)
        .union(Self::USE_EMOJI)
        .union(Self::ADD_REACTIONS)
        .union(Self::VOICE_CONNECT)
        .union(Self::VOICE_SPEAK)
        .union(Self::CREATE_INVITE);

    /// Default permissions for moderators.
    ///
    /// Includes content permissions, voice moderation, and member management.
    pub const MODERATOR_DEFAULT: Self = Self::EVERYONE_DEFAULT
        .union(Self::VOICE_MUTE_OTHERS)
        .union(Self::VOICE_DEAFEN_OTHERS)
        .union(Self::VOICE_MOVE_MEMBERS)
        .union(Self::MANAGE_MESSAGES)
        .union(Self::TIMEOUT_MEMBERS)
        .union(Self::KICK_MEMBERS)
        .union(Self::VIEW_AUDIT_LOG)
        .union(Self::MANAGE_INVITES)
        .union(Self::SCREEN_SHARE)
        .union(Self::MENTION_EVERYONE);

    /// Default permissions for officers (senior moderators).
    ///
    /// Includes moderator permissions plus ban, channel, and page management.
    pub const OFFICER_DEFAULT: Self = Self::MODERATOR_DEFAULT
        .union(Self::BAN_MEMBERS)
        .union(Self::MANAGE_CHANNELS)
        .union(Self::MANAGE_PAGES);

    /// Permissions that @everyone can NEVER have.
    ///
    /// These are sensitive permissions that should only be granted to trusted roles.
    /// Used for validation when modifying the @everyone role.
    pub const EVERYONE_FORBIDDEN: Self = Self::VOICE_MUTE_OTHERS
        .union(Self::VOICE_DEAFEN_OTHERS)
        .union(Self::VOICE_MOVE_MEMBERS)
        .union(Self::MANAGE_MESSAGES)
        .union(Self::TIMEOUT_MEMBERS)
        .union(Self::KICK_MEMBERS)
        .union(Self::BAN_MEMBERS)
        .union(Self::MANAGE_CHANNELS)
        .union(Self::MANAGE_ROLES)
        .union(Self::VIEW_AUDIT_LOG)
        .union(Self::MANAGE_GUILD)
        .union(Self::TRANSFER_OWNERSHIP)
        .union(Self::MANAGE_INVITES)
        .union(Self::MANAGE_PAGES)
        .union(Self::SCREEN_SHARE)
        .union(Self::MENTION_EVERYONE);

    // === Database Conversion ===

    /// Create permissions from a database BIGINT value.
    ///
    /// This safely handles the i64 to u64 conversion required for `PostgreSQL` compatibility.
    /// Invalid bits are silently ignored to maintain forward compatibility.
    #[must_use]
    pub const fn from_db(value: i64) -> Self {
        // Reinterpret the i64 bit pattern as u64
        let bits = value as u64;
        // Only keep known permission bits, ignore unknown ones
        Self::from_bits_truncate(bits)
    }

    /// Convert permissions to a database BIGINT value.
    ///
    /// Returns the bit pattern as i64 for `PostgreSQL` storage.
    #[must_use]
    pub const fn to_db(self) -> i64 {
        self.bits() as i64
    }

    // === Permission Checking ===

    /// Check if this permission set includes the specified permission(s).
    ///
    /// # Examples
    ///
    /// ```
    /// use vc_server::permissions::GuildPermissions;
    ///
    /// let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    /// assert!(perms.has(GuildPermissions::SEND_MESSAGES));
    /// assert!(!perms.has(GuildPermissions::BAN_MEMBERS));
    /// ```
    #[must_use]
    pub const fn has(self, permission: Self) -> bool {
        self.contains(permission)
    }

    /// Validate that these permissions are safe for the @everyone role.
    ///
    /// Returns `true` if none of the forbidden permissions are present.
    ///
    /// # Examples
    ///
    /// ```
    /// use vc_server::permissions::GuildPermissions;
    ///
    /// let safe = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
    /// assert!(safe.validate_for_everyone());
    ///
    /// let unsafe_perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::BAN_MEMBERS;
    /// assert!(!unsafe_perms.validate_for_everyone());
    /// ```
    #[must_use]
    pub const fn validate_for_everyone(self) -> bool {
        !self.intersects(Self::EVERYONE_FORBIDDEN)
    }
}

impl Default for GuildPermissions {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Bit Position Tests ===

    #[test]
    fn test_content_permission_bits() {
        assert_eq!(GuildPermissions::SEND_MESSAGES.bits(), 1 << 0);
        assert_eq!(GuildPermissions::EMBED_LINKS.bits(), 1 << 1);
        assert_eq!(GuildPermissions::ATTACH_FILES.bits(), 1 << 2);
        assert_eq!(GuildPermissions::USE_EMOJI.bits(), 1 << 3);
        assert_eq!(GuildPermissions::ADD_REACTIONS.bits(), 1 << 4);
    }

    #[test]
    fn test_voice_permission_bits() {
        assert_eq!(GuildPermissions::VOICE_CONNECT.bits(), 1 << 5);
        assert_eq!(GuildPermissions::VOICE_SPEAK.bits(), 1 << 6);
        assert_eq!(GuildPermissions::VOICE_MUTE_OTHERS.bits(), 1 << 7);
        assert_eq!(GuildPermissions::VOICE_DEAFEN_OTHERS.bits(), 1 << 8);
        assert_eq!(GuildPermissions::VOICE_MOVE_MEMBERS.bits(), 1 << 9);
    }

    #[test]
    fn test_moderation_permission_bits() {
        assert_eq!(GuildPermissions::MANAGE_MESSAGES.bits(), 1 << 10);
        assert_eq!(GuildPermissions::TIMEOUT_MEMBERS.bits(), 1 << 11);
        assert_eq!(GuildPermissions::KICK_MEMBERS.bits(), 1 << 12);
        assert_eq!(GuildPermissions::BAN_MEMBERS.bits(), 1 << 13);
    }

    #[test]
    fn test_guild_management_permission_bits() {
        assert_eq!(GuildPermissions::MANAGE_CHANNELS.bits(), 1 << 14);
        assert_eq!(GuildPermissions::MANAGE_ROLES.bits(), 1 << 15);
        assert_eq!(GuildPermissions::VIEW_AUDIT_LOG.bits(), 1 << 16);
        assert_eq!(GuildPermissions::MANAGE_GUILD.bits(), 1 << 17);
        assert_eq!(GuildPermissions::TRANSFER_OWNERSHIP.bits(), 1 << 18);
    }

    #[test]
    fn test_invite_permission_bits() {
        assert_eq!(GuildPermissions::CREATE_INVITE.bits(), 1 << 19);
        assert_eq!(GuildPermissions::MANAGE_INVITES.bits(), 1 << 20);
    }

    #[test]
    fn test_pages_permission_bits() {
        assert_eq!(GuildPermissions::MANAGE_PAGES.bits(), 1 << 21);
    }

    #[test]
    fn test_screen_sharing_permission_bits() {
        assert_eq!(GuildPermissions::SCREEN_SHARE.bits(), 1 << 22);
    }

    #[test]
    fn test_mention_everyone_permission_bits() {
        assert_eq!(GuildPermissions::MENTION_EVERYONE.bits(), 1 << 23);
    }

    #[test]
    fn test_view_channel_permission_bits() {
        assert_eq!(GuildPermissions::VIEW_CHANNEL.bits(), 1 << 24);
    }

    // === Preset Tests ===

    #[test]
    fn test_everyone_default_includes_basic_permissions() {
        let everyone = GuildPermissions::EVERYONE_DEFAULT;

        // Should include content permissions
        assert!(everyone.has(GuildPermissions::SEND_MESSAGES));
        assert!(everyone.has(GuildPermissions::EMBED_LINKS));
        assert!(everyone.has(GuildPermissions::ATTACH_FILES));
        assert!(everyone.has(GuildPermissions::USE_EMOJI));
        assert!(everyone.has(GuildPermissions::ADD_REACTIONS));

        // Should include basic voice
        assert!(everyone.has(GuildPermissions::VOICE_CONNECT));
        assert!(everyone.has(GuildPermissions::VOICE_SPEAK));

        // Should include invite creation
        assert!(everyone.has(GuildPermissions::CREATE_INVITE));

        // Should NOT include moderation
        assert!(!everyone.has(GuildPermissions::MANAGE_MESSAGES));
        assert!(!everyone.has(GuildPermissions::KICK_MEMBERS));
        assert!(!everyone.has(GuildPermissions::BAN_MEMBERS));

        // Should NOT include screen sharing (privileged feature)
        assert!(!everyone.has(GuildPermissions::SCREEN_SHARE));
    }

    #[test]
    fn test_moderator_default_extends_everyone() {
        let moderator = GuildPermissions::MODERATOR_DEFAULT;
        let everyone = GuildPermissions::EVERYONE_DEFAULT;

        // Moderator should have all everyone permissions
        assert!(moderator.contains(everyone));

        // Plus moderation permissions
        assert!(moderator.has(GuildPermissions::VOICE_MUTE_OTHERS));
        assert!(moderator.has(GuildPermissions::MANAGE_MESSAGES));
        assert!(moderator.has(GuildPermissions::TIMEOUT_MEMBERS));
        assert!(moderator.has(GuildPermissions::KICK_MEMBERS));
        assert!(moderator.has(GuildPermissions::VIEW_AUDIT_LOG));
        assert!(moderator.has(GuildPermissions::SCREEN_SHARE));

        // But not ban or channel management
        assert!(!moderator.has(GuildPermissions::BAN_MEMBERS));
        assert!(!moderator.has(GuildPermissions::MANAGE_CHANNELS));
    }

    #[test]
    fn test_officer_default_extends_moderator() {
        let officer = GuildPermissions::OFFICER_DEFAULT;
        let moderator = GuildPermissions::MODERATOR_DEFAULT;

        // Officer should have all moderator permissions
        assert!(officer.contains(moderator));

        // Plus additional permissions
        assert!(officer.has(GuildPermissions::BAN_MEMBERS));
        assert!(officer.has(GuildPermissions::MANAGE_CHANNELS));
        assert!(officer.has(GuildPermissions::MANAGE_PAGES));

        // But not ownership transfer
        assert!(!officer.has(GuildPermissions::TRANSFER_OWNERSHIP));
    }

    #[test]
    fn test_everyone_default_passes_validation() {
        assert!(GuildPermissions::EVERYONE_DEFAULT.validate_for_everyone());
    }

    #[test]
    fn test_moderator_default_fails_everyone_validation() {
        assert!(!GuildPermissions::MODERATOR_DEFAULT.validate_for_everyone());
    }

    // === Database Conversion Tests ===

    #[test]
    fn test_to_db_and_from_db_roundtrip() {
        let original = GuildPermissions::SEND_MESSAGES
            | GuildPermissions::VOICE_CONNECT
            | GuildPermissions::MANAGE_CHANNELS;

        let db_value = original.to_db();
        let restored = GuildPermissions::from_db(db_value);

        assert_eq!(original, restored);
    }

    #[test]
    fn test_from_db_with_zero() {
        let perms = GuildPermissions::from_db(0);
        assert!(perms.is_empty());
    }

    #[test]
    fn test_from_db_with_negative_value() {
        // PostgreSQL might return negative values for high bit patterns
        // This should be handled correctly
        let db_value: i64 = -1; // All bits set
        let perms = GuildPermissions::from_db(db_value);

        // Should have all defined permissions
        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(perms.has(GuildPermissions::TRANSFER_OWNERSHIP));
        assert!(perms.has(GuildPermissions::MANAGE_INVITES));
    }

    #[test]
    fn test_from_db_truncates_unknown_bits() {
        // Set a bit beyond our defined permissions (bit 63)
        let db_value: i64 = (1_i64 << 0) | (1_i64 << 63);
        let perms = GuildPermissions::from_db(db_value);

        // Should have SEND_MESSAGES but unknown bit should be truncated
        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        // The unknown bit should not appear in the result
        assert_eq!(perms.bits(), 1);
    }

    #[test]
    fn test_to_db_preserves_all_bits() {
        let all_perms = GuildPermissions::all();
        let db_value = all_perms.to_db();

        // Should be a positive value (highest bit is 20, well within i64 range)
        assert!(db_value > 0);

        // Verify specific bits
        assert_eq!(db_value & (1 << 0), 1 << 0); // SEND_MESSAGES
        assert_eq!(db_value & (1 << 20), 1 << 20); // MANAGE_INVITES
    }

    // === Has Method Tests ===

    #[test]
    fn test_has_single_permission() {
        let perms = GuildPermissions::SEND_MESSAGES;
        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(!perms.has(GuildPermissions::VOICE_CONNECT));
    }

    #[test]
    fn test_has_multiple_permissions() {
        let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;

        // Check for combined requirement
        let required = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
        assert!(perms.has(required));

        // Check for partial - should fail since has() requires ALL bits
        let partial_missing = GuildPermissions::SEND_MESSAGES | GuildPermissions::BAN_MEMBERS;
        assert!(!perms.has(partial_missing));
    }

    #[test]
    fn test_has_empty_permissions() {
        let perms = GuildPermissions::empty();
        assert!(perms.has(GuildPermissions::empty()));
        assert!(!perms.has(GuildPermissions::SEND_MESSAGES));
    }

    // === Validate for Everyone Tests ===

    #[test]
    fn test_validate_for_everyone_with_safe_permissions() {
        let safe = GuildPermissions::SEND_MESSAGES
            | GuildPermissions::VOICE_CONNECT
            | GuildPermissions::CREATE_INVITE;

        assert!(safe.validate_for_everyone());
    }

    #[test]
    fn test_validate_for_everyone_with_forbidden_permission() {
        // Each forbidden permission should fail validation
        let forbidden_perms = [
            GuildPermissions::VOICE_MUTE_OTHERS,
            GuildPermissions::VOICE_DEAFEN_OTHERS,
            GuildPermissions::VOICE_MOVE_MEMBERS,
            GuildPermissions::MANAGE_MESSAGES,
            GuildPermissions::TIMEOUT_MEMBERS,
            GuildPermissions::KICK_MEMBERS,
            GuildPermissions::BAN_MEMBERS,
            GuildPermissions::MANAGE_CHANNELS,
            GuildPermissions::MANAGE_ROLES,
            GuildPermissions::VIEW_AUDIT_LOG,
            GuildPermissions::MANAGE_GUILD,
            GuildPermissions::TRANSFER_OWNERSHIP,
            GuildPermissions::MANAGE_INVITES,
            GuildPermissions::MANAGE_PAGES,
            GuildPermissions::SCREEN_SHARE,
            GuildPermissions::MENTION_EVERYONE,
        ];

        for forbidden in forbidden_perms {
            let perms = GuildPermissions::SEND_MESSAGES | forbidden;
            assert!(
                !perms.validate_for_everyone(),
                "{forbidden:?} should be forbidden for @everyone"
            );
        }
    }

    #[test]
    fn test_validate_for_everyone_empty() {
        assert!(GuildPermissions::empty().validate_for_everyone());
    }

    // === Bitwise Operation Tests ===

    #[test]
    fn test_union_operation() {
        let a = GuildPermissions::SEND_MESSAGES;
        let b = GuildPermissions::VOICE_CONNECT;
        let combined = a | b;

        assert!(combined.has(GuildPermissions::SEND_MESSAGES));
        assert!(combined.has(GuildPermissions::VOICE_CONNECT));
    }

    #[test]
    fn test_intersection_operation() {
        let a = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
        let b = GuildPermissions::VOICE_CONNECT | GuildPermissions::BAN_MEMBERS;
        let intersection = a & b;

        assert!(!intersection.has(GuildPermissions::SEND_MESSAGES));
        assert!(intersection.has(GuildPermissions::VOICE_CONNECT));
        assert!(!intersection.has(GuildPermissions::BAN_MEMBERS));
    }

    #[test]
    fn test_difference_operation() {
        let moderator = GuildPermissions::MODERATOR_DEFAULT;
        let everyone = GuildPermissions::EVERYONE_DEFAULT;
        let moderator_only = moderator - everyone;

        // Should not have everyone permissions
        assert!(!moderator_only.has(GuildPermissions::SEND_MESSAGES));

        // Should have moderator-specific permissions
        assert!(moderator_only.has(GuildPermissions::MANAGE_MESSAGES));
        assert!(moderator_only.has(GuildPermissions::KICK_MEMBERS));
    }

    // === Default and Empty Tests ===

    #[test]
    fn test_default_is_empty() {
        assert_eq!(GuildPermissions::default(), GuildPermissions::empty());
    }

    #[test]
    fn test_empty_has_no_permissions() {
        let empty = GuildPermissions::empty();
        assert!(!empty.has(GuildPermissions::SEND_MESSAGES));
        assert!(empty.is_empty());
    }

    // === Clone and Copy Tests ===

    #[test]
    fn test_clone() {
        let original = GuildPermissions::MODERATOR_DEFAULT;
        let cloned = original;

        assert_eq!(original, cloned);
    }

    // === Debug and Display Tests ===

    #[test]
    fn test_debug_format() {
        let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
        let debug_str = format!("{perms:?}");

        assert!(debug_str.contains("SEND_MESSAGES"));
        assert!(debug_str.contains("VOICE_CONNECT"));
    }

    // === Edge Case Tests ===

    #[test]
    fn test_all_permissions() {
        let all = GuildPermissions::all();

        // Should have all defined permissions
        assert!(all.has(GuildPermissions::SEND_MESSAGES));
        assert!(all.has(GuildPermissions::TRANSFER_OWNERSHIP));
        assert!(all.has(GuildPermissions::MANAGE_INVITES));
    }

    #[test]
    fn test_no_bit_overlaps() {
        // Verify no permissions share the same bit
        let all_perms = [
            GuildPermissions::SEND_MESSAGES,
            GuildPermissions::EMBED_LINKS,
            GuildPermissions::ATTACH_FILES,
            GuildPermissions::USE_EMOJI,
            GuildPermissions::ADD_REACTIONS,
            GuildPermissions::VOICE_CONNECT,
            GuildPermissions::VOICE_SPEAK,
            GuildPermissions::VOICE_MUTE_OTHERS,
            GuildPermissions::VOICE_DEAFEN_OTHERS,
            GuildPermissions::VOICE_MOVE_MEMBERS,
            GuildPermissions::MANAGE_MESSAGES,
            GuildPermissions::TIMEOUT_MEMBERS,
            GuildPermissions::KICK_MEMBERS,
            GuildPermissions::BAN_MEMBERS,
            GuildPermissions::MANAGE_CHANNELS,
            GuildPermissions::MANAGE_ROLES,
            GuildPermissions::VIEW_AUDIT_LOG,
            GuildPermissions::MANAGE_GUILD,
            GuildPermissions::TRANSFER_OWNERSHIP,
            GuildPermissions::CREATE_INVITE,
            GuildPermissions::MANAGE_INVITES,
            GuildPermissions::MANAGE_PAGES,
            GuildPermissions::SCREEN_SHARE,
            GuildPermissions::MENTION_EVERYONE,
            GuildPermissions::VIEW_CHANNEL,
        ];

        // Check that combining all equals the sum of individual bits
        let combined: u64 = all_perms.iter().fold(0, |acc, p| acc | p.bits());
        let sum: u64 = all_perms.iter().map(|p| p.bits()).sum();

        assert_eq!(combined, sum, "Some permissions share the same bit!");
    }

    // === Serde Tests ===
    // Note: bitflags with serde feature uses human-readable flag names

    #[test]
    fn test_serialize_single_permission() {
        let perms = GuildPermissions::SEND_MESSAGES;
        let json = serde_json::to_string(&perms).unwrap();
        // bitflags serde serializes as human-readable flag names
        assert_eq!(json, "\"SEND_MESSAGES\"");
    }

    #[test]
    fn test_serialize_multiple_permissions() {
        let perms = GuildPermissions::SEND_MESSAGES | GuildPermissions::VOICE_CONNECT;
        let json = serde_json::to_string(&perms).unwrap();
        // bitflags serde serializes as pipe-separated flag names
        assert_eq!(json, "\"SEND_MESSAGES | VOICE_CONNECT\"");
    }

    #[test]
    fn test_serialize_empty_permissions() {
        let perms = GuildPermissions::empty();
        let json = serde_json::to_string(&perms).unwrap();
        // Empty flags serialize as empty string
        assert_eq!(json, "\"\"");
    }

    #[test]
    fn test_deserialize_single_permission() {
        // bitflags serde accepts both formats
        let json = "\"SEND_MESSAGES\"";
        let perms: GuildPermissions = serde_json::from_str(json).unwrap();
        assert_eq!(perms, GuildPermissions::SEND_MESSAGES);
    }

    #[test]
    fn test_deserialize_multiple_permissions() {
        let json = "\"SEND_MESSAGES | VOICE_CONNECT\"";
        let perms: GuildPermissions = serde_json::from_str(json).unwrap();
        assert!(perms.has(GuildPermissions::SEND_MESSAGES));
        assert!(perms.has(GuildPermissions::VOICE_CONNECT));
    }

    #[test]
    fn test_serde_roundtrip() {
        let original = GuildPermissions::EVERYONE_DEFAULT;
        let json = serde_json::to_string(&original).unwrap();
        let restored: GuildPermissions = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_serde_roundtrip_all_permissions() {
        let original = GuildPermissions::all();
        let json = serde_json::to_string(&original).unwrap();
        let restored: GuildPermissions = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_db_value_differs_from_json() {
        // DB stores as numeric BIGINT, JSON uses human-readable format
        // These are intentionally different for different use cases
        let perms = GuildPermissions::SEND_MESSAGES;
        let db_value = perms.to_db();
        let json = serde_json::to_string(&perms).unwrap();
        assert_eq!(db_value, 1);
        assert_eq!(json, "\"SEND_MESSAGES\"");
    }
}
