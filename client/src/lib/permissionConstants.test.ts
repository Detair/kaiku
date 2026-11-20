import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  CHANNEL_OVERRIDE_PERMISSION_KEYS,
  EVERYONE_DEFAULT,
  EVERYONE_FORBIDDEN,
  PERMISSIONS,
  PermissionBits,
  addPermission,
  hasPermission,
  isValidForEveryone,
  mergeShownBits,
  removePermission,
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
 * The server's bit table, read from its source (`server/src/permissions/
 * guild.rs`), so a change of a bit or a name on either side fails here.
 */
function readServerBits(): Record<string, number> {
  const source = readFileSync(
    resolve(__dirname, "../../../server/src/permissions/guild.rs"),
    "utf8",
  );
  const bits: Record<string, number> = {};
  for (const m of source.matchAll(/^\s*const ([A-Z_]+)\s*=\s*1 << (\d+);/gm)) {
    bits[m[1]] = Number(m[2]);
  }
  return bits;
}
const SERVER_BITS = readServerBits();

/**
 * The server's @everyone sets as of the screen-share permission change
 * (SCREEN_SHARE on by default, allowed for @everyone). They are a copy:
 * that server change ships in its own package.
 */
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
  it("reads the server's whole table", () => {
    expect(Object.keys(SERVER_BITS)).toHaveLength(30);
  });

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

describe("permission mask helpers keep bits above 31", () => {
  const high = 2 ** 40;

  it("adds and removes one bit without touching the others", () => {
    const stored = high + PermissionBits.SCREEN_SHARE;
    expect(addPermission(stored, PermissionBits.SEND_MESSAGES)).toBe(
      stored + PermissionBits.SEND_MESSAGES,
    );
    expect(removePermission(stored, PermissionBits.SCREEN_SHARE)).toBe(high);
    expect(hasPermission(stored, high)).toBe(true);
    expect(hasPermission(stored, PermissionBits.SEND_MESSAGES)).toBe(false);
  });

  it("replaces only the shown bits of a mask", () => {
    const shown = PermissionBits.SEND_MESSAGES | PermissionBits.VIEW_CHANNEL;
    const stored = high + PermissionBits.SCREEN_SHARE + PermissionBits.SEND_MESSAGES;
    expect(mergeShownBits(stored, shown, PermissionBits.VIEW_CHANNEL)).toBe(
      high + PermissionBits.SCREEN_SHARE + PermissionBits.VIEW_CHANNEL,
    );
  });

  it("ignores edited bits that are not shown", () => {
    expect(mergeShownBits(0, PermissionBits.SEND_MESSAGES, PermissionBits.MANAGE_GUILD)).toBe(0);
  });
});
