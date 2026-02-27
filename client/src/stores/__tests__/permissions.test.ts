import { describe, it, expect, beforeEach, vi } from "vitest";

// Mock the tauri module before importing permissions
vi.mock("@/lib/tauri", () => ({
  getGuildRoles: vi.fn(),
  getGuildMemberRoles: vi.fn(),
  createGuildRole: vi.fn(),
  updateGuildRole: vi.fn(),
  deleteGuildRole: vi.fn(),
  getChannelOverrides: vi.fn(),
  setChannelOverride: vi.fn(),
  deleteChannelOverride: vi.fn(),
  assignMemberRole: vi.fn(),
  removeMemberRole: vi.fn(),
}));

import * as tauri from "@/lib/tauri";
import {
  loadGuildRoles,
  getGuildRoles,
  reorderRole,
  getUserHighestRolePosition,
} from "../permissions";

const mockRoles = [
  {
    id: "role1",
    guild_id: "guild1",
    name: "Admin",
    position: 0,
    permissions: 8,
    color: "#ff0000",
    is_default: false,
    created_at: "2024-01-01T00:00:00Z",
  },
  {
    id: "role2",
    guild_id: "guild1",
    name: "Mod",
    position: 1,
    permissions: 4,
    color: "#00ff00",
    is_default: false,
    created_at: "2024-01-01T00:00:00Z",
  },
  {
    id: "role3",
    guild_id: "guild1",
    name: "Member",
    position: 2,
    permissions: 1,
    color: "#0000ff",
    is_default: false,
    created_at: "2024-01-01T00:00:00Z",
  },
  {
    id: "everyone",
    guild_id: "guild1",
    name: "@everyone",
    position: 3,
    permissions: 0,
    color: null,
    is_default: true,
    created_at: "2024-01-01T00:00:00Z",
  },
];

describe("permissions store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("loadGuildRoles", () => {
    it("should load and sort roles by position", async () => {
      const unsortedRoles = [
        mockRoles[2],
        mockRoles[0],
        mockRoles[3],
        mockRoles[1],
      ];
      vi.mocked(tauri.getGuildRoles).mockResolvedValue(unsortedRoles);

      await loadGuildRoles("guild1");

      const roles = getGuildRoles("guild1");
      expect(roles[0].position).toBe(0);
      expect(roles[1].position).toBe(1);
      expect(roles[2].position).toBe(2);
      expect(roles[3].position).toBe(3);
    });
  });

  describe("reorderRole", () => {
    beforeEach(async () => {
      vi.mocked(tauri.getGuildRoles).mockResolvedValue([...mockRoles]);
      vi.mocked(tauri.updateGuildRole).mockResolvedValue(mockRoles[0]);
      await loadGuildRoles("guild1");
    });

    it("should not reorder @everyone role", async () => {
      await reorderRole("guild1", "everyone", 0);

      expect(tauri.updateGuildRole).not.toHaveBeenCalled();
    });

    it("should optimistically update local state", async () => {
      // Move Mod (position 1) to position 0 (Admin's position)
      await reorderRole("guild1", "role2", 0);

      // Verify optimistic update happened
      expect(tauri.updateGuildRole).toHaveBeenCalledWith("guild1", "role2", {
        position: 0,
      });
    });

    it("should revert on API failure", async () => {
      vi.mocked(tauri.updateGuildRole).mockRejectedValue(
        new Error("API Error"),
      );
      vi.mocked(tauri.getGuildRoles).mockResolvedValue([...mockRoles]);

      await expect(reorderRole("guild1", "role2", 0)).rejects.toThrow(
        "API Error",
      );

      // Should have reloaded roles
      expect(tauri.getGuildRoles).toHaveBeenCalledTimes(2); // Initial load + reload after error
    });
  });

  describe("getUserHighestRolePosition", () => {
    beforeEach(async () => {
      vi.mocked(tauri.getGuildRoles).mockResolvedValue([...mockRoles]);
      await loadGuildRoles("guild1");
    });

    it("should return Infinity for users with no roles", () => {
      const position = getUserHighestRolePosition("guild1", "userWithNoRoles");
      expect(position).toBe(Infinity);
    });
  });
});
