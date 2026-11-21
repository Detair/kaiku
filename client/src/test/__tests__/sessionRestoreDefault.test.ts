import { afterEach, describe, expect, it, vi } from "vitest";

// Nothing in this file sets the flag (and it does not import the setup
// module): the default must come from the suite setup in vitest.config.ts.
const SKIP_SESSION_RESTORE_KEY = "kaiku:skip-session-restore";
const fetchMock = vi.hoisted(() => {
  const f = vi.fn(async (..._args: unknown[]) => new Response(null, { status: 401 }));
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("session restore at import in unit tests", () => {
  afterEach(() => {
    localStorage.clear();
  });

  it("is off by default: importing the module starts no refresh request", async () => {
    vi.resetModules();
    await import("@/lib/tauri/common");
    await flush();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("stays off after localStorage.clear()", async () => {
    localStorage.clear();
    expect(localStorage.getItem(SKIP_SESSION_RESTORE_KEY)).toBe("1");
    vi.resetModules();
    await import("@/lib/tauri/common");
    await flush();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("runs when a test opts in by removing the flag", async () => {
    localStorage.removeItem(SKIP_SESSION_RESTORE_KEY);
    vi.resetModules();
    await import("@/lib/tauri/common");
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    // Let the refused refresh finish, so nothing outlives the test.
    await flush();
    expect(String(fetchMock.mock.calls[0][0])).toMatch(/\/auth\/refresh$/);
  });
});
