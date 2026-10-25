import { beforeEach, describe, expect, it, vi } from "vitest";

const channelsState = { selectedChannelId: null as string | null, channels: [] as unknown[] };
const dmsState = { selectedDMId: null as string | null };
const selectChannel = vi.fn((id: string) => {
  channelsState.selectedChannelId = id;
});
const loadChannelsForGuild = vi.fn(async () => {
  channelsState.channels = [
    { id: "v1", channel_type: "voice", position: 0 },
    { id: "t2", channel_type: "text", position: 2 },
    { id: "t1", channel_type: "text", position: 1 },
  ];
});

vi.mock("@/lib/tauri", () => ({ getGuildMembers: vi.fn(async () => []) }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/channels", () => ({
  channelsState,
  selectChannel: (id: string) => selectChannel(id),
  loadChannelsForGuild: () => loadChannelsForGuild(),
  textChannels: () =>
    (channelsState.channels as { channel_type: string; position: number }[])
      .filter((c) => c.channel_type === "text")
      .sort((a, b) => a.position - b.position),
}));
vi.mock("@/stores/dms", () => ({ dmsState }));
vi.mock("@/stores/voice", () => ({ voiceState: { channelId: null }, leaveVoice: vi.fn() }));

import { guildsState, setGuildsState, landOnFirstServerIfIdle } from "../guilds";

describe("landOnFirstServerIfIdle", () => {
  beforeEach(() => {
    selectChannel.mockClear();
    loadChannelsForGuild.mockClear();
    channelsState.selectedChannelId = null;
    channelsState.channels = [];
    dmsState.selectedDMId = null;
    window.history.pushState({}, "", "/");
    setGuildsState({
      guilds: [{ id: "g1", name: "Family" }, { id: "g2", name: "Other" }] as never,
      activeGuildId: null,
    });
  });

  it("lands on the first text channel when nothing is selected", async () => {
    landOnFirstServerIfIdle();
    await vi.waitFor(() => expect(selectChannel).toHaveBeenCalledWith("t1"));
    expect(guildsState.activeGuildId).toBe("g1");
  });

  it("does nothing when a server is already active", async () => {
    setGuildsState({ activeGuildId: "g2" });
    landOnFirstServerIfIdle();
    await new Promise((r) => setTimeout(r, 20));
    expect(loadChannelsForGuild).not.toHaveBeenCalled();
    expect(guildsState.activeGuildId).toBe("g2");
  });

  it("does nothing when a DM is selected", async () => {
    dmsState.selectedDMId = "dm1";
    landOnFirstServerIfIdle();
    await new Promise((r) => setTimeout(r, 20));
    expect(guildsState.activeGuildId).toBeNull();
  });

  it("does nothing when a deep link is pending", async () => {
    window.history.pushState({}, "", "/invite/abc");
    landOnFirstServerIfIdle();
    await new Promise((r) => setTimeout(r, 20));
    expect(guildsState.activeGuildId).toBeNull();
  });

  it("does nothing without servers", async () => {
    setGuildsState({ guilds: [] });
    landOnFirstServerIfIdle();
    await new Promise((r) => setTimeout(r, 20));
    expect(loadChannelsForGuild).not.toHaveBeenCalled();
  });
});
