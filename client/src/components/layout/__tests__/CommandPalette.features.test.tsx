import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));
vi.mock("@/stores/channels", async () => {
  const actual = await vi.importActual<typeof import("@/stores/channels")>("@/stores/channels");
  const ch = (id: string, name: string, channel_type: string) => ({ id, name, channel_type, position: 0 });
  return {
    ...actual,
    channelsState: { channels: [ch("t", "general", "text"), ch("f", "forumchan", "forum")] },
    selectChannel: vi.fn(),
  };
});
vi.mock("@/stores/search", () => ({ setShowGlobalSearch: vi.fn() }));
vi.mock("@/stores/voice", () => ({
  toggleMute: vi.fn(),
  toggleDeafen: vi.fn(),
  voiceState: { state: "disconnected", muted: false, deafened: false },
}));

import * as tauri from "@/lib/tauri";
import { loadServerFeatures, resetServerFeatures } from "@/stores/serverFeatures";
import CommandPalette from "../CommandPalette";

describe("CommandPalette", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    resetServerFeatures();
  });

  it("does not list channels of a type the server turned off", async () => {
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue({
      features: { forum_channels: false, announcement_channels: false, scheduled_events: false, membership_screening: false },
    } as never);
    await loadServerFeatures();
    render(() => <CommandPalette />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(await screen.findByText("# general")).toBeTruthy();
    expect(screen.queryByText("# forumchan")).toBeNull();
  });
});
