import { describe, it, expect, beforeEach } from "vitest";
import { produce } from "solid-js/store";
import { voiceState, setVoiceState } from "@/stores/voice";
import {
  handleScreenShareStarted,
  handleScreenShareStopped,
} from "@/stores/websocket";

/**
 * Tests for WebSocket screen share event handlers.
 *
 * Calls the actual exported handlers from websocket.ts to verify
 * they correctly mutate voiceState.
 */

function resetVoiceState() {
  setVoiceState({
    channelId: "test-channel-1",
    screenShares: [],
    screenSharing: false,
    screenShareInfo: null,
    participants: {},
  });
}

describe("WebSocket screen share event handlers", () => {
  beforeEach(() => {
    resetVoiceState();
  });

  describe("handleScreenShareStarted", () => {
    it("should add share to voiceState.screenShares", async () => {
      await handleScreenShareStarted({
        channel_id: "test-channel-1",
        user_id: "user-1",
        username: "alice",
        source_label: "Display 1",
        has_audio: true,
        quality: "high",
      });

      expect(voiceState.screenShares.length).toBe(1);
      expect(voiceState.screenShares[0].user_id).toBe("user-1");
      expect(voiceState.screenShares[0].username).toBe("alice");
      expect(voiceState.screenShares[0].source_label).toBe("Display 1");
      expect(voiceState.screenShares[0].has_audio).toBe(true);
      expect(voiceState.screenShares[0].quality).toBe("high");
    });

    it("should set participant.screen_sharing = true", async () => {
      setVoiceState(
        produce((state) => {
          state.participants["user-1"] = {
            user_id: "user-1",
            username: "alice",
            display_name: "Alice",
            muted: false,
            screen_sharing: false,
          } as any;
        }),
      );

      await handleScreenShareStarted({
        channel_id: "test-channel-1",
        user_id: "user-1",
        username: "alice",
        source_label: "Display 1",
        has_audio: false,
        quality: "medium",
      });

      expect(voiceState.participants["user-1"].screen_sharing).toBe(true);
    });

    it("should not add share if channel_id does not match", async () => {
      await handleScreenShareStarted({
        channel_id: "other-channel",
        user_id: "user-1",
        username: "alice",
        source_label: "Display 1",
        has_audio: false,
        quality: "high",
      });

      expect(voiceState.screenShares.length).toBe(0);
    });
  });

  describe("handleScreenShareStopped", () => {
    it("should remove share from voiceState.screenShares", async () => {
      // Pre-populate a screen share
      setVoiceState(
        produce((state) => {
          state.screenShares.push({
            user_id: "user-1",
            username: "alice",
            source_label: "Display 1",
            has_audio: true,
            quality: "high" as any,
            started_at: new Date().toISOString(),
          });
        }),
      );

      expect(voiceState.screenShares.length).toBe(1);

      await handleScreenShareStopped({
        channel_id: "test-channel-1",
        user_id: "user-1",
        reason: "user_stopped",
      });

      expect(voiceState.screenShares.length).toBe(0);
    });

    it("should set participant.screen_sharing = false", async () => {
      setVoiceState(
        produce((state) => {
          state.participants["user-1"] = {
            user_id: "user-1",
            username: "alice",
            display_name: "Alice",
            muted: false,
            screen_sharing: true,
          } as any;
          state.screenShares.push({
            user_id: "user-1",
            username: "alice",
            source_label: "Display 1",
            has_audio: false,
            quality: "high" as any,
            started_at: new Date().toISOString(),
          });
        }),
      );

      await handleScreenShareStopped({
        channel_id: "test-channel-1",
        user_id: "user-1",
        reason: "user_stopped",
      });

      expect(voiceState.participants["user-1"].screen_sharing).toBe(false);
    });

    it("should not remove share if channel_id does not match", async () => {
      setVoiceState(
        produce((state) => {
          state.screenShares.push({
            user_id: "user-1",
            username: "alice",
            source_label: "Display 1",
            has_audio: false,
            quality: "high" as any,
            started_at: new Date().toISOString(),
          });
        }),
      );

      await handleScreenShareStopped({
        channel_id: "other-channel",
        user_id: "user-1",
        reason: "user_stopped",
      });

      expect(voiceState.screenShares.length).toBe(1);
    });
  });
});
