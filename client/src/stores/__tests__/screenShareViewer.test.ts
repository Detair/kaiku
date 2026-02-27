import { describe, it, expect, beforeEach, vi } from "vitest";
import { unwrap } from "solid-js/store";
import {
  viewerState,
  addAvailableTrack,
  removeAvailableTrack,
  viewUserShare,
  startViewing,
  stopViewing,
  getAvailableSharers,
} from "../screenShareViewer";

// Mock MediaStreamTrack
function createMockTrack(
  readyState: "live" | "ended" = "live",
): MediaStreamTrack {
  const track = {
    readyState,
    onended: null as (() => void) | null,
    id: Math.random().toString(36).substring(7),
    kind: "video",
    label: "screen",
    enabled: true,
    muted: false,
    contentHint: "",
    isolated: false,
    clone: vi.fn(),
    stop: vi.fn(),
    getCapabilities: vi.fn(),
    getConstraints: vi.fn(),
    getSettings: vi.fn(),
    applyConstraints: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  } as unknown as MediaStreamTrack;
  return track;
}

describe("screenShareViewer", () => {
  beforeEach(() => {
    // Reset state before each test
    stopViewing();
    // Clear available tracks
    for (const userId of getAvailableSharers()) {
      removeAvailableTrack(userId);
    }
  });

  describe("addAvailableTrack", () => {
    it("should add track to available tracks", () => {
      const track = createMockTrack();
      addAvailableTrack("user1", track);

      expect(getAvailableSharers()).toContain("user1");
      expect(viewerState.availableTracks.get("user1")).toBe(track);
    });

    it("should set onended handler for auto-cleanup", () => {
      const track = createMockTrack();
      addAvailableTrack("user1", track);

      expect(track.onended).not.toBeNull();
    });

    it("should remove track when onended fires", () => {
      const track = createMockTrack();
      addAvailableTrack("user1", track);

      // Simulate track ending
      if (track.onended) {
        track.onended(new Event("ended"));
      }

      expect(getAvailableSharers()).not.toContain("user1");
    });
  });

  describe("removeAvailableTrack", () => {
    it("should remove track from available tracks", () => {
      const track = createMockTrack();
      addAvailableTrack("user1", track);
      removeAvailableTrack("user1");

      expect(getAvailableSharers()).not.toContain("user1");
    });

    it("should stop viewing if currently viewing that user", () => {
      const track = createMockTrack();
      startViewing("user1", track);

      expect(viewerState.viewingUserId).toBe("user1");

      removeAvailableTrack("user1");

      expect(viewerState.viewingUserId).toBeNull();
    });
  });

  describe("viewUserShare", () => {
    it("should return false if no track available", () => {
      const result = viewUserShare("nonexistent");
      expect(result).toBe(false);
    });

    it("should return false if track has ended", () => {
      const track = createMockTrack("ended");
      // Directly set in map to bypass onended setup
      const newTracks = new Map(viewerState.availableTracks);
      newTracks.set("user1", track);
      // Use addAvailableTrack but with ended track
      addAvailableTrack("user1", track);

      // Now manually set readyState to ended (simulating track ending after add)
      (track as { readyState: string }).readyState = "ended";

      const result = viewUserShare("user1");
      expect(result).toBe(false);
    });

    it("should switch to viewing the user if track is active", () => {
      const track = createMockTrack("live");
      addAvailableTrack("user1", track);

      const result = viewUserShare("user1");

      expect(result).toBe(true);
      expect(viewerState.viewingUserId).toBe("user1");
      expect(unwrap(viewerState).videoTrack).toBe(track);
    });
  });

  describe("startViewing", () => {
    it("should set viewing user and track", () => {
      const track = createMockTrack();
      startViewing("user1", track);

      expect(viewerState.viewingUserId).toBe("user1");
      expect(unwrap(viewerState).videoTrack).toBe(track);
    });

    it("should add track to available tracks", () => {
      const track = createMockTrack();
      startViewing("user1", track);

      expect(getAvailableSharers()).toContain("user1");
    });

    it("should set onended handler for auto-cleanup", () => {
      const track = createMockTrack();
      startViewing("user1", track);

      expect(track.onended).not.toBeNull();
    });
  });

  describe("stopViewing", () => {
    it("should clear viewing state", () => {
      const track = createMockTrack();
      startViewing("user1", track);
      stopViewing();

      expect(viewerState.viewingUserId).toBeNull();
      expect(viewerState.videoTrack).toBeNull();
    });
  });

  describe("getAvailableSharers", () => {
    it("should return list of user IDs with available tracks", () => {
      addAvailableTrack("user1", createMockTrack());
      addAvailableTrack("user2", createMockTrack());

      const sharers = getAvailableSharers();

      expect(sharers).toContain("user1");
      expect(sharers).toContain("user2");
      expect(sharers.length).toBe(2);
    });
  });
});
