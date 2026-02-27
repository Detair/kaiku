import { describe, expect, it } from "vitest";
import {
  CHANNEL_OVERRIDE_PERMISSION_KEYS,
  PERMISSIONS,
  PermissionBits,
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
