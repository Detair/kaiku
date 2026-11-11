/**
 * Codex Major 3 / K7: a channel rename or delete that worked is not reported
 * as failed when only the list refresh afterwards fails, and the local list
 * shows the result anyway (a retry would otherwise hit a 404).
 */
import { describe, it, expect, vi, beforeEach } from "vitest";

const m = vi.hoisted(() => ({
  renameChannel: vi.fn(),
  deleteChannel: vi.fn(),
  getGuildChannels: vi.fn(),
}));
vi.mock("@/lib/tauri/channelAdmin", () => ({ renameChannel: m.renameChannel, deleteChannel: m.deleteChannel }));
vi.mock("@/lib/tauri", async (orig) => ({ ...(await orig<object>()), getGuildChannels: m.getGuildChannels }));

import { renameChannel, deleteChannel } from "../guildAdmin";
import { channelsState, setChannelsState } from "../channels";

beforeEach(() => {
  vi.clearAllMocks();
  setChannelsState({
    channels: [{ id: "c1", name: "allgemein", guild_id: "g1", channel_type: "text" } as never],
    selectedChannelId: "c1",
  });
  m.getGuildChannels.mockRejectedValue(new TypeError("Failed to fetch"));
});

describe("guildAdmin channel actions", () => {
  it("rename succeeds and updates the list even when the refresh fails", async () => {
    m.renameChannel.mockResolvedValue({ id: "c1", name: "familie" });
    await expect(renameChannel("g1", "c1", "familie")).resolves.toBeUndefined();
    expect(channelsState.channels[0].name).toBe("familie");
  });

  it("delete succeeds and removes the channel even when the refresh fails", async () => {
    m.deleteChannel.mockResolvedValue(undefined);
    await expect(deleteChannel("g1", "c1")).resolves.toBeUndefined();
    expect(channelsState.channels.find((c) => c.id === "c1")).toBeUndefined();
    expect(channelsState.selectedChannelId).toBeNull();
  });

  it("a failed delete still fails", async () => {
    m.deleteChannel.mockRejectedValue(new TypeError("Failed to fetch"));
    await expect(deleteChannel("g1", "c1")).rejects.toThrow();
    expect(channelsState.channels).toHaveLength(1);
  });
});
