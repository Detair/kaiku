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
