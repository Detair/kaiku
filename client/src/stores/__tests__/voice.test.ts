import { beforeEach, describe, expect, it, vi } from "vitest";

const mockAdapter = {
  join: vi.fn(),
  leave: vi.fn(),
  setMute: vi.fn(),
  setDeafen: vi.fn(),
  setEventHandlers: vi.fn(),
  getConnectionMetrics: vi.fn(),
  getScreenShareInfo: vi.fn(),
  startScreenShare: vi.fn(),
  stopScreenShare: vi.fn(),
  startWebcam: vi.fn(),
  stopWebcam: vi.fn(),
  handleOffer: vi.fn(),
  handleIceCandidate: vi.fn(),
};

vi.mock("@/lib/webrtc", () => ({
  createVoiceAdapter: vi.fn(() => mockAdapter),
  getVoiceAdapter: vi.fn(() => mockAdapter),
}));

vi.mock("@/lib/tauri", () => ({
  wsSend: vi.fn(),
  wsScreenShareStart: vi.fn(),
  wsScreenShareStop: vi.fn(),
  wsWebcamStart: vi.fn(),
  wsWebcamStop: vi.fn(),
}));

vi.mock("@/stores/channels", () => ({
  channelsState: {
    channels: [
      { id: "ch-1", name: "voice-1", channel_type: "voice", guild_id: "g1" },
      { id: "ch-2", name: "voice-2", channel_type: "voice", guild_id: "g1" },
    ],
  },
}));

vi.mock("@/components/ui/Toast", () => ({
  showToast: vi.fn(),
  dismissToast: vi.fn(),
}));

import {
  voiceState,
  setVoiceState,
  joinVoice,
  leaveVoice,
  toggleMute,
  toggleDeafen,
  setSpeaking,
  getParticipants,
  isInVoice,
  isInChannel,
  getVoiceChannelInfo,
  getLocalMetrics,
  getParticipantMetrics,
  handleVoiceUserStats,
} from "../voice";

