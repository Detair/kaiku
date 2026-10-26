import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const participants = vi.hoisted(() => ({ list: [{ user_id: "u1" }] as unknown[] }));
const mocks = vi.hoisted(() => ({
  joinVoice: vi.fn(),
  showToast: vi.fn(),
  requestSettings: vi.fn(),
  voiceState: { state: "disconnected", channelId: null, screenShares: [], error: null as unknown },
}));

vi.mock("@/stores/voice", () => ({
  voiceState: mocks.voiceState,
  getParticipants: () => participants.list,
  joinVoice: mocks.joinVoice,
}));
vi.mock("../VoiceControls", () => ({ default: () => null }));
vi.mock("../VoiceTileGrid", () => ({ default: () => null }));
vi.mock("@/components/ui/Toast", () => ({ showToast: mocks.showToast }));
vi.mock("@/stores/settingsNav", () => ({ requestSettings: mocks.requestSettings }));

import VoiceChannelView from "../VoiceChannelView";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.voiceState.error = null;
});

describe("VoiceChannelView", () => {
  it("participant count pluralises in VoiceChannelView", () => {
    participants.list = [{ user_id: "u1" }];
    render(() => <VoiceChannelView channelId="c1" channelName="Lounge" />);
    expect(screen.getByText("1 person connected")).toBeTruthy();
    cleanup();
    participants.list = [{ user_id: "u1" }, { user_id: "u2" }];
    render(() => <VoiceChannelView channelId="c1" channelName="Lounge" />);
    expect(screen.getByText("2 people connected")).toBeTruthy();
  });

  it("mic failure shows reason and audio settings action", async () => {
    mocks.voiceState.error = { type: "permission_denied", message: "NotAllowedError" };
    mocks.joinVoice.mockRejectedValue(new Error("Microphone access denied."));
    render(() => <VoiceChannelView channelId="c1" channelName="Lounge" />);
    fireEvent.click(screen.getByText("Join Voice"));
    await waitFor(() => expect(mocks.showToast).toHaveBeenCalled());
    const toast = mocks.showToast.mock.calls[0][0];
    expect(toast.message).toBe("Kaiku can't use your microphone.");
    expect(toast.action.label).toBe("Audio settings");
    toast.action.onClick();
    expect(mocks.requestSettings).toHaveBeenCalledWith("audio");
  });
});
