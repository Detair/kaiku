import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

const hoisted = vi.hoisted(() => ({ native: false }));
vi.mock("@/stores/voice", () => ({
  voiceState: { state: "connected", screenSharing: false, screenShares: [] },
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "u1" }) }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => hoisted.native }));

import ScreenShareButton from "../ScreenShareButton";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  hoisted.native = false;
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