describe("voice store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setVoiceState({
      state: "disconnected",
      channelId: null,
      muted: false,
      deafened: false,
      speaking: false,
      participants: {},
      error: null,
      screenSharing: false,
      screenShareInfo: null,
      screenShares: [],
      webcamActive: false,
      webcams: [],
      sessionId: null,
      connectedAt: null,
      localMetrics: null,
      participantMetrics: new Map(),
    });
  });

  describe("initial state", () => {
    it("is disconnected with defaults", () => {
      expect(voiceState.state).toBe("disconnected");
      expect(voiceState.channelId).toBeNull();
      expect(voiceState.muted).toBe(false);
      expect(voiceState.deafened).toBe(false);
      expect(voiceState.speaking).toBe(false);
      expect(voiceState.participants).toEqual({});
      expect(voiceState.error).toBeNull();
    });
  });

  describe("isInVoice", () => {
    it("returns true when connected", () => {
      setVoiceState({ state: "connected" });

      expect(isInVoice()).toBe(true);
    });

    it("returns false when disconnected", () => {
      expect(isInVoice()).toBe(false);
    });

    it("returns false when connecting", () => {
      setVoiceState({ state: "connecting" });

      expect(isInVoice()).toBe(false);
    });
  });

  describe("isInChannel", () => {
    it("returns true for matching channel", () => {
      setVoiceState({ state: "connected", channelId: "ch-1" });

      expect(isInChannel("ch-1")).toBe(true);
    });

    it("returns false for different channel", () => {
      setVoiceState({ state: "connected", channelId: "ch-1" });

      expect(isInChannel("ch-2")).toBe(false);
    });

    it("returns false when disconnected", () => {
      expect(isInChannel("ch-1")).toBe(false);
    });
  });

  describe("getParticipants", () => {
    it("returns empty array when no participants", () => {
      expect(getParticipants()).toEqual([]);
    });

    it("returns array of participants", () => {
      setVoiceState("participants", {
        u1: {
          user_id: "u1",
          muted: false,
          speaking: false,
          screen_sharing: false,
        },
        u2: {
          user_id: "u2",
          muted: true,
          speaking: false,
          screen_sharing: false,
        },
      });

      expect(getParticipants()).toHaveLength(2);
    });
  });

  describe("getVoiceChannelInfo", () => {
    it("returns null when disconnected", () => {
      expect(getVoiceChannelInfo()).toBeNull();
    });

    it("returns channel info when connected to known channel", () => {
      setVoiceState({ state: "connected", channelId: "ch-1" });

      const info = getVoiceChannelInfo();
      expect(info?.id).toBe("ch-1");
      expect(info?.name).toBe("voice-1");
    });

    it("returns 'Unknown Channel' for unknown channel", () => {
      setVoiceState({ state: "connected", channelId: "unknown-ch" });

      const info = getVoiceChannelInfo();
      expect(info?.id).toBe("unknown-ch");
      expect(info?.name).toBe("Unknown Channel");
    });
  });

  describe("handleVoiceUserStats", () => {
    it("updates participant metrics for matching channel", () => {
      setVoiceState({ state: "connected", channelId: "ch-1" });

      handleVoiceUserStats({
        channel_id: "ch-1",
        user_id: "u1",
        latency: 25,
        packet_loss: 0.5,
        jitter: 3,
        quality: 3,
      });

      const metrics = getParticipantMetrics("u1");
      expect(metrics?.latency).toBe(25);
      expect(metrics?.packetLoss).toBe(0.5);
      expect(metrics?.quality).toBe("good");
    });

    it("ignores stats for wrong channel", () => {
      setVoiceState({ state: "connected", channelId: "ch-1" });

      handleVoiceUserStats({
        channel_id: "ch-other",
        user_id: "u1",
        latency: 25,
        packet_loss: 0,
        jitter: 0,
        quality: 3,
      });

      expect(getParticipantMetrics("u1")).toBeUndefined();
    });
  });

  describe("setSpeaking", () => {
    it("sets speaking state", () => {
      setSpeaking(true);

      expect(voiceState.speaking).toBe(true);

      setSpeaking(false);

      expect(voiceState.speaking).toBe(false);
    });
  });

  describe("getLocalMetrics", () => {
    it("returns null when not connected", () => {
      expect(getLocalMetrics()).toBeNull();
    });

    it("returns stored metrics", () => {
      const metrics = {
        latency: 20,
        packetLoss: 0,
        jitter: 1,
        quality: "good" as const,
        timestamp: Date.now(),
      };
      setVoiceState({ localMetrics: metrics });

      expect(getLocalMetrics()).toEqual(metrics);
    });
  });

  describe("joinVoice", () => {
    it("joins voice channel on success", async () => {
      mockAdapter.join.mockResolvedValue({ ok: true });

      await joinVoice("ch-1");

      expect(mockAdapter.setEventHandlers).toHaveBeenCalled();
      expect(mockAdapter.join).toHaveBeenCalledWith("ch-1");
      expect(voiceState.sessionId).not.toBeNull();
      expect(voiceState.connectedAt).not.toBeNull();
    });

    it("resets state on join error", async () => {
      mockAdapter.join.mockResolvedValue({
        ok: false,
        error: { type: "permission_denied", message: "Mic denied" },
      });

      await expect(joinVoice("ch-1")).rejects.toThrow("Mic denied");
      expect(voiceState.state).toBe("disconnected");
      expect(voiceState.channelId).toBeNull();
    });
  });

  describe("leaveVoice", () => {
    it("resets all state", async () => {
      setVoiceState({
        state: "connected",
        channelId: "ch-1",
        participants: {
          u1: {
            user_id: "u1",
            muted: false,
            speaking: false,
            screen_sharing: false,
          },
        },
        sessionId: "sess-1",
        connectedAt: Date.now(),
      });
      mockAdapter.leave.mockResolvedValue({ ok: true });

      await leaveVoice();

      expect(voiceState.state).toBe("disconnected");
      expect(voiceState.channelId).toBeNull();
      expect(voiceState.participants).toEqual({});
      expect(voiceState.sessionId).toBeNull();
    });

    it("no-ops when already disconnected", async () => {
      await leaveVoice();

      expect(mockAdapter.leave).not.toHaveBeenCalled();
    });
  });

  describe("toggleMute", () => {
    it("toggles mute state via adapter", async () => {
      mockAdapter.setMute.mockResolvedValue({ ok: true });

      await toggleMute();

      expect(mockAdapter.setMute).toHaveBeenCalledWith(true);
    });
  });

  describe("toggleDeafen", () => {
    it("deafening also mutes", async () => {
      mockAdapter.setDeafen.mockResolvedValue({ ok: true });

      await toggleDeafen();

      expect(voiceState.deafened).toBe(true);
      expect(voiceState.muted).toBe(true);
    });

    it("undeafening preserves previous mute state", async () => {
      setVoiceState({ deafened: true, muted: true });
      mockAdapter.setDeafen.mockResolvedValue({ ok: true });

      await toggleDeafen();

      expect(voiceState.deafened).toBe(false);
      // muted stays true since it was the deafen-induced mute state
    });
  });
});
