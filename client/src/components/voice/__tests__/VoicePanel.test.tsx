/**
 * The voice bar (also on Home): mute/deafen while reconnecting, back to the
 * call's channel across servers, and a share click that works on desktop.
 */
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({
  tauri: false,
  selectGuild: vi.fn().mockResolvedValue(undefined),
  selectChannel: vi.fn(),
  startViewing: vi.fn(),
  requestTileFocus: vi.fn(),
  guildsState: { activeGuildId: null as string | null },
  voiceState: {
    state: "reconnecting",
    channelId: "c1" as string | null,
    channelName: "Lounge",
    guildId: "g1",
    lostChannelId: null,
    muted: false,
    deafened: false,
    speaking: false,
    screenSharing: false,
    screenShares: [
      { stream_id: "s1", user_id: "u2", username: "bob", has_audio: false, quality: "medium" },
    ],
    webcamActive: false,
    participants: {},
    error: null,
    connectedAt: null,
  },
}));

vi.mock("@/stores/voice", () => ({
  voiceState: mocks.voiceState,
  getParticipants: () => [],
  joinVoice: vi.fn(),
  leaveVoice: vi.fn(),
  dismissVoiceRejoin: vi.fn(),
  toggleMute: vi.fn(),
  toggleDeafen: vi.fn(),
  isPttActive: () => false,
  getLocalMetrics: () => null,
  startWebcam: vi.fn(),
  stopWebcam: vi.fn(),
  stopScreenShare: vi.fn(),
}));
vi.mock("@/stores/channels", () => ({
  // On Home the channel list holds DMs: the voice channel is not in it.
  getChannel: () => undefined,
  selectChannel: mocks.selectChannel,
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: mocks.guildsState,
  selectGuild: mocks.selectGuild,
}));
vi.mock("@/stores/screenShareViewer", () => ({
  startViewing: mocks.startViewing,
  requestTileFocus: mocks.requestTileFocus,
}));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => mocks.tauri }));

import VoicePanel from "../VoicePanel";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.tauri = false;
  mocks.voiceState.state = "reconnecting";
});

describe("VoicePanel", () => {
  it("shows mute and deafen while reconnecting", () => {
    render(() => <VoicePanel />);
    expect((screen.getByTestId("voice-mute") as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByTestId("voice-deafen")).toBeTruthy();
  });

  it("keeps the channel name on Home and goes back to its server", async () => {
    render(() => <VoicePanel />);
    expect(screen.getByText("Lounge")).toBeTruthy();
    fireEvent.click(screen.getByTestId("voice-panel-open-channel"));
    await waitFor(() => expect(mocks.selectChannel).toHaveBeenCalledWith("c1"));
    expect(mocks.selectGuild).toHaveBeenCalledWith("g1");
  });

  it("browser: a share opens the viewer", () => {
    render(() => <VoicePanel />);
    fireEvent.click(screen.getByTestId("voice-panel-share"));
    expect(mocks.startViewing).toHaveBeenCalledWith("s1");
  });

  it("desktop: a share opens the channel view with that share focused", async () => {
    mocks.tauri = true;
    render(() => <VoicePanel />);
    fireEvent.click(screen.getByTestId("voice-panel-share"));
    expect(mocks.requestTileFocus).toHaveBeenCalledWith("s1");
    expect(mocks.startViewing).not.toHaveBeenCalled();
    await waitFor(() => expect(mocks.selectChannel).toHaveBeenCalledWith("c1"));
  });
});
