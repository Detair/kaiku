import { describe, expect, it } from "vitest";
import { ChunkLoadError, isChunkLoadError, loadOrTag } from "../chunkLoadError";

describe("loadOrTag (the loader behind lazyWindow)", () => {
  it("passes a loaded module through unchanged", async () => {
    const mod = { default: () => null };
    await expect(loadOrTag(() => Promise.resolve(mod))()).resolves.toBe(mod);
  });

  it.each([
    // WebKit, when the server answers a missing chunk with an HTML page
    ["'text/html' is not a valid JavaScript MIME type."],
    // A chunk that downloaded but does not parse
    ["Unexpected token '<'"],
    ["anything at all"],
  ])("tags every failed import as a load failure, whatever its wording: %s", async (message) => {
    const cause = new TypeError(message);
    const err = await loadOrTag(() => Promise.reject(cause))().catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ChunkLoadError);
    expect((err as Error).cause).toBe(cause);
    expect(isChunkLoadError(err)).toBe(true);
  });
});

describe("isChunkLoadError", () => {
  it.each([
    // Chromium (Chrome, Edge, WebView2)
    ["Failed to fetch dynamically imported module: https://app.example/assets/PinDrawer-abc.js"],
    // Firefox
    ["error loading dynamically imported module: https://app.example/assets/PinDrawer-abc.js"],
    // Safari, WebKitGTK
    ["Importing a module script failed."],
    // Vite's preload helper, when a chunk's stylesheet cannot load
    ["Unable to preload CSS for /assets/PinDrawer-abc.css"],
  ])("recognises a failed download that did not come through lazyWindow: %s", (message) => {
    expect(isChunkLoadError(new TypeError(message))).toBe(true);
  });

  it("recognises the error by name when the message is unhelpful", () => {
    const err = new Error("Loading chunk 12 failed.");
    err.name = "ChunkLoadError";
    expect(isChunkLoadError(err)).toBe(true);
  });

  it.each([
    ["a render error", new TypeError("Cannot read properties of undefined (reading 'id')")],
    ["a failed request inside the window", new TypeError("Failed to fetch")],
    ["a thrown string", "something broke"],
    ["nothing", undefined],
    ["null", null],
    ["a plain object", { message: 42 }],
  ])("treats %s as a problem in the window itself", (_label, err) => {
    expect(isChunkLoadError(err)).toBe(false);
  });

  it("reads a message from a non-Error rejection value", () => {
    expect(
      isChunkLoadError({ message: "Failed to fetch dynamically imported module: x.js" }),
    ).toBe(true);
  });
});
