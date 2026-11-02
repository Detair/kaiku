import { describe, it, expect, vi, afterEach } from "vitest";

const hoisted = vi.hoisted(() => ({ native: false }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: () => hoisted.native }));

import { canSelectOutputDevice } from "@/lib/browserCapabilities";

describe("canSelectOutputDevice", () => {
  const original = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, "setSinkId");
  afterEach(() => {
    hoisted.native = false;
    if (original) Object.defineProperty(HTMLMediaElement.prototype, "setSinkId", original);
    else delete (HTMLMediaElement.prototype as { setSinkId?: unknown }).setSinkId;
  });

  it("is false in the browser without setSinkId", () => {
    delete (HTMLMediaElement.prototype as { setSinkId?: unknown }).setSinkId;
    expect(canSelectOutputDevice()).toBe(false);
  });

  it("is true when setSinkId exists", () => {
    Object.defineProperty(HTMLMediaElement.prototype, "setSinkId", {
      value: vi.fn(),
      configurable: true,
      writable: true,
    });
    expect(canSelectOutputDevice()).toBe(true);
  });

  it("is true in the desktop app", () => {
    delete (HTMLMediaElement.prototype as { setSinkId?: unknown }).setSinkId;
    hoisted.native = true;
    expect(canSelectOutputDevice()).toBe(true);
  });
});
