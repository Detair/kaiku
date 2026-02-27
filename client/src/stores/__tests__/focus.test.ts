import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/stores/sound", () => ({
  isDndActive: vi.fn(() => false),
}));

vi.mock("@/stores/preferences", () => {
  let _prefs = {
    focus: {
      modes: [
        {
          id: "gaming",
          name: "Gaming",
          icon: "gamepad-2",
          builtin: true,
          triggerCategories: ["game"] as string[],
          autoActivateEnabled: true,
          suppressionLevel: "all" as const,
          vipUserIds: ["vip-user-1"],
          vipChannelIds: ["vip-channel-1"],
          emergencyKeywords: ["urgent", "emergency"],
        },
        {
          id: "deep-work",
          name: "Deep Work",
          icon: "brain",
          builtin: true,
          triggerCategories: ["coding"] as string[],
          autoActivateEnabled: true,
          suppressionLevel: "except_mentions" as const,
          vipUserIds: [],
          vipChannelIds: [],
          emergencyKeywords: [],
        },
        {
          id: "streaming",
          name: "Streaming",
          icon: "radio",
          builtin: true,
          triggerCategories: null,
          autoActivateEnabled: false,
          suppressionLevel: "all" as const,
          vipUserIds: [],
          vipChannelIds: [],
          emergencyKeywords: [],
        },
        {
          id: "dm-friendly",
          name: "DM Friendly",
          icon: "message-circle",
          builtin: false,
          triggerCategories: null,
          autoActivateEnabled: false,
          suppressionLevel: "except_dms" as const,
          vipUserIds: [],
          vipChannelIds: [],
          emergencyKeywords: [],
        },
      ],
      autoActivateGlobal: false,
    },
  };

  return {
    preferences: vi.fn(() => _prefs),
    updatePreference: vi.fn(),
    // Helper for tests to override preferences
    __setPrefs: (p: typeof _prefs) => {
      _prefs = p;
    },
  };
});

vi.mock("@/stores/auth", () => ({
  currentUser: vi.fn(() => ({
    id: "me",
    username: "me",
    display_name: "Me",
    avatar_url: null,
    status: "online",
    email: null,
    mfa_enabled: false,
    created_at: "2025-01-01T00:00:00Z",
  })),
}));

import { isDndActive } from "@/stores/sound";
import { preferences } from "@/stores/preferences";
import {
  evaluateFocusPolicy,
  activateFocusMode,
  deactivateFocusMode,
  handleActivityChange,
  focusState,
  getActiveFocusMode,
} from "../focus";
import type { SoundEvent } from "@/lib/sound/types";

function makeEvent(overrides: Partial<SoundEvent> = {}): SoundEvent {
  return {
    type: "message_channel",
    channelId: "ch-1",
    isDm: false,
    authorId: "other-user",
    ...overrides,
  };
}

