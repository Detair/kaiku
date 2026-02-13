/**
 * Permission Constants
 *
 * These permission bits must match the server-side GuildPermissions bitflags
 * defined in server/src/permissions/guild.rs
 */

// Permission bit values
export const PermissionBits = {
  // Content (bits 0-4)
  SEND_MESSAGES: 1 << 0,
  EMBED_LINKS: 1 << 1,
  ATTACH_FILES: 1 << 2,
  USE_EMOJI: 1 << 3,
  ADD_REACTIONS: 1 << 4,

  // Voice (bits 5-9)
  VOICE_CONNECT: 1 << 5,
  VOICE_SPEAK: 1 << 6,
  VOICE_MUTE_OTHERS: 1 << 7,
  VOICE_DEAFEN_OTHERS: 1 << 8,
  VOICE_MOVE_MEMBERS: 1 << 9,

  // Moderation (bits 10-13)
  MANAGE_MESSAGES: 1 << 10,
  TIMEOUT_MEMBERS: 1 << 11,
  KICK_MEMBERS: 1 << 12,
  BAN_MEMBERS: 1 << 13,

  // Guild Management (bits 14-18)
  MANAGE_CHANNELS: 1 << 14,
  MANAGE_ROLES: 1 << 15,
  VIEW_AUDIT_LOG: 1 << 16,
  MANAGE_GUILD: 1 << 17,
  TRANSFER_OWNERSHIP: 1 << 18,

  // Invites (bits 19-20)
  CREATE_INVITE: 1 << 19,
  MANAGE_INVITES: 1 << 20,

  // Pages (bit 21)
  MANAGE_PAGES: 1 << 21,
  MANAGE_EMOJIS_AND_STICKERS: 1 << 22,

  // Mentions (bit 23)
  MENTION_EVERYONE: 1 << 23,

  // Channel access (bit 24)
  VIEW_CHANNEL: 1 << 24,
} as const;

export type PermissionBit = (typeof PermissionBits)[keyof typeof PermissionBits];

// Permission categories for UI grouping
export type PermissionCategory =
  | "content"
  | "voice"
  | "moderation"
  | "guild_management"
  | "invites"
  | "pages";

// Permission definition for UI display
export interface PermissionDefinition {
  key: keyof typeof PermissionBits;
  bit: number;
  name: string;
  description: string;
  category: PermissionCategory;
  forbiddenForEveryone: boolean;
}

export const CHANNEL_OVERRIDE_PERMISSION_KEYS = [
  "VIEW_CHANNEL",
  "SEND_MESSAGES",
  "EMBED_LINKS",
  "ATTACH_FILES",
  "ADD_REACTIONS",
  "USE_EMOJI",
  "MANAGE_MESSAGES",
  "MENTION_EVERYONE",
  "VOICE_CONNECT",
  "VOICE_SPEAK",
  "VOICE_MUTE_OTHERS",
  "VOICE_DEAFEN_OTHERS",
  "VOICE_MOVE_MEMBERS",
  "CREATE_INVITE",
] as const;

