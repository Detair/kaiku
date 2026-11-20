import { describe, expect, it } from "vitest";
import {
  CHANNEL_OVERRIDE_PERMISSION_KEYS,
  EVERYONE_DEFAULT,
  EVERYONE_FORBIDDEN,
  PERMISSIONS,
  PermissionBits,
  isValidForEveryone,
} from "./permissionConstants";

describe("channel override permission constants", () => {
  it("includes VIEW_CHANNEL in channel override keys", () => {
    expect(CHANNEL_OVERRIDE_PERMISSION_KEYS).toContain("VIEW_CHANNEL");
  });

  it("excludes guild-level permissions from channel override keys", () => {
    expect(CHANNEL_OVERRIDE_PERMISSION_KEYS).not.toContain("MANAGE_GUILD");
    expect(CHANNEL_OVERRIDE_PERMISSION_KEYS).not.toContain("MANAGE_ROLES");
    expect(CHANNEL_OVERRIDE_PERMISSION_KEYS).not.toContain("MANAGE_PAGES");
  });

  it("maps all override keys to known permission bits", () => {
    const bits = CHANNEL_OVERRIDE_PERMISSION_KEYS.map(
      (key) => PermissionBits[key],
    );
    expect(bits.every((bit) => typeof bit === "number")).toBe(true);

    const keysInPermissions = new Set(PERMISSIONS.map((perm) => perm.key));
    expect(
      CHANNEL_OVERRIDE_PERMISSION_KEYS.every((key) =>
        keysInPermissions.has(key),
      ),
    ).toBe(true);
  });
});

/**
 * Copy of the server's table (`server/src/permissions/guild.rs`, 2026-11-20):
 * bit per name, EVERYONE_DEFAULT, EVERYONE_FORBIDDEN, EVERYONE_ALLOWED.
 * A change on either side must change this copy too.
 */
const SERVER_BITS: Record<string, number> = {
  SEND_MESSAGES: 0,
  EMBED_LINKS: 1,
  ATTACH_FILES: 2,
  USE_EMOJI: 3,
  ADD_REACTIONS: 4,
  VOICE_CONNECT: 5,
  VOICE_SPEAK: 6,
  VOICE_MUTE_OTHERS: 7,
  VOICE_DEAFEN_OTHERS: 8,
  VOICE_MOVE_MEMBERS: 9,
  MANAGE_MESSAGES: 10,
  TIMEOUT_MEMBERS: 11,
  KICK_MEMBERS: 12,
  BAN_MEMBERS: 13,
  MANAGE_CHANNELS: 14,
  MANAGE_ROLES: 15,
  VIEW_AUDIT_LOG: 16,
  MANAGE_GUILD: 17,
  TRANSFER_OWNERSHIP: 18,
  CREATE_INVITE: 19,
  MANAGE_INVITES: 20,
  MANAGE_PAGES: 21,
  SCREEN_SHARE: 22,
  MENTION_EVERYONE: 23,
  VIEW_CHANNEL: 24,
  PIN_MESSAGES: 25,
  MANAGE_POSTS: 26,
  SEND_ANNOUNCEMENTS: 27,
  MANAGE_EVENTS: 28,
  MANAGE_WEBHOOKS: 29,
};
const SERVER_EVERYONE_DEFAULT = [
  "VIEW_CHANNEL", "SEND_MESSAGES", "EMBED_LINKS", "ATTACH_FILES", "USE_EMOJI",
  "ADD_REACTIONS", "VOICE_CONNECT", "VOICE_SPEAK", "CREATE_INVITE", "SCREEN_SHARE",
];
const SERVER_EVERYONE_ALLOWED = [
  "SEND_MESSAGES", "EMBED_LINKS", "ATTACH_FILES", "USE_EMOJI", "ADD_REACTIONS",
  "VOICE_CONNECT", "VOICE_SPEAK", "CREATE_INVITE", "VIEW_CHANNEL", "SCREEN_SHARE",
];

const maskOf = (names: string[]) => names.reduce((m, n) => m | (1 << SERVER_BITS[n]), 0);

describe("permission table matches the server", () => {
  it("every bit has the server's name", () => {
    expect(PermissionBits).toEqual(
      Object.fromEntries(Object.entries(SERVER_BITS).map(([k, b]) => [k, 1 << b])),
    );
  });

  it("@everyone default, forbidden and allowed sets match", () => {
    expect(EVERYONE_DEFAULT).toBe(maskOf(SERVER_EVERYONE_DEFAULT));
    const forbidden = Object.keys(SERVER_BITS).filter((k) => !SERVER_EVERYONE_ALLOWED.includes(k));
    expect(EVERYONE_FORBIDDEN).toBe(maskOf(forbidden));
  });

  it("every permission is offered in the editors with the server's @everyone rule", () => {
    const defs = new Map(PERMISSIONS.map((p) => [p.key, p]));
    for (const key of Object.keys(SERVER_BITS)) {
      const def = defs.get(key as keyof typeof PermissionBits);
      expect(def, key).toBeDefined();
      expect(def!.bit).toBe(1 << SERVER_BITS[key]);
      expect(def!.forbiddenForEveryone, key).toBe(!SERVER_EVERYONE_ALLOWED.includes(key));
    }
  });

  it("screen sharing is on for @everyone and can be denied per channel", () => {
    expect(isValidForEveryone(EVERYONE_DEFAULT)).toBe(true);
    expect(EVERYONE_DEFAULT & PermissionBits.SCREEN_SHARE).toBe(PermissionBits.SCREEN_SHARE);
    expect(CHANNEL_OVERRIDE_PERMISSION_KEYS).toContain("SCREEN_SHARE");
  });
});

describe("permission bits stay inside 32-bit integer math", () => {
  it("no bit above 30 (mask operations use JS int32)", () => {
    for (const [name, bit] of Object.entries(PermissionBits)) {
      expect(bit, name).toBeGreaterThan(0);
      expect(bit, name).toBeLessThanOrEqual(1 << 30);
    }
  });
});
