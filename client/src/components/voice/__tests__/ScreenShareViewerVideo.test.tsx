/**
 * The share viewer attaches the track to whichever <video> is showing:
 * switching the view mode mounts a new element and it gets the stream too.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@solidjs/testing-library";

vi.mock("@/stores/voice", () => ({ voiceState: { participants: {}, channelId: "ch" } }));
vi.mock("@/stores/simulcastLayers", () => ({
  getActiveLayer: () => null,
  getLayerPreference: () => "auto",
  setLayerPreference: () => {},
}));
vi.mock("@/lib/tauri", () => ({ wsSend: vi.fn() }));

import ScreenShareViewer from "../ScreenShareViewer";
import {
  addAvailableTrack,
  removeAvailableTrack,
  setLayoutMode,
  setViewMode,
  startViewing,
  stopViewing,
} from "@/stores/screenShareViewer";

class FakeMediaStream {
  constructor(readonly tracks: MediaStreamTrack[]) {}
  getTracks() {
    return this.tracks;
  }
}

beforeEach(() => {
  vi.stubGlobal("MediaStream", FakeMediaStream);
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  stopViewing();
  removeAvailableTrack("s1");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("ScreenShareViewer", () => {
  it("a view mode change keeps the picture", () => {
    const track = { id: "v", kind: "video", readyState: "live" } as unknown as MediaStreamTrack;
    addAvailableTrack("s1", track, "u2", "bea", "Screen");
    setLayoutMode("focus");
    setViewMode("spotlight");
    startViewing("s1");
    render(() => <ScreenShareViewer />);
    const shown = () =>
      (document.querySelector("video")?.srcObject as unknown as FakeMediaStream | null)?.tracks[0]?.id;
    expect(shown()).toBe("v");
    setViewMode("pip");
    expect(shown()).toBe("v");
  });
});

describe("ScreenShareViewer start of playback", () => {
  /** Frames are run by the test; a cancelled one never runs. */
  let frames: Map<number, FrameRequestCallback>;
  const runFrames = () => {
    const due = [...frames.values()];
    frames.clear();
    due.forEach((cb) => cb(0));
  };

  beforeEach(() => {
    frames = new Map();
    let next = 0;
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      next += 1;
      frames.set(next, cb);
      return next;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  });

  function viewing() {
    const track = { id: "v", kind: "video", readyState: "live" } as unknown as MediaStreamTrack;
    addAvailableTrack("s1", track, "u2", "bea", "Screen");
    setLayoutMode("focus");
    setViewMode("spotlight");
    startViewing("s1");
  }

  it("a play() that returns no promise does not throw", () => {
    vi.spyOn(HTMLMediaElement.prototype, "play").mockReturnValue(
      undefined as unknown as Promise<void>,
    );
    viewing();
    render(() => <ScreenShareViewer />);
    expect(frames.size).toBeGreaterThan(0);
    expect(runFrames).not.toThrow();
  });

  it("a frame requested by a viewer that is gone does not run", () => {
    const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
    viewing();
    const { unmount } = render(() => <ScreenShareViewer />);
    expect(frames.size).toBeGreaterThan(0);
    unmount();
    expect(frames.size).toBe(0); // cancelled, not left to run later
    runFrames();
    expect(play).not.toHaveBeenCalled();
  });
});