// All permissions with their definitions
export const PERMISSIONS: PermissionDefinition[] = [
  // Content permissions
  {
    key: "VIEW_CHANNEL",
    bit: PermissionBits.VIEW_CHANNEL,
    name: "View Channel",
    description: "Allows viewing channels and reading their message history",
    category: "content",
    forbiddenForEveryone: false,
  },
  {
    key: "SEND_MESSAGES",
    bit: PermissionBits.SEND_MESSAGES,
    name: "Send Messages",
    description: "Allows sending text messages in channels",
    category: "content",
    forbiddenForEveryone: false,
  },
  {
    key: "EMBED_LINKS",
    bit: PermissionBits.EMBED_LINKS,
    name: "Embed Links",
    description: "Allows auto-preview of links in messages",
    category: "content",
    forbiddenForEveryone: false,
  },
  {
    key: "ATTACH_FILES",
    bit: PermissionBits.ATTACH_FILES,
    name: "Attach Files",
    description: "Allows attaching files to messages",
    category: "content",
    forbiddenForEveryone: false,
  },
  {
    key: "USE_EMOJI",
    bit: PermissionBits.USE_EMOJI,
    name: "Use Emoji",
    description: "Allows using custom emoji",
    category: "content",
    forbiddenForEveryone: false,
  },
  {
    key: "ADD_REACTIONS",
    bit: PermissionBits.ADD_REACTIONS,
    name: "Add Reactions",
    description: "Allows adding reactions to messages",
    category: "content",
    forbiddenForEveryone: false,
  },

  // Voice permissions
  {
    key: "VOICE_CONNECT",
    bit: PermissionBits.VOICE_CONNECT,
    name: "Connect",
    description: "Allows connecting to voice channels",
    category: "voice",
    forbiddenForEveryone: false,
  },
  {
    key: "VOICE_SPEAK",
    bit: PermissionBits.VOICE_SPEAK,
    name: "Speak",
    description: "Allows speaking in voice channels",
    category: "voice",
    forbiddenForEveryone: false,
  },
  {
    key: "VOICE_MUTE_OTHERS",
    bit: PermissionBits.VOICE_MUTE_OTHERS,
    name: "Mute Members",
    description: "Allows muting other members in voice channels",
    category: "voice",
    forbiddenForEveryone: true,
  },
  {
    key: "VOICE_DEAFEN_OTHERS",
    bit: PermissionBits.VOICE_DEAFEN_OTHERS,
    name: "Deafen Members",
    description: "Allows deafening other members in voice channels",
    category: "voice",
    forbiddenForEveryone: true,
  },
  {
    key: "VOICE_MOVE_MEMBERS",
    bit: PermissionBits.VOICE_MOVE_MEMBERS,
    name: "Move Members",
    description: "Allows moving members between voice channels",
    category: "voice",
    forbiddenForEveryone: true,
  },

  // Moderation permissions
  {
    key: "MANAGE_MESSAGES",
    bit: PermissionBits.MANAGE_MESSAGES,
    name: "Manage Messages",
    description: "Allows deleting messages from other members",
    category: "moderation",
    forbiddenForEveryone: true,
  },
  {
    key: "TIMEOUT_MEMBERS",
    bit: PermissionBits.TIMEOUT_MEMBERS,
    name: "Timeout Members",
    description: "Allows temporarily muting members",
    category: "moderation",
    forbiddenForEveryone: true,
  },
  {
    key: "KICK_MEMBERS",
    bit: PermissionBits.KICK_MEMBERS,
    name: "Kick Members",
    description: "Allows kicking members from the server",
    category: "moderation",
    forbiddenForEveryone: true,
  },
  {
    key: "BAN_MEMBERS",
    bit: PermissionBits.BAN_MEMBERS,
    name: "Ban Members",
    description: "Allows banning members from the server",
    category: "moderation",
    forbiddenForEveryone: true,
  },

  // Guild management permissions
  {
    key: "MANAGE_CHANNELS",
    bit: PermissionBits.MANAGE_CHANNELS,
    name: "Manage Channels",
    description: "Allows creating, editing, and deleting channels",
    category: "guild_management",
    forbiddenForEveryone: true,
  },
  {
    key: "MANAGE_ROLES",
    bit: PermissionBits.MANAGE_ROLES,
    name: "Manage Roles",
    description: "Allows creating, editing, and deleting roles",
    category: "guild_management",
    forbiddenForEveryone: true,
  },
  {
    key: "VIEW_AUDIT_LOG",
    bit: PermissionBits.VIEW_AUDIT_LOG,
    name: "View Audit Log",
    description: "Allows viewing the server audit log",
    category: "guild_management",
    forbiddenForEveryone: true,
  },
  {
    key: "MANAGE_GUILD",
    bit: PermissionBits.MANAGE_GUILD,
    name: "Manage Server",
    description: "Allows modifying server settings",
    category: "guild_management",
    forbiddenForEveryone: true,
  },
  {
    key: "TRANSFER_OWNERSHIP",
    bit: PermissionBits.TRANSFER_OWNERSHIP,
    name: "Transfer Ownership",
    description: "Allows transferring server ownership (owner only)",
    category: "guild_management",
    forbiddenForEveryone: true,
  },

  // Invite permissions
  {
    key: "CREATE_INVITE",
    bit: PermissionBits.CREATE_INVITE,
    name: "Create Invite",
    description: "Allows creating invite links",
    category: "invites",
    forbiddenForEveryone: false,
  },
  {
    key: "MANAGE_INVITES",
    bit: PermissionBits.MANAGE_INVITES,
    name: "Manage Invites",
    description: "Allows revoking invite links",
    category: "invites",
    forbiddenForEveryone: true,
  },

  // Pages permission
  {
    key: "MANAGE_PAGES",
    bit: PermissionBits.MANAGE_PAGES,
    name: "Manage Pages",
    description: "Allows creating, editing, and deleting information pages",
    category: "pages",
    forbiddenForEveryone: true,
  },
  {
    key: "MANAGE_EMOJIS_AND_STICKERS",
    bit: PermissionBits.MANAGE_EMOJIS_AND_STICKERS,
    name: "Manage Emojis",
    description: "Allows managing custom guild emojis",
    category: "guild_management",
    forbiddenForEveryone: true,
  },
  {
    key: "MENTION_EVERYONE",
    bit: PermissionBits.MENTION_EVERYONE,
    name: "Mention @everyone",
    description: "Allows mentioning @everyone and @here",
    category: "content",
    forbiddenForEveryone: true,
  },
];

