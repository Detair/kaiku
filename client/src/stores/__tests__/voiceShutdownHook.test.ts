/**
 * The start-up path reaches voice only through the hook module, so the
 * voice code is not part of the start-up bundle, and ending voice before a
 * change of the signed-in user still happens in the same turn.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

describe("voice hooks for the start-up path", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("before any voice code is loaded: ending voice does nothing, throws nothing, loads nothing", async () => {
    const loaded = vi.fn();
    vi.doMock("../voiceOwner", () => {
      loaded();
      return {};
    });
    const hook = await import("../voiceShutdownHook");
    expect(() => hook.shutdownVoice()).not.toThrow();
    expect(() => hook.noteVoiceInbound()).not.toThrow();
    expect(hook.voiceIdentity()).toBeNull();
    expect(loaded).not.toHaveBeenCalled();
    vi.doUnmock("../voiceOwner");
  });

  it("once the voice owner is loaded, the hooks reach it synchronously", async () => {
    const hook = await import("../voiceShutdownHook");
    const { voiceOwner } = await import("../voiceOwner");
    const shutdown = vi.spyOn(voiceOwner, "shutdown");
    const inbound = vi.spyOn(voiceOwner, "noteInbound");
    hook.shutdownVoice();
    hook.noteVoiceInbound();
    expect(shutdown).toHaveBeenCalledTimes(1);
    expect(inbound).toHaveBeenCalledTimes(1);
    hook.setVoiceIdentity("u1");
    const wish = voiceOwner.wish("channel", "A", {});
    void voiceOwner.request(wish);
    hook.setVoiceIdentity("u2");
    voiceOwner.socketReady(); // a wish of another user is ended, not served
    expect(voiceOwner.wants(wish)).toBe(false);
  });

  it("the auth store and the socket store do not import voice code", () => {
    for (const file of ["auth.ts", "websocket/index.ts"]) {
      const source = readFileSync(resolve(__dirname, "..", file), "utf8");
      const imports = source
        .split("\n")
        .filter((line) => /^import .* from /.test(line) && !line.startsWith("import type"));
      const voice = imports.filter((line) => /["'](\.\.?\/)+(voice|voiceOwner|call)["']/.test(line));
      // The socket store routes call events (as before); neither store
      // pulls in the owner or the voice store.
      expect(voice.filter((line) => !/\/call["']/.test(line)), file).toEqual([]);
    }
    const hookSource = readFileSync(resolve(__dirname, "..", "voiceShutdownHook.ts"), "utf8");
    expect(hookSource).not.toMatch(/^import /m);
  });
});
