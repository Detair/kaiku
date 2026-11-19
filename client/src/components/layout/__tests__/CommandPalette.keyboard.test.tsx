import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: vi.fn(),
  getServerUrl: vi.fn(() => "https://chat.example"),
}));
const mocks = vi.hoisted(() => ({ selectChannel: vi.fn() }));
vi.mock("@/stores/channels", async () => {
  const actual = await vi.importActual<typeof import("@/stores/channels")>("@/stores/channels");
  return {
    ...actual,
    channelsState: { channels: [{ id: "t", name: "general", channel_type: "text", position: 0 }] },
    selectChannel: mocks.selectChannel,
  };
});
vi.mock("@/stores/search", () => ({ setShowGlobalSearch: vi.fn() }));
vi.mock("@/stores/voice", () => ({
  toggleMute: vi.fn(),
  toggleDeafen: vi.fn(),
  voiceState: { state: "disconnected", muted: false, deafened: false },
}));

import CommandPalette from "../CommandPalette";
import Modal from "@/components/ui/Modal";
import { setAuthState } from "@/stores/auth";

afterEach(() => {
  cleanup();
  setAuthState({ sessionExpired: false });
  document.body.innerHTML = "";
  vi.clearAllMocks();
});

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("CommandPalette keyboard", () => {
  it("does not open while the session-ended notice is up", async () => {
    render(() => <CommandPalette />);
    setAuthState({ sessionExpired: true });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await wait(30);
    expect(screen.queryByTestId("command-palette")).toBeNull();
  });

  it("leaves Enter to a window opened above it", async () => {
    render(() => <CommandPalette />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByTestId("command-palette");
    await wait(20);

    // For example a window that failed to open, asked for with Ctrl+/.
    const onClick = vi.fn();
    render(() => (
      <Modal onClose={() => {}} label="On top">
        <button type="button" onClick={onClick}>
          Close
        </button>
      </Modal>
    ));
    await wait(0);
    const button = screen.getByRole("button", { name: "Close" });
    expect(document.activeElement).toBe(button);

    // Not cancelled, so the browser clicks the focused button.
    expect(fireEvent.keyDown(button, { key: "Enter" })).toBe(true);
    expect(fireEvent.keyDown(button, { key: "ArrowDown" })).toBe(true);
    expect(mocks.selectChannel).not.toHaveBeenCalled();
    expect(screen.getByTestId("command-palette")).toBeTruthy();
  });

  it("still runs the selected entry on Enter while it has the keyboard", async () => {
    render(() => <CommandPalette />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByTestId("command-palette");
    await wait(20);
    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
    expect(mocks.selectChannel).toHaveBeenCalledWith("t");
  });
});
