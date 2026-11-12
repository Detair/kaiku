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

  it("is a dialog with a combobox: focus in, arrows mark an option, Escape closes and focus returns (A11Y-1)", async () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    render(() => <CommandPalette />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const dialog = await screen.findByRole("dialog");
    await new Promise((r) => setTimeout(r, 20));
    const box = screen.getByRole("combobox");
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).toBe(box);
    const first = screen.getAllByRole("option")[0];
    expect(box.getAttribute("aria-activedescendant")).toBe(first.id);
    expect(first.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(box, { key: "ArrowDown" });
    expect(box.getAttribute("aria-activedescendant")).toBe(screen.getAllByRole("option")[1].id);
    fireEvent.keyDown(box, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });
});
