/**
 * R2: "reconnecting" is an existing voice session. Participants stay
 * visible (marked in plain words), there is no Join button for the same
 * channel, and mute/deafen stay usable.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  joinVoice: vi.fn(),
  toggleMute: vi.fn(),
  toggleDeafen: vi.fn(),
  voiceState: {
    state: "reconnecting",
    channelId: "c1" as string | null,
    muted: false,
    deafened: false,
    speaking: false,
    screenSharing: false,
    screenShares: [],
    webcamActive: false,
    participants: {
      u2: { user_id: "u2", username: "bob", muted: false, speaking: false },
    } as Record<string, unknown>,
    error: null,
  },
}));

vi.mock("@/stores/voice", () => ({
  voiceState: mocks.voiceState,
  getParticipants: () => Object.values(mocks.voiceState.participants),
  joinVoice: mocks.joinVoice,
  toggleMute: mocks.toggleMute,
  toggleDeafen: mocks.toggleDeafen,
  isPttActive: () => false,
  getLocalMetrics: () => null,
  getParticipantMetrics: () => undefined,
  startWebcam: vi.fn(),
  stopWebcam: vi.fn(),
}));
vi.mock("@/stores/auth", () => ({
  authState: { user: { id: "me", username: "me" } },
  currentUser: () => ({ id: "me" }),
}));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => false }));
vi.mock("../VoiceTileGrid", () => ({ default: () => null }));
vi.mock("../MicrophoneTest", () => ({ default: () => null }));
vi.mock("../ScreenShareQualityPicker", () => ({ default: () => null }));
vi.mock("../ScreenShareSourcePicker", () => ({ default: () => null }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/stores/settingsNav", () => ({ requestSettings: vi.fn() }));

import VoiceChannelView from "../VoiceChannelView";
import VoiceControls from "../VoiceControls";
import VoiceParticipants from "../VoiceParticipants";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.voiceState.state = "reconnecting";
  mocks.voiceState.channelId = "c1";
});

describe("reconnecting voice session", () => {
  it("channel view: no Join for the same channel, says it is reconnecting", () => {
    render(() => <VoiceChannelView channelId="c1" channelName="Lounge" />);
    expect(screen.queryByText("Join Voice")).toBeNull();
    expect(screen.getByTestId("voice-channel-reconnecting").textContent).toBe(
      "Connection lost. Reconnecting, you are still in this channel.",
    );
    // The controls are there and usable.
    expect((screen.getByTestId("voice-mute") as HTMLButtonElement).disabled).toBe(false);
  });

  it("channel view of another channel still offers Join", () => {
    render(() => <VoiceChannelView channelId="c2" channelName="Other" />);
    fireEvent.click(screen.getByText("Join Voice"));
    expect(mocks.joinVoice).toHaveBeenCalledWith("c2");
  });

  it("mute and deafen stay usable", () => {
    render(() => <VoiceControls />);
    const mute = screen.getByTestId("voice-mute") as HTMLButtonElement;
    const deafen = screen.getByTestId("voice-deafen") as HTMLButtonElement;
    expect(mute.disabled).toBe(false);
    expect(deafen.disabled).toBe(false);
    fireEvent.click(mute);
    fireEvent.click(deafen);
    expect(mocks.toggleMute).toHaveBeenCalled();
    expect(mocks.toggleDeafen).toHaveBeenCalled();
  });

  it("participants stay visible and the own row says reconnecting", () => {
    render(() => <VoiceParticipants channelId="c1" />);
    expect(screen.getByText("bob")).toBeTruthy();
    expect(screen.getByTestId("voice-self-reconnecting").textContent).toBe("reconnecting");
  });

  it("controls are disabled once disconnected", () => {
    mocks.voiceState.state = "disconnected";
    render(() => <VoiceControls />);
    expect((screen.getByTestId("voice-mute") as HTMLButtonElement).disabled).toBe(true);
  });
});
