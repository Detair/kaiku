// A failed first adapter load must not stick: the next voice attempt retries.
import { describe, expect, it, vi } from "vitest";

const control = vi.hoisted(() => ({ failNext: true, constructed: 0 }));

vi.mock("@/lib/tauri/common", () => ({ isTauri: () => false }));
vi.mock("../browser", () => ({
  BrowserVoiceAdapter: class {
    constructor() {
      if (control.failNext) {
        control.failNext = false;
        throw new Error("chunk failed to load");
      }
      control.constructed += 1;
    }
    dispose() {}
  },
}));

import { createVoiceAdapter, getVoiceAdapter, resetVoiceAdapter } from "../index";

describe("createVoiceAdapter", () => {
  it("retries after the first load failed", async () => {
    await expect(createVoiceAdapter()).rejects.toThrow("chunk failed to load");
    expect(getVoiceAdapter()).toBeNull();

    const adapter = await createVoiceAdapter();
    expect(adapter).toBeTruthy();
    expect(getVoiceAdapter()).toBe(adapter);
    expect(control.constructed).toBe(1);

    // Later calls reuse the one adapter.
    expect(await createVoiceAdapter()).toBe(adapter);
    expect(control.constructed).toBe(1);
    resetVoiceAdapter();
  });

  it("shares one load between concurrent callers", async () => {
    const [a, b] = await Promise.all([createVoiceAdapter(), createVoiceAdapter()]);
    expect(a).toBe(b);
    resetVoiceAdapter();
  });
});
