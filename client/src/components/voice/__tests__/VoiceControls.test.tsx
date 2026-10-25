import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";

vi.mock("@/stores/voice", () => ({
  voiceState: {
    state: "connected",
    muted: false,
    deafened: false,
    screenSharing: false,
    screenShares: [],
    webcamActive: false,
  },
  toggleMute: vi.fn(),
  toggleDeafen: vi.fn(),
  isPttActive: () => false,
  startWebcam: vi.fn(),
  stopWebcam: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "u1" }) }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => false }));
vi.mock("../MicrophoneTest", () => ({ default: () => null }));
vi.mock("../ScreenShareQualityPicker", () => ({ default: () => null }));
vi.mock("../ScreenShareSourcePicker", () => ({ default: () => null }));

import VoiceControls from "../VoiceControls";

describe("VoiceControls", () => {
  it("every voice control button has an accessible name", () => {
    render(() => <VoiceControls />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThanOrEqual(5);
    for (const b of buttons) {
      expect(b.getAttribute("aria-label")?.trim() || b.textContent?.trim()).toBeTruthy();
      expect(b.getAttribute("type")).toBe("button");
    }
  });
});
