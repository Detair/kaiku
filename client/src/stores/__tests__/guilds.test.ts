import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/tauri", () => ({
  getGuilds: vi.fn(),
  getGuildMembers: vi.fn(),
  getGuildChannels: vi.fn(),
  createGuild: vi.fn(),
  updateGuild: vi.fn(),
  deleteGuild: vi.fn(),
  joinGuild: vi.fn(),
  leaveGuild: vi.fn(),
  getGuildInvites: vi.fn(),
  createGuildInvite: vi.fn(),
  deleteGuildInvite: vi.fn(),
  joinViaInvite: vi.fn(),
  kickGuildMember: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
}));

// Dynamic imports used by selectGuild / selectHome — mock the modules
vi.mock("./channels", () => ({
  loadChannelsForGuild: vi.fn(),
  loadDMChannels: vi.fn(),
  channelsState: { channels: [] },
}));

vi.mock("./voice", () => ({
  voiceState: { channelId: null },
  leaveVoice: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import type { Guild, GuildMember, GuildInvite } from "@/lib/types";
import {
  guildsState,
  setGuildsState,
  loadGuilds,
  loadGuildMembers,
  getActiveGuild,
  createGuild,
  updateGuild,
  deleteGuild,
  joinGuild,
  leaveGuild,
  loadGuildInvites,
  createInvite,
  deleteInvite,
  kickMember,
  isGuildOwner,
  patchGuild,
  areThreadsEnabled,
  getGuildUnreadCount,
  incrementGuildUnread,
  clearGuildUnread,
  getGuildIdForChannel,
  joinViaInviteCode,
  selectGuild,
  selectHome,
} from "../guilds";

function createGuildObj(overrides: Partial<Guild> = {}): Guild {
  return {
    id: "guild-1",
    name: "Test Guild",
    owner_id: "owner-1",
    icon_url: null,
    description: null,
    threads_enabled: true,
    discoverable: false,
    tags: [],
    banner_url: null,
    created_at: "2025-01-01T00:00:00Z",
    ...overrides,
  };
}

function createMember(overrides: Partial<GuildMember> = {}): GuildMember {
  return {
    user_id: "user-1",
    username: "alice",
    display_name: "Alice",
    avatar_url: null,
    nickname: null,
    joined_at: "2025-01-01T00:00:00Z",
    status: "online",
    last_seen_at: null,
    ...overrides,
  };
}

function createInviteObj(overrides: Partial<GuildInvite> = {}): GuildInvite {
  return {
    id: "inv-1",
    guild_id: "guild-1",
    code: "ABC123",
    created_by: "owner-1",
    expires_at: null,
    use_count: 0,
    created_at: "2025-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("guilds store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setGuildsState({
      guilds: [],
      activeGuildId: null,
      members: {},
      invites: {},
      guildChannels: {},
      guildUnreadCounts: {},
      channelGuildMap: {},
      isLoading: false,
      isMembersLoading: false,
      isInvitesLoading: false,
      error: null,
    });
  });

  describe("initial state", () => {
    it("has empty arrays and no active guild", () => {
      expect(guildsState.guilds).toEqual([]);
      expect(guildsState.activeGuildId).toBeNull();
      expect(guildsState.isLoading).toBe(false);
      expect(guildsState.error).toBeNull();
    });
  });

  describe("loadGuilds", () => {
    it("loads guilds and prefetches unread counts", async () => {
      const guilds = [createGuildObj({ id: "g1" }), createGuildObj({ id: "g2" })];
      vi.mocked(tauri.getGuilds).mockResolvedValue(guilds);
      vi.mocked(tauri.getGuildChannels).mockResolvedValue([]);

      await loadGuilds();

      expect(guildsState.guilds).toEqual(guilds);
      expect(guildsState.isLoading).toBe(false);
      // getGuildChannels called for each guild during unread prefetch
      expect(tauri.getGuildChannels).toHaveBeenCalledTimes(2);
    });

    it("sets error on failure", async () => {
      vi.mocked(tauri.getGuilds).mockRejectedValue(new Error("fail"));

      await loadGuilds();

      expect(guildsState.error).toBe("fail");
      expect(guildsState.isLoading).toBe(false);
    });
  });

  describe("loadGuildMembers", () => {
    it("loads members for a guild", async () => {
      const members = [createMember()];
      vi.mocked(tauri.getGuildMembers).mockResolvedValue(members);

      await loadGuildMembers("guild-1");

      expect(guildsState.members["guild-1"]).toEqual(members);
      expect(guildsState.isMembersLoading).toBe(false);
    });
  });

  describe("getActiveGuild", () => {
    it("returns null when no guild active", () => {
      expect(getActiveGuild()).toBeNull();
    });

    it("returns the active guild", () => {
      setGuildsState({ guilds: [createGuildObj()], activeGuildId: "guild-1" });

      expect(getActiveGuild()?.id).toBe("guild-1");
    });
  });

  describe("createGuild", () => {
    it("creates and adds guild to store", async () => {
      const guild = createGuildObj({ id: "new-g" });
      vi.mocked(tauri.createGuild).mockResolvedValue(guild);

      const result = await createGuild("Test Guild");

      expect(result.id).toBe("new-g");
      expect(guildsState.guilds).toHaveLength(1);
    });
  });

  describe("updateGuild", () => {
    it("updates guild in store", async () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1", name: "Old" })] });
      const updated = createGuildObj({ id: "g1", name: "New" });
      vi.mocked(tauri.updateGuild).mockResolvedValue(updated);

      await updateGuild("g1", "New");

      expect(guildsState.guilds[0].name).toBe("New");
    });
  });

  describe("deleteGuild", () => {
    it("removes guild from store", async () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1" })] });
      vi.mocked(tauri.deleteGuild).mockResolvedValue(undefined);

      await deleteGuild("g1");

      expect(guildsState.guilds).toEqual([]);
    });

    it("selects home if deleted guild was active", async () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1" })], activeGuildId: "g1" });
      vi.mocked(tauri.deleteGuild).mockResolvedValue(undefined);

      await deleteGuild("g1");

      // selectHome sets activeGuildId to null
      expect(guildsState.activeGuildId).toBeNull();
    });
  });

  describe("joinGuild", () => {
    it("joins guild and reloads", async () => {
      vi.mocked(tauri.joinGuild).mockResolvedValue(undefined);
      vi.mocked(tauri.getGuilds).mockResolvedValue([createGuildObj()]);
      vi.mocked(tauri.getGuildChannels).mockResolvedValue([]);

      await joinGuild("g1", "invite123");

      expect(tauri.joinGuild).toHaveBeenCalledWith("g1", "invite123");
      expect(tauri.getGuilds).toHaveBeenCalled();
    });
  });

  describe("leaveGuild", () => {
    it("leaves guild and removes from store", async () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1" })] });
      vi.mocked(tauri.leaveGuild).mockResolvedValue(undefined);

      await leaveGuild("g1");

      expect(guildsState.guilds).toEqual([]);
    });
  });

  describe("loadGuildInvites", () => {
    it("loads invites for a guild", async () => {
      const invites = [createInviteObj()];
      vi.mocked(tauri.getGuildInvites).mockResolvedValue(invites);

      await loadGuildInvites("guild-1");

      expect(guildsState.invites["guild-1"]).toEqual(invites);
      expect(guildsState.isInvitesLoading).toBe(false);
    });
  });

  describe("createInvite", () => {
    it("creates invite and prepends to list", async () => {
      const invite = createInviteObj({ code: "NEW" });
      vi.mocked(tauri.createGuildInvite).mockResolvedValue(invite);

      const result = await createInvite("guild-1");

      expect(result.code).toBe("NEW");
      expect(guildsState.invites["guild-1"]).toHaveLength(1);
    });
  });

  describe("deleteInvite", () => {
    it("removes invite from list", async () => {
      setGuildsState("invites", "guild-1", [createInviteObj({ code: "ABC" })]);
      vi.mocked(tauri.deleteGuildInvite).mockResolvedValue(undefined);

      await deleteInvite("guild-1", "ABC");

      expect(guildsState.invites["guild-1"]).toEqual([]);
    });
  });

  describe("kickMember", () => {
    it("kicks member and removes from store", async () => {
      setGuildsState("members", "guild-1", [createMember({ user_id: "u1" })]);
      vi.mocked(tauri.kickGuildMember).mockResolvedValue(undefined);

      await kickMember("guild-1", "u1");

      expect(guildsState.members["guild-1"]).toEqual([]);
    });
  });

  describe("isGuildOwner", () => {
    it("returns true for owner", () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1", owner_id: "owner-1" })] });

      expect(isGuildOwner("g1", "owner-1")).toBe(true);
    });

    it("returns false for non-owner", () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1", owner_id: "owner-1" })] });

      expect(isGuildOwner("g1", "other")).toBe(false);
    });
  });

  describe("patchGuild", () => {
    it("updates valid fields", () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1", name: "Old" })] });

      patchGuild("g1", { name: "New", description: "desc" });

      expect(guildsState.guilds[0].name).toBe("New");
      expect(guildsState.guilds[0].description).toBe("desc");
    });

    it("ignores unknown fields", () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1" })] });

      patchGuild("g1", { invalid_field: "value" });

      // Should not throw and guild should be unchanged
      expect(guildsState.guilds[0].name).toBe("Test Guild");
    });

    it("ignores unknown guild", () => {
      patchGuild("unknown", { name: "New" });

      // No error expected
    });
  });

  describe("areThreadsEnabled", () => {
    it("returns true for null/undefined guild ID (DMs)", () => {
      expect(areThreadsEnabled(null)).toBe(true);
      expect(areThreadsEnabled(undefined)).toBe(true);
    });

    it("returns guild's threads_enabled setting", () => {
      setGuildsState({ guilds: [createGuildObj({ id: "g1", threads_enabled: false })] });

      expect(areThreadsEnabled("g1")).toBe(false);
    });

    it("defaults to true for unknown guild", () => {
      expect(areThreadsEnabled("unknown")).toBe(true);
    });
  });

  describe("unread count management", () => {
    it("getGuildUnreadCount returns 0 by default", () => {
      expect(getGuildUnreadCount("g1")).toBe(0);
    });

    it("incrementGuildUnread increments count", () => {
      incrementGuildUnread("g1");
      incrementGuildUnread("g1");

      expect(getGuildUnreadCount("g1")).toBe(2);
    });

    it("clearGuildUnread resets to 0", () => {
      setGuildsState("guildUnreadCounts", "g1", 5);

      clearGuildUnread("g1");

      expect(getGuildUnreadCount("g1")).toBe(0);
    });
  });

  describe("getGuildIdForChannel", () => {
    it("returns guild ID from channel map", () => {
      setGuildsState("channelGuildMap", "ch-1", "g1");

      expect(getGuildIdForChannel("ch-1")).toBe("g1");
    });

    it("returns undefined for unmapped channel", () => {
      expect(getGuildIdForChannel("unknown")).toBeUndefined();
    });
  });

  describe("joinViaInviteCode", () => {
    it("joins via invite code, reloads guilds and selects the joined guild", async () => {
      vi.mocked(tauri.joinViaInvite).mockResolvedValue({ guild_id: "g1" } as any);
      vi.mocked(tauri.getGuilds).mockResolvedValue([createGuildObj({ id: "g1" })]);
      vi.mocked(tauri.getGuildChannels).mockResolvedValue([]);
      vi.mocked(tauri.getGuildMembers).mockResolvedValue([]);

      await joinViaInviteCode("INVITE");

      expect(tauri.joinViaInvite).toHaveBeenCalledWith("INVITE");
    });
  });

  describe("selectGuild", () => {
    it("sets active guild and clears unread", async () => {
      setGuildsState({ guildUnreadCounts: { "g1": 5 } });
      vi.mocked(tauri.getGuildChannels).mockResolvedValue([]);
      vi.mocked(tauri.getGuildMembers).mockResolvedValue([]);

      await selectGuild("g1");

      expect(guildsState.activeGuildId).toBe("g1");
      expect(guildsState.guildUnreadCounts["g1"]).toBe(0);
    });
  });

  describe("selectHome", () => {
    it("clears active guild", async () => {
      setGuildsState({ activeGuildId: "g1" });

      await selectHome();

      expect(guildsState.activeGuildId).toBeNull();
    });
  });
});
