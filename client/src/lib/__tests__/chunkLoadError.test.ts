import { describe, expect, it } from "vitest";
import { isChunkLoadError } from "../chunkLoadError";

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
  ])("recognises a failed download: %s", (message) => {
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