describe("focus store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(isDndActive).mockReturnValue(false);
    deactivateFocusMode();

    // Reset to default prefs with autoActivateGlobal off
    const prefs = vi.mocked(preferences)();
    prefs.focus.autoActivateGlobal = false;
  });

  describe("evaluateFocusPolicy", () => {
    it("suppresses when DND is active (absolute priority)", () => {
      vi.mocked(isDndActive).mockReturnValue(true);
      activateFocusMode("gaming");

      // Even with VIP user, DND suppresses
      const result = evaluateFocusPolicy(makeEvent({ authorId: "vip-user-1" }));
      expect(result).toBe("suppress");
    });

    it("allows when no focus mode is active", () => {
      const result = evaluateFocusPolicy(makeEvent());
      expect(result).toBe("allow");
    });

    it("allows VIP user messages through suppression", () => {
      activateFocusMode("gaming");

      const result = evaluateFocusPolicy(makeEvent({ authorId: "vip-user-1" }));
      expect(result).toBe("allow");
    });

    it("allows VIP channel messages through suppression", () => {
      activateFocusMode("gaming");

      const result = evaluateFocusPolicy(
        makeEvent({ channelId: "vip-channel-1" }),
      );
      expect(result).toBe("allow");
    });

    it("allows messages with emergency keywords", () => {
      activateFocusMode("gaming");

      const result = evaluateFocusPolicy(
        makeEvent({ content: "This is URGENT please respond" }),
      );
      expect(result).toBe("allow");
    });

    it("keyword matching is case-insensitive", () => {
      activateFocusMode("gaming");

      const result = evaluateFocusPolicy(
        makeEvent({ content: "EMERGENCY situation" }),
      );
      expect(result).toBe("allow");
    });

    it("suppresses non-VIP, non-keyword messages in 'all' mode", () => {
      activateFocusMode("gaming");

      const result = evaluateFocusPolicy(
        makeEvent({ authorId: "random-user", content: "hey what's up" }),
      );
      expect(result).toBe("suppress");
    });

    it("allows mentions through in 'except_mentions' mode", () => {
      activateFocusMode("deep-work");

      const result = evaluateFocusPolicy(makeEvent({ mentionType: "direct" }));
      expect(result).toBe("allow");
    });

    it("suppresses non-mentions in 'except_mentions' mode", () => {
      activateFocusMode("deep-work");

      const result = evaluateFocusPolicy(makeEvent());
      expect(result).toBe("suppress");
    });

    it("allows DMs through in 'except_dms' mode", () => {
      activateFocusMode("dm-friendly");

      const result = evaluateFocusPolicy(makeEvent({ isDm: true }));
      expect(result).toBe("allow");
    });

    it("suppresses non-DMs in 'except_dms' mode", () => {
      activateFocusMode("dm-friendly");

      const result = evaluateFocusPolicy(makeEvent({ isDm: false }));
      expect(result).toBe("suppress");
    });

    it("does not match keywords when content is undefined", () => {
      activateFocusMode("gaming");

      const result = evaluateFocusPolicy(makeEvent({ content: undefined }));
      expect(result).toBe("suppress");
    });
  });

  describe("activateFocusMode / deactivateFocusMode", () => {
    it("activates a mode and sets state", () => {
      activateFocusMode("gaming");

      expect(focusState().activeModeId).toBe("gaming");
      expect(focusState().autoActivated).toBe(false);
      expect(focusState().activatedAt).toBeTruthy();
    });

    it("deactivates a mode and clears state", () => {
      activateFocusMode("gaming");
      deactivateFocusMode();

      expect(focusState().activeModeId).toBeNull();
      expect(focusState().autoActivated).toBe(false);
      expect(focusState().activatedAt).toBeNull();
    });

    it("does nothing for unknown mode ID", () => {
      activateFocusMode("nonexistent");

      expect(focusState().activeModeId).toBeNull();
    });

    it("getActiveFocusMode returns the mode object", () => {
      activateFocusMode("gaming");

      const mode = getActiveFocusMode();
      expect(mode?.name).toBe("Gaming");
    });

    it("getActiveFocusMode returns null when no mode active", () => {
      expect(getActiveFocusMode()).toBeNull();
    });
  });

  describe("handleActivityChange", () => {
    it("does nothing when autoActivateGlobal is off", () => {
      handleActivityChange("game");

      expect(focusState().activeModeId).toBeNull();
    });

    it("auto-activates matching mode when global toggle is on", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      handleActivityChange("game");

      expect(focusState().activeModeId).toBe("gaming");
      expect(focusState().autoActivated).toBe(true);
      expect(focusState().triggeringCategory).toBe("game");
    });

    it("auto-deactivates when activity clears", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      handleActivityChange("game");
      expect(focusState().activeModeId).toBe("gaming");

      handleActivityChange(null);
      expect(focusState().activeModeId).toBeNull();
    });

    it("does not override a manually activated mode", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      // Manually activate streaming
      activateFocusMode("streaming");
      expect(focusState().autoActivated).toBe(false);

      // Activity change should not override
      handleActivityChange("game");
      expect(focusState().activeModeId).toBe("streaming");
    });

    it("does not deactivate a manually activated mode when activity clears", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      activateFocusMode("gaming");

      handleActivityChange(null);
      expect(focusState().activeModeId).toBe("gaming");
    });

    it("switches auto-activated mode when category changes", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      handleActivityChange("game");
      expect(focusState().activeModeId).toBe("gaming");

      handleActivityChange("coding");
      expect(focusState().activeModeId).toBe("deep-work");
    });

    it("deactivates auto-activated mode when no mode matches new category", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      handleActivityChange("game");
      expect(focusState().activeModeId).toBe("gaming");

      // "listening" has no matching mode in our test data
      handleActivityChange("listening");
      expect(focusState().activeModeId).toBeNull();
    });

    it("does not activate mode with autoActivateEnabled=false", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      // Streaming has triggerCategories=null and autoActivateEnabled=false
      // No mode has "watching" trigger
      handleActivityChange("watching");
      expect(focusState().activeModeId).toBeNull();
    });

    it("skips re-activation if same mode is already active", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      handleActivityChange("game");
      const firstActivatedAt = focusState().activatedAt;

      // Same category again — should not change activatedAt
      handleActivityChange("game");
      expect(focusState().activatedAt).toBe(firstActivatedAt);
    });

    it("deactivates auto-activated mode when global toggle is off and activity clears", () => {
      const prefs = vi.mocked(preferences)();
      prefs.focus.autoActivateGlobal = true;

      // Auto-activate gaming mode
      handleActivityChange("game");
      expect(focusState().activeModeId).toBe("gaming");
      expect(focusState().autoActivated).toBe(true);

      // User turns off global toggle while mode is active
      prefs.focus.autoActivateGlobal = false;

      // App exits — activity clears, should still deactivate
      handleActivityChange(null);
      expect(focusState().activeModeId).toBeNull();
    });
  });
});
