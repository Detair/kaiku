import { describe, it, expect, vi, beforeEach } from "vitest";

const getMeSecurity = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), getMeSecurity }));

import { currentPrompt, meSecurity, refreshMeSecurity, resetMeSecurity, snoozePrompt } from "../security";
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
    // The reload asked for during the flight runs once afterwards.
    getMeSecurity.mockResolvedValue(security({ passkeys: 3 }));
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

  it("a hint during a flight reloads once more afterwards (Argon review 6 #3)", async () => {
    const first = deferred();
    getMeSecurity.mockReturnValueOnce(first.promise).mockResolvedValueOnce(security({ passkeys: 5 }));
    const running = refreshMeSecurity();
    void refreshMeSecurity(); // e.g. account_request_updated while loading
    void refreshMeSecurity();
    first.resolve(security({ passkeys: 1 }));
    await running;
    await vi.waitFor(() => expect(meSecurity()?.passkeys).toBe(5));
    expect(getMeSecurity).toHaveBeenCalledTimes(2);
  });

  it("a snoozed prompt stays snoozed only for that account (Argon review 6 #9)", async () => {
    getMeSecurity.mockResolvedValue(security({ prompts: ["set_up_phone"] }));
    await refreshMeSecurity();
    snoozePrompt("set_up_phone", "user-a");
    expect(currentPrompt("user-a")).toBeNull();
    expect(currentPrompt("user-b")).toBe("set_up_phone");
    localStorage.removeItem("kaiku:prompt-snooze:user-a:set_up_phone");
  });
});
