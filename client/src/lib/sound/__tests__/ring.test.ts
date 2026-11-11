/**
 * The ring's AudioContext is created/resumed on the first user gesture, so
 * a call that arrives later is audible (browser autoplay policy).
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/stores/sound", () => ({
  getSoundVolume: () => 50,
  getSoundEnabled: () => true,
  isDndActive: () => false,
}));

const contexts: FakeAudioContext[] = [];

class FakeAudioContext {
  state: "suspended" | "running" = "suspended";
  resume = vi.fn(async () => {
    this.state = "running";
  });
  decodeAudioData = vi.fn(async () => ({}) as AudioBuffer);
  constructor() {
    contexts.push(this);
  }
}

describe("ring audio unlock", () => {
  beforeEach(() => {
    vi.resetModules();
    contexts.length = 0;
    vi.stubGlobal("AudioContext", FakeAudioContext);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(8) })),
    );
  });

  it("creates no context before a gesture and resumes it on the first one", async () => {
    await import("../ring");
    expect(contexts).toHaveLength(0);

    document.dispatchEvent(new Event("pointerdown"));
    await vi.waitFor(() => expect(contexts).toHaveLength(1));
    await vi.waitFor(() => expect(contexts[0].resume).toHaveBeenCalledTimes(1));
    expect(contexts[0].state).toBe("running");

    // Later gestures do not prepare it again.
    document.dispatchEvent(new Event("keydown"));
    await Promise.resolve();
    expect(contexts).toHaveLength(1);
    expect(contexts[0].resume).toHaveBeenCalledTimes(1);
  });

  it("primeRingAudio resumes a suspended context and loads the sound", async () => {
    const ring = await import("../ring");
    await ring.primeRingAudio();
    expect(contexts[0].resume).toHaveBeenCalled();
    expect(contexts[0].decodeAudioData).toHaveBeenCalled();
  });
});
