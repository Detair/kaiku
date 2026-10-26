import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";

const participants = vi.hoisted(() => ({ list: [{ user_id: "u1" }] as unknown[] }));

vi.mock("@/stores/voice", () => ({
  voiceState: { state: "disconnected", channelId: null, screenShares: [] },
  getParticipants: () => participants.list,
  joinVoice: vi.fn(),
}));
vi.mock("../VoiceControls", () => ({ default: () => null }));
vi.mock("../VoiceTileGrid", () => ({ default: () => null }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import VoiceChannelView from "../VoiceChannelView";

afterEach(cleanup);

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
});
