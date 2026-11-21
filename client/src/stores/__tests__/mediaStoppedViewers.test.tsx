/**
 * A stop event of a camera or a screen share removes the picture the
 * viewers hold for it. A browser does not end a received track when the
 * server takes it out of the call, so without this a tile would keep the
 * last frame instead of showing the avatar again.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@solidjs/testing-library";

vi.mock("@/lib/tauri/common", async (orig) => ({
  ...(await orig<typeof import("@/lib/tauri/common")>()),
  isTauri: () => false,
}));
vi.mock("@/components/voice/TauriVideoFrame", () => ({ default: () => null }));

import VoiceTile from "@/components/voice/VoiceTile";
import { setVoiceState } from "@/stores/voice";
import { handleScreenShareStopped, handleWebcamStopped } from "@/stores/websocket/mediaEvents";
import * as webcams from "@/stores/webcamViewer";
import * as shares from "@/stores/screenShareViewer";

class FakeMediaStream {
  constructor(readonly tracks: MediaStreamTrack[]) {}
  getTracks() {
    return this.tracks;
  }
}

const track = (id: string) =>
  ({ id, kind: "video", readyState: "live" }) as unknown as MediaStreamTrack;

const CH = "ch-1";
const camStop = (user_id: string) => ({ channel_id: CH, user_id, reason: "user" }) as never;
const shareStop = (user_id: string, stream_id: string) =>
  ({ channel_id: CH, user_id, stream_id, reason: "user" }) as never;

beforeEach(() => {
  vi.stubGlobal("MediaStream", FakeMediaStream);
  vi.spyOn(console, "log").mockImplementation(() => {});
  setVoiceState({ channelId: CH, screenShares: [], webcams: [], participants: {} });
});
afterEach(() => {
  cleanup();
  webcams.clearAll();
  shares.stopViewing();
  for (const s of shares.getAvailableSharers()) shares.removeAvailableTrack(s.streamId);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
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

describe("a camera stop", () => {
  it("removes the stored picture of that person; the tile shows the avatar", async () => {
    webcams.addAvailableTrack("u2", track("cam-1"));
    webcams.addAvailableTrack("u3", track("cam-3"));
    const { container } = render(() => <VoiceTile tile={participant} onClick={() => {}} />);
    expect(container.querySelector("video")).not.toBeNull();

    await handleWebcamStopped(camStop("u2"));

    expect(webcams.getTrack("u2")).toBeUndefined();
    expect(webcams.getTrack("u3")).toBeDefined(); // only the person it names
    expect(container.querySelector("video")).toBeNull();
    expect(container.textContent).toContain("B");
  });

  it("stop, start, new track: the new picture stays", async () => {
    webcams.addAvailableTrack("u2", track("cam-1"));
    await handleWebcamStopped(camStop("u2"));
    const fresh = track("cam-2");
    webcams.addAvailableTrack("u2", fresh);
    expect(webcams.getTrack("u2")).toBe(fresh);
  });

  it("a stop still being handled when the new track arrives leaves the new picture", async () => {
    webcams.addAvailableTrack("u2", track("cam-1"));
    const stopping = handleWebcamStopped(camStop("u2"));
    const fresh = track("cam-2");
    webcams.addAvailableTrack("u2", fresh);
    await stopping;
    expect(webcams.getTrack("u2")).toBe(fresh);
  });

  it("a stop of another channel leaves the picture", async () => {
    const cam = track("cam-1");
    webcams.addAvailableTrack("u2", cam);
    await handleWebcamStopped({ channel_id: "other", user_id: "u2", reason: "user" } as never);
    expect(webcams.getTrack("u2")).toBe(cam);
  });
});

describe("a screen share stop", () => {
  it("removes the stored track of that share, and a viewer on it closes", async () => {
    shares.addAvailableTrack("s1", track("v-1"), "u2", "bea", "Screen");
    shares.startViewing("s1");
    await handleScreenShareStopped(shareStop("u2", "s1"));
    expect(shares.viewerState.availableTracks.has("s1")).toBe(false);
    expect(shares.viewerState.viewingStreamId).toBeNull();
  });

  it("the watched share stops: the viewer closes, no other share or own preview opens", async () => {
    shares.addAvailableTrack("own", track("own-1"), "me", "me", "Screen");
    shares.addAvailableTrack("a", track("v-a"), "u2", "bea", "Screen");
    shares.addAvailableTrack("b", track("v-b"), "u3", "cem", "Screen");
    shares.startViewing("a");
    await handleScreenShareStopped(shareStop("u2", "a"));
    expect(shares.viewerState.viewingStreamId).toBeNull();
    expect(shares.viewerState.videoTrack).toBeNull();
    expect([...shares.viewerState.availableTracks.keys()].sort()).toEqual(["b", "own"]);
  });

  it("the own preview of a new share stays when the stop of the earlier one arrives", async () => {
    // Own previews are stored per share id, like everybody's shares.
    const fresh = track("own-2");
    shares.addAvailableTrack("own-new", fresh, "me", "me", "Screen");
    await handleScreenShareStopped(shareStop("me", "own-old"));
    expect(shares.viewerState.availableTracks.get("own-new")?.track).toBe(fresh);
  });
});