const CHANNEL_OVERRIDE_PERMISSION_KEY_SET = new Set<string>(
  CHANNEL_OVERRIDE_PERMISSION_KEYS
);

export const CHANNEL_OVERRIDE_PERMISSIONS = PERMISSIONS.filter((permission) =>
  CHANNEL_OVERRIDE_PERMISSION_KEY_SET.has(permission.key)
);

// Category display names
export const CATEGORY_NAMES: Record<PermissionCategory, string> = {
  content: "Content",
  voice: "Voice",
  moderation: "Moderation",
  guild_management: "Server Management",
  invites: "Invites",
  pages: "Information Pages",
};

// Get permissions by category
export function getPermissionsByCategory(
  category: PermissionCategory
): PermissionDefinition[] {
  return PERMISSIONS.filter((p) => p.category === category);
}

// Permission helper functions
export function hasPermission(permissions: number, bit: number): boolean {
  return (permissions & bit) === bit;
}

export function addPermission(permissions: number, bit: number): number {
  return permissions | bit;
}

export function removePermission(permissions: number, bit: number): number {
  return permissions & ~bit;
}

export function togglePermission(permissions: number, bit: number): number {
  return permissions ^ bit;
}

// Default permission presets (matching server)
export const EVERYONE_DEFAULT =
  PermissionBits.SEND_MESSAGES |
  PermissionBits.EMBED_LINKS |
  PermissionBits.ATTACH_FILES |
  PermissionBits.USE_EMOJI |
  PermissionBits.ADD_REACTIONS |
  PermissionBits.VOICE_CONNECT |
  PermissionBits.VOICE_SPEAK |
  PermissionBits.CREATE_INVITE;

export const MODERATOR_DEFAULT =
  EVERYONE_DEFAULT |
  PermissionBits.VOICE_MUTE_OTHERS |
  PermissionBits.VOICE_DEAFEN_OTHERS |
  PermissionBits.VOICE_MOVE_MEMBERS |
  PermissionBits.MANAGE_MESSAGES |
  PermissionBits.TIMEOUT_MEMBERS |
  PermissionBits.KICK_MEMBERS |
  PermissionBits.VIEW_AUDIT_LOG |
  PermissionBits.MANAGE_INVITES |
  PermissionBits.MENTION_EVERYONE;

export const OFFICER_DEFAULT =
  MODERATOR_DEFAULT |
  PermissionBits.BAN_MEMBERS |
  PermissionBits.MANAGE_CHANNELS |
  PermissionBits.MANAGE_PAGES;

// Permissions that @everyone can never have
export const EVERYONE_FORBIDDEN =
  PermissionBits.VOICE_MUTE_OTHERS |
  PermissionBits.VOICE_DEAFEN_OTHERS |
  PermissionBits.VOICE_MOVE_MEMBERS |
  PermissionBits.MANAGE_MESSAGES |
  PermissionBits.TIMEOUT_MEMBERS |
  PermissionBits.KICK_MEMBERS |
  PermissionBits.BAN_MEMBERS |
  PermissionBits.MANAGE_CHANNELS |
  PermissionBits.MANAGE_ROLES |
  PermissionBits.VIEW_AUDIT_LOG |
  PermissionBits.MANAGE_GUILD |
  PermissionBits.TRANSFER_OWNERSHIP |
  PermissionBits.MANAGE_INVITES |
  PermissionBits.MANAGE_PAGES |
  PermissionBits.MENTION_EVERYONE;

// Check if a permission is valid for @everyone role
export function isValidForEveryone(permissions: number): boolean {
  return (permissions & EVERYONE_FORBIDDEN) === 0;
}

// Get list of forbidden permissions for @everyone that are currently set
export function getForbiddenForEveryone(
  permissions: number
): PermissionDefinition[] {
  return PERMISSIONS.filter(
    (p) => p.forbiddenForEveryone && hasPermission(permissions, p.bit)
  );
}
