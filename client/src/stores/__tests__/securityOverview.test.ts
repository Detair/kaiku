import { describe, it, expect, vi, beforeEach } from "vitest";

const getMeSecurity = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), getMeSecurity }));

import { meSecurity, refreshMeSecurity, resetMeSecurity } from "../security";
import { security } from "@/components/settings/security/__tests__/securityTestUtils";
import type { MeSecurity } from "@/lib/types";

function deferred() {
  let resolve!: (s: MeSecurity) => void;
  const promise = new Promise<MeSecurity>((r) => (resolve = r));
  return { promise, resolve };
}

beforeEach(() => {
  getMeSecurity.mockReset();
  resetMeSecurity();
});

describe("security overview per account (Codex review)", () => {
  it("a response for the previous account never shows after a reset", async () => {
    const previous = deferred();
    const next = deferred();
    getMeSecurity.mockReturnValueOnce(previous.promise).mockReturnValueOnce(next.promise);
    const old = refreshMeSecurity();
    // Account switch: the old flight is detached, a new one starts.
    resetMeSecurity();
    const fresh = refreshMeSecurity();
    expect(getMeSecurity).toHaveBeenCalledTimes(2);
    next.resolve(security({ passkeys: 2 }));
    await fresh;
    previous.resolve(
      security({ passkeys: 0, open_request: { id: "r-old" } as MeSecurity["open_request"], prompts: ["open_request"] }),
    );
    await old;
    expect(meSecurity()?.passkeys).toBe(2);
    expect(meSecurity()?.open_request).toBeNull();
  });

  it("the end of an old flight does not detach the newer one", async () => {
    const previous = deferred();
    const next = deferred();
    getMeSecurity.mockReturnValueOnce(previous.promise).mockReturnValueOnce(next.promise);
    const old = refreshMeSecurity();
    resetMeSecurity();
    const fresh = refreshMeSecurity();
    previous.resolve(security());
    await old;
    // Still single flight: the newer request is reused.
    expect(refreshMeSecurity()).toBe(fresh);
    expect(getMeSecurity).toHaveBeenCalledTimes(2);
    next.resolve(security({ passkeys: 3 }));
    await fresh;
    expect(meSecurity()?.passkeys).toBe(3);
  });

  it("a reset during a flight leaves the overview empty", async () => {
    const previous = deferred();
    getMeSecurity.mockReturnValueOnce(previous.promise);
    const old = refreshMeSecurity();
    resetMeSecurity();
    previous.resolve(security());
    await old;
    expect(meSecurity()).toBeNull();
  });
});
