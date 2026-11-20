/**
 * A tile shows the video it has: the track is attached whenever both the
 * <video> and the track exist, in whichever order they appear, and a new
 * track replaces the old one.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@solidjs/testing-library";

const h = await vi.hoisted(async () => {
  const { createSignal } = await import("solid-js");
  const [webcam, setWebcam] = createSignal<MediaStreamTrack | undefined>(undefined);
  return { webcam, setWebcam };
});

vi.mock("@/stores/webcamViewer", () => ({ getTrack: () => h.webcam() }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => false }));
vi.mock("../TauriVideoFrame", () => ({ default: () => null }));

import VoiceTile from "../VoiceTile";
import { addAvailableTrack, removeAvailableTrack } from "@/stores/screenShareViewer";

class FakeMediaStream {
  constructor(readonly tracks: MediaStreamTrack[]) {}
  getTracks() {
    return this.tracks;
  }
}

const track = (id: string) => ({ id, kind: "video", readyState: "live" }) as unknown as MediaStreamTrack;
const attached = (el: HTMLVideoElement | null) =>
  (el?.srcObject as unknown as FakeMediaStream | null)?.tracks[0]?.id;

beforeEach(() => vi.stubGlobal("MediaStream", FakeMediaStream));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  h.setWebcam(undefined);
  removeAvailableTrack("s1");
});

const participant = {
  type: "participant" as const,
  userId: "u2",
  displayName: "Bea",
  username: "bea",
  muted: false,
  deafened: false,
  speaking: false,
};

describe("participant tile", () => {
  it("a webcam that arrives after the tile is shown is attached", () => {
    const { container } = render(() => <VoiceTile tile={participant} onClick={() => {}} />);
    expect(container.querySelector("video")).toBeNull();
    const cam = track("cam-1");
    h.setWebcam(cam);
    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    expect(attached(video)).toBe("cam-1");
  });

  it("a webcam present from the start is attached, a new one replaces it", () => {
    const first = track("cam-1");
    h.setWebcam(first);
    const { container } = render(() => <VoiceTile tile={participant} onClick={() => {}} />);
    expect(attached(container.querySelector("video"))).toBe(first.id);
    const second = track("cam-2");
    h.setWebcam(second);
    expect(attached(container.querySelector("video"))).toBe(second.id);
  });
});

describe("screen share tile", () => {
  const share = { type: "screen_share" as const, streamId: "s1", userId: "u2", username: "bea" };

  it("a new track for the same share replaces the shown one", () => {
    const first = track("v-1");
    addAvailableTrack("s1", first, "u2", "bea", "Screen");
    const { container } = render(() => <VoiceTile tile={share} onClick={() => {}} />);
    expect(attached(container.querySelector("video"))).toBe(first.id);
    const second = track("v-2");
    addAvailableTrack("s1", second, "u2", "bea", "Screen");
    expect(attached(container.querySelector("video"))).toBe(second.id);
  });
});
