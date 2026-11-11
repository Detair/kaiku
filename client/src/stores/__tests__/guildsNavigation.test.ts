// NAV-6/NAV-7: moving around the app never ends a running voice session.
// Home, another server and "Explore Servers" only change the view; leaving
// is always the user's own click on "Disconnect".
import { beforeEach, describe, expect, it, vi } from "vitest";

const channelsState = {
  selectedChannelId: null as string | null,
  // The loaders below leave this list alone, so the voice channel of the
  // previous server is still findable when the navigation code runs.
  channels: [{ id: "voice-1", guild_id: "g1", channel_type: "voice" }] as unknown[],
};
const leaveVoice = vi.fn(async () => {});

vi.mock("@/lib/tauri", () => ({ getGuildMembers: vi.fn(async () => []) }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/channels", () => ({
  channelsState,
  loadChannelsForGuild: vi.fn(async () => {}),
  loadDMChannels: vi.fn(async () => {}),
}));
vi.mock("@/stores/voice", () => ({
  voiceState: { channelId: "voice-1" },
  leaveVoice: () => leaveVoice(),
}));

import { guildsState, setGuildsState, selectGuild, selectHome, selectDiscovery } from "../guilds";

describe("navigation keeps the voice session", () => {
  beforeEach(() => {
    leaveVoice.mockClear();
    setGuildsState({
      guilds: [{ id: "g1", name: "Family" }, { id: "g2", name: "Friends" }] as never,
      activeGuildId: "g1",
    });
  });

  it("stays connected when the user opens Home", async () => {
    await selectHome();
    expect(guildsState.activeGuildId).toBeNull();
    expect(leaveVoice).not.toHaveBeenCalled();
  });

  it("stays connected when the user opens another server", async () => {
    await selectGuild("g2");
    expect(guildsState.activeGuildId).toBe("g2");
    expect(leaveVoice).not.toHaveBeenCalled();
  });

  it("stays connected when the user opens Explore Servers", async () => {
    await selectDiscovery();
    expect(leaveVoice).not.toHaveBeenCalled();
  });
});
