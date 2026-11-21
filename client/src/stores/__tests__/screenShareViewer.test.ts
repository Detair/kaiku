import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { unwrap } from "solid-js/store";
import {
  viewerState,
  addAvailableTrack,
  removeAvailableTrack,
  startViewing,
  stopViewing,
  getAvailableSharers,
  setScreenVolume,
  toggleMute,
  setLayoutMode,
  addToGrid,
  removeFromGrid,
  swapPrimary,
  isViewingStream,
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
    for (const sharer of getAvailableSharers()) {
      removeAvailableTrack(sharer.streamId);
    }
    // Reset layout mode
    setLayoutMode("focus");
  });

  describe("addAvailableTrack", () => {
    it("should add track to available tracks by streamId", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");

      const sharers = getAvailableSharers();
      expect(sharers).toHaveLength(1);
      expect(sharers[0].streamId).toBe("stream-1");
      expect(sharers[0].userId).toBe("user1");
      expect(sharers[0].username).toBe("Alice");
      expect(sharers[0].sourceLabel).toBe("Screen 1");

      const info = viewerState.availableTracks.get("stream-1");
      expect(info).toBeDefined();
      expect(info!.track).toBe(track);
      expect(info!.userId).toBe("user1");
    });

    it("should set onended handler for auto-cleanup", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");

      expect(track.onended).not.toBeNull();
    });

    it("should remove track when onended fires", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");

      // Simulate track ending
      if (track.onended) {
        track.onended(new Event("ended"));
      }

      expect(getAvailableSharers()).toHaveLength(0);
    });

    it("should support multiple streams from the same user", () => {
      const track1 = createMockTrack();
      const track2 = createMockTrack();
      addAvailableTrack("stream-1", track1, "user1", "Alice", "Screen 1");
      addAvailableTrack("stream-2", track2, "user1", "Alice", "Screen 2");

      const sharers = getAvailableSharers();
      expect(sharers).toHaveLength(2);
      expect(sharers.map((s) => s.streamId)).toContain("stream-1");
      expect(sharers.map((s) => s.streamId)).toContain("stream-2");
    });
  });

  describe("removeAvailableTrack", () => {
    it("should remove track from available tracks", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      removeAvailableTrack("stream-1");

      expect(getAvailableSharers()).toHaveLength(0);
    });

    it("closes the viewer when the watched stream is removed, also if others remain", () => {
      const track1 = createMockTrack();
      const track2 = createMockTrack();
      addAvailableTrack("stream-1", track1, "user1", "Alice", "Screen 1");
      addAvailableTrack("stream-2", track2, "user2", "Bob", "Screen 1");
      startViewing("stream-1");

      expect(viewerState.viewingStreamId).toBe("stream-1");

      removeAvailableTrack("stream-1");

      // A share the person did not choose is not opened in its place.
      expect(viewerState.viewingStreamId).toBeNull();
      expect(viewerState.videoTrack).toBeNull();
      expect(getAvailableSharers().map((s) => s.streamId)).toEqual(["stream-2"]);
    });

    it("should clear viewing when removing the only stream", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      startViewing("stream-1");

      removeAvailableTrack("stream-1");

      expect(viewerState.viewingStreamId).toBeNull();
      expect(viewerState.videoTrack).toBeNull();
    });

    it("should not change primary if removing a non-primary stream", () => {
      const track1 = createMockTrack();
      const track2 = createMockTrack();
      addAvailableTrack("stream-1", track1, "user1", "Alice", "Screen 1");
      addAvailableTrack("stream-2", track2, "user2", "Bob", "Screen 1");
      startViewing("stream-1");

      removeAvailableTrack("stream-2");

      expect(viewerState.viewingStreamId).toBe("stream-1");
      expect(unwrap(viewerState).videoTrack).toBe(track1);
    });

    it("should remove stream from grid when removed", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      addToGrid("stream-1");

      expect(viewerState.gridStreamIds).toContain("stream-1");

      removeAvailableTrack("stream-1");

      expect(viewerState.gridStreamIds).not.toContain("stream-1");
    });
  });

  describe("startViewing", () => {
    it("should set viewing stream and track", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      startViewing("stream-1");

      expect(viewerState.viewingStreamId).toBe("stream-1");
      expect(unwrap(viewerState).videoTrack).toBe(track);
    });

    it("should not start viewing if stream not registered", () => {
      startViewing("nonexistent");

      expect(viewerState.viewingStreamId).toBeNull();
      expect(viewerState.videoTrack).toBeNull();
    });

    it("should not start viewing if track has ended", () => {
      const track = createMockTrack("live");
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");

      // Manually set readyState to ended (simulating track ending after add)
      (track as { readyState: string }).readyState = "ended";

      startViewing("stream-1");

      expect(viewerState.viewingStreamId).toBeNull();
    });

    describe("while waiting for the track", () => {
      beforeEach(() => {
        vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
      });
      afterEach(() => {
        vi.useRealTimers();
      });

      it("opens once the track arrives", () => {
        startViewing("late");
        addAvailableTrack("late", createMockTrack(), "user1", "Alice", "Screen 1");
        vi.advanceTimersByTime(500);
        expect(viewerState.viewingStreamId).toBe("late");
      });

      it("a viewer closed before the track arrives stays closed", () => {
        startViewing("late");
        stopViewing();
        addAvailableTrack("late", createMockTrack(), "user1", "Alice", "Screen 1");
        vi.advanceTimersByTime(5_000);
        expect(viewerState.viewingStreamId).toBeNull();
        expect(viewerState.videoTrack).toBeNull();
      });

      it("viewing another share drops the wait for the first", () => {
        const b = createMockTrack();
        addAvailableTrack("b", b, "user2", "Bob", "Screen 2");
        startViewing("a"); // no track yet
        startViewing("b");
        expect(viewerState.viewingStreamId).toBe("b");
        addAvailableTrack("a", createMockTrack(), "user1", "Alice", "Screen 1");
        vi.advanceTimersByTime(5_000);
        expect(viewerState.viewingStreamId).toBe("b");
        expect(unwrap(viewerState).videoTrack).toBe(b);
      });
    });
  });

  describe("stopViewing", () => {
    it("should clear viewing state", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      startViewing("stream-1");
      stopViewing();

      expect(viewerState.viewingStreamId).toBeNull();
      expect(viewerState.videoTrack).toBeNull();
    });
  });

  describe("isViewingStream", () => {
    it("should return true for the current stream", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      startViewing("stream-1");

      expect(isViewingStream("stream-1")).toBe(true);
      expect(isViewingStream("stream-2")).toBe(false);
    });
  });

  describe("toggleMute", () => {
    it("should remember volume when muting and restore on unmute", () => {
      setScreenVolume(75);
      toggleMute(); // mute
      expect(viewerState.screenVolume).toBe(0);
      toggleMute(); // unmute — should restore to 75
      expect(viewerState.screenVolume).toBe(75);
    });
  });

  describe("getAvailableSharers", () => {
    it("should return list of sharers with metadata", () => {
      addAvailableTrack("stream-1", createMockTrack(), "user1", "Alice", "Screen 1");
      addAvailableTrack("stream-2", createMockTrack(), "user2", "Bob", "Monitor 2");

      const sharers = getAvailableSharers();

      expect(sharers).toHaveLength(2);
      const stream1 = sharers.find((s) => s.streamId === "stream-1");
      expect(stream1).toBeDefined();
      expect(stream1!.userId).toBe("user1");
      expect(stream1!.username).toBe("Alice");
      expect(stream1!.sourceLabel).toBe("Screen 1");
    });
  });

  describe("layoutMode", () => {
    it("should default to focus mode", () => {
      expect(viewerState.layoutMode).toBe("focus");
    });

    it("should switch to grid mode", () => {
      setLayoutMode("grid");
      expect(viewerState.layoutMode).toBe("grid");
    });

    it("should switch back to focus mode", () => {
      setLayoutMode("grid");
      setLayoutMode("focus");
      expect(viewerState.layoutMode).toBe("focus");
    });
  });

  describe("grid management", () => {
    it("should add a stream to grid", () => {
      addAvailableTrack("stream-1", createMockTrack(), "user1", "Alice", "Screen 1");
      const result = addToGrid("stream-1");

      expect(result).toBe(true);
      expect(viewerState.gridStreamIds).toContain("stream-1");
    });

    it("should not add duplicate stream to grid", () => {
      addAvailableTrack("stream-1", createMockTrack(), "user1", "Alice", "Screen 1");
      addToGrid("stream-1");
      const result = addToGrid("stream-1");

      expect(result).toBe(false);
      expect(viewerState.gridStreamIds).toHaveLength(1);
    });

    it("should enforce max 4 streams in grid", () => {
      for (let i = 1; i <= 5; i++) {
        addAvailableTrack(
          `stream-${i}`,
          createMockTrack(),
          `user${i}`,
          `User ${i}`,
          `Screen ${i}`,
        );
      }

      expect(addToGrid("stream-1")).toBe(true);
      expect(addToGrid("stream-2")).toBe(true);
      expect(addToGrid("stream-3")).toBe(true);
      expect(addToGrid("stream-4")).toBe(true);
      expect(addToGrid("stream-5")).toBe(false);

      expect(viewerState.gridStreamIds).toHaveLength(4);
    });

    it("should not add non-existent stream to grid", () => {
      const result = addToGrid("nonexistent");
      expect(result).toBe(false);
    });

    it("should remove a stream from grid", () => {
      addAvailableTrack("stream-1", createMockTrack(), "user1", "Alice", "Screen 1");
      addToGrid("stream-1");
      removeFromGrid("stream-1");

      expect(viewerState.gridStreamIds).not.toContain("stream-1");
    });
  });

  describe("swapPrimary", () => {
    it("should swap a stream into primary view", () => {
      const track1 = createMockTrack();
      const track2 = createMockTrack();
      addAvailableTrack("stream-1", track1, "user1", "Alice", "Screen 1");
      addAvailableTrack("stream-2", track2, "user2", "Bob", "Screen 1");
      startViewing("stream-1");

      swapPrimary("stream-2");

      expect(viewerState.viewingStreamId).toBe("stream-2");
      expect(unwrap(viewerState).videoTrack).toBe(track2);
    });

    it("should not swap if stream does not exist", () => {
      const track = createMockTrack();
      addAvailableTrack("stream-1", track, "user1", "Alice", "Screen 1");
      startViewing("stream-1");

      swapPrimary("nonexistent");

      // Should remain on stream-1
      expect(viewerState.viewingStreamId).toBe("stream-1");
    });

    it("should not swap if track has ended", () => {
      const track1 = createMockTrack();
      const track2 = createMockTrack();
      addAvailableTrack("stream-1", track1, "user1", "Alice", "Screen 1");
      addAvailableTrack("stream-2", track2, "user2", "Bob", "Screen 1");
      startViewing("stream-1");

      // Mark track2 as ended
      (track2 as { readyState: string }).readyState = "ended";

      swapPrimary("stream-2");

      // Should remain on stream-1 (stream-2 was removed)
      expect(viewerState.viewingStreamId).toBe("stream-1");
    });
  });
});

describe("tile focus request", () => {
  it("waits for its tile, but not for ever", async () => {
    vi.useFakeTimers();
    const m = await import("../screenShareViewer");
    m.requestTileFocus("s1");
    vi.advanceTimersByTime(m.TILE_FOCUS_REQUEST_TTL_MS - 1);
    expect(m.tileFocusRequest()).toBe("s1");
    vi.advanceTimersByTime(1);
    expect(m.tileFocusRequest()).toBeNull();
    vi.useRealTimers();
  });
});
