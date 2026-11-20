/**
 * The desktop video tile acknowledges every frame it gets (flow control),
 * also frames that arrive before its subscription id is known, names its
 * page instance, and unsubscribes when it goes.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@solidjs/testing-library";

const h = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: ((m: unknown) => void) | null }>,
  invoke: vi.fn(),
  resolveSubscribe: null as ((id: number) => void) | null,
  renderThrows: false,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args: unknown) => h.invoke(cmd, args),
  Channel: class {
    onmessage: ((m: unknown) => void) | null = null;
    constructor() {
      h.channels.push(this);
    }
  },
}));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => true }));
vi.mock("@/lib/voice/nativeVideoRenderer", () => ({
  NativeYuvRenderer: class {
    renderFrame() {
      if (h.renderThrows) throw new Error("texture too large");
    }
    dispose() {}
  },
  parseNativeFrame: () => ({ stream_id: "s1" }),
}));

import TauriVideoFrame from "../TauriVideoFrame";

async function flush() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

afterEach(() => {
  cleanup();
  h.channels.length = 0;
  h.invoke.mockReset();
  h.renderThrows = false;
});

describe("TauriVideoFrame", () => {
  it("acks early and later frames, then unsubscribes on unmount", async () => {
    h.invoke.mockImplementation((cmd: string) =>
      cmd === "subscribe_video_frames"
        ? new Promise((r) => {
            h.resolveSubscribe = r;
          })
        : Promise.resolve(),
    );
    const { unmount } = render(() => <TauriVideoFrame userId="u" streamId="s1" />);
    await flush();
    const sub = h.invoke.mock.calls.find(([c]) => c === "subscribe_video_frames");
    expect(sub?.[1]).toMatchObject({ streamId: "s1", pageId: expect.any(String) });
    // Two frames before the id is known.
    h.channels[0].onmessage?.(new ArrayBuffer(8));
    h.channels[0].onmessage?.(new ArrayBuffer(8));
    h.resolveSubscribe?.(7);
    await flush();
    h.channels[0].onmessage?.(new ArrayBuffer(8));
    const acks = h.invoke.mock.calls.filter(([c]) => c === "ack_video_frame");
    expect(acks).toHaveLength(3);
    expect(acks[0][1]).toEqual({ streamId: "s1", subscriptionId: 7 });
    unmount();
    expect(h.invoke).toHaveBeenCalledWith("unsubscribe_video_frames", {
      streamId: "s1",
      subscriptionId: 7,
    });
  });

  it("a render error shows the tile's error and ends the subscription", async () => {
    h.renderThrows = true;
    h.invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "subscribe_video_frames" ? 9 : undefined),
    );
    const { findByText } = render(() => <TauriVideoFrame userId="u" streamId="s1" />);
    await flush();
    h.channels[0].onmessage?.(new ArrayBuffer(64));
    expect(await findByText(/cannot be shown/)).toBeTruthy();
    expect(h.invoke).toHaveBeenCalledWith("unsubscribe_video_frames", {
      streamId: "s1",
      subscriptionId: 9,
    });
    // Later frames are neither acknowledged nor rendered.
    const acks = () => h.invoke.mock.calls.filter(([c]) => c === "ack_video_frame").length;
    const before = acks();
    h.channels[0].onmessage?.(new ArrayBuffer(64));
    expect(acks()).toBe(before);
  });

  it("a failing acknowledgement is caught", async () => {
    const unhandled = vi.fn();
    process.on("unhandledRejection", unhandled);
    h.invoke.mockImplementation((cmd: string) =>
      cmd === "subscribe_video_frames" ? Promise.resolve(3) : Promise.reject(new Error("view gone")),
    );
    vi.spyOn(console, "debug").mockImplementation(() => {});
    render(() => <TauriVideoFrame userId="u" streamId="s1" />);
    await flush();
    h.channels[0].onmessage?.(new ArrayBuffer(64));
    await new Promise((r) => setTimeout(r, 0));
    expect(unhandled).not.toHaveBeenCalled();
    process.off("unhandledRejection", unhandled);
  });
});
