import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@solidjs/testing-library";

const hoisted = vi.hoisted(() => ({ native: false }));
const { voiceState, setVoiceState, stopScreenShare } = await vi.hoisted(async () => {
  const { createStore } = await import("solid-js/store");
  const [voiceState, setVoiceState] = createStore({
    state: "connected",
    screenSharing: false,
    screenShares: [] as unknown[],
  });
  return { voiceState, setVoiceState, stopScreenShare: vi.fn() };
});
vi.mock("@/stores/voice", () => ({
  voiceState,
  stopScreenShare: (...a: unknown[]) => stopScreenShare(...a),
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "u1" }) }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => hoisted.native }));

import ScreenShareButton from "../ScreenShareButton";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  hoisted.native = false;
  setVoiceState({ state: "connected", screenSharing: false });
});

describe("ScreenShareButton capability check", () => {
  it("is hidden in the browser without getDisplayMedia", () => {
    vi.stubGlobal("navigator", { mediaDevices: {} });
    render(() => <ScreenShareButton />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("is shown when getDisplayMedia exists", () => {
    vi.stubGlobal("navigator", { mediaDevices: { getDisplayMedia: vi.fn() } });
    render(() => <ScreenShareButton />);
    expect(screen.getByRole("button")).toBeTruthy();
  });

  it("is always shown in the desktop app", () => {
    hoisted.native = true;
    vi.stubGlobal("navigator", { mediaDevices: {} });
    render(() => <ScreenShareButton />);
    expect(screen.getByRole("button")).toBeTruthy();
  });
});

describe("ScreenShareButton while the call reconnects", () => {
  it("a running share can be stopped while the call reconnects", () => {
    vi.stubGlobal("navigator", { mediaDevices: { getDisplayMedia: vi.fn() } });
    stopScreenShare.mockResolvedValue(undefined);
    setVoiceState({ state: "reconnecting", screenSharing: true });
    render(() => <ScreenShareButton />);
    const button = screen.getByRole("button") as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    fireEvent.click(button);
    expect(stopScreenShare).toHaveBeenCalledTimes(1);
  });

  it("a new share cannot be started while the call reconnects", () => {
    vi.stubGlobal("navigator", { mediaDevices: { getDisplayMedia: vi.fn() } });
    setVoiceState({ state: "reconnecting", screenSharing: false });
    render(() => <ScreenShareButton />);
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });
});
