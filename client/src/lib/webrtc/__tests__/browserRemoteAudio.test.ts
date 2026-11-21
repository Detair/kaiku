/**
 * Received audio is played from a stream of its own that holds exactly the
 * received track. The browser hands out the same stream object for every
 * track with the same stream id, so after someone leaves and joins again
 * that object can hold their old track next to the new one.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowserVoiceAdapter } from "../browser";
import type { RemoteTrack } from "../types";

type TrackEvent = { track: FakeTrack; streams: FakeMediaStream[] };

class FakeSubscriberPc {
  ontrack: ((event: TrackEvent) => void) | null = null;
  onicecandidate: unknown = null;
  onconnectionstatechange: unknown = null;
}

let nextStream = 0;
class FakeMediaStream {
  readonly id: string;
  private readonly tracks: FakeTrack[];
  constructor(tracks: FakeTrack[] = [], id?: string) {
    nextStream += 1;
    this.id = id ?? `own-${nextStream}`;
    this.tracks = [...tracks];
  }
  addTrack(track: FakeTrack) {
    this.tracks.push(track);
  }
  getTracks() {
    return this.tracks;
  }
  getAudioTracks() {
    return this.tracks.filter((t) => t.kind === "audio");
  }
}

type FakeTrack = {
  id: string;
  kind: string;
  enabled: boolean;
  onended: null | (() => void);
};

function audioTrack(id: string): FakeTrack {
  return { id, kind: "audio", enabled: true, onended: null };
}

type AdapterPrivate = {
  subscriberState: {
    pc: FakeSubscriberPc;
    remoteDescriptionSet: boolean;
    pendingCandidates: unknown[];
  };
  setupSubscriberPC: (channelId: string) => void;
};

describe("BrowserVoiceAdapter: received audio", () => {
  beforeEach(() => {
    vi.stubGlobal("MediaStream", FakeMediaStream);
    vi.spyOn(console, "log").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  function subscribed() {
    const adapter = new BrowserVoiceAdapter();
    const played: RemoteTrack[] = [];
    const removed = vi.fn();
    adapter.setEventHandlers({
      onRemoteTrack: (t) => played.push(t),
      onRemoteTrackRemoved: removed,
    });
    const pc = new FakeSubscriberPc();
    const priv = adapter as unknown as AdapterPrivate;
    priv.subscriberState = { pc, remoteDescriptionSet: false, pendingCandidates: [] };
    priv.setupSubscriberPC("ch-a");
    return { adapter, played, removed, pc };
  }

  it("plays a stream of its own holding exactly the received track", () => {
    const { played, pc } = subscribed();
    const t1 = audioTrack("u2:microphone:1");
    const eventStream = new FakeMediaStream([t1], "u2:microphone");
    pc.ontrack?.({ track: t1, streams: [eventStream] });

    expect(played).toHaveLength(1);
    expect(played[0].userId).toBe("u2");
    expect(played[0].stream).not.toBe(eventStream);
    expect(played[0].stream?.getTracks()).toEqual([t1]);
  });

  it("a second track of the same person replaces the first", () => {
    const { played, pc } = subscribed();
    // The browser gives both tracks the same stream object.
    const shared = new FakeMediaStream([], "u2:microphone");
    const t1 = audioTrack("u2:microphone:1");
    shared.addTrack(t1);
    pc.ontrack?.({ track: t1, streams: [shared] });
    const t2 = audioTrack("u2:microphone:2");
    shared.addTrack(t2);
    pc.ontrack?.({ track: t2, streams: [shared] });

    expect(played).toHaveLength(2);
    expect(played[1].stream?.getTracks()).toEqual([t2]);
  });

  it("the older track ending does not remove the newer one", async () => {
    const { adapter, played, removed, pc } = subscribed();
    const shared = new FakeMediaStream([], "u2:microphone");
    const t1 = audioTrack("u2:microphone:1");
    shared.addTrack(t1);
    pc.ontrack?.({ track: t1, streams: [shared] });
    const t2 = audioTrack("u2:microphone:2");
    shared.addTrack(t2);
    pc.ontrack?.({ track: t2, streams: [shared] });

    t1.onended?.();
    expect(removed).not.toHaveBeenCalled();

    // The newer track is still the one the adapter controls.
    await adapter.setDeafen(true);
    expect(t2.enabled).toBe(false);
    expect(played[1].stream?.getTracks()).toEqual([t2]);

    t2.onended?.();
    expect(removed).toHaveBeenCalledWith("u2");
  });
});
