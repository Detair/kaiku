import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type * as Common from "../tauri/common";
import { createConnectionSupervisor } from "@/stores/websocket/connection";

function respond(status: number, body: unknown = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: vi.fn().mockResolvedValue(body),
  };
}

const fetchMock = vi.fn();
let common: typeof Common;

beforeAll(async () => {
  vi.stubGlobal("fetch", fetchMock);
  // Importing common.ts starts a session-restore refresh; let it settle.
  fetchMock.mockResolvedValue(respond(401));
  common = await import("../tauri/common");
  await common.refreshAccessToken();
});

describe("token refresh failure handling", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    common.browserState.accessToken = "old-token";
    // Within 60 s of expiry: ensureFreshAccessToken must refresh.
    common.browserState.tokenExpiresAt = Date.now() + 30_000;
  });

  afterEach(() => {
    common.clearBrowserTokens();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("a 503 refresh keeps the session", async () => {
    fetchMock.mockResolvedValue(respond(503));

    expect(await common.refreshAccessToken()).toBe(false);
    expect(common.browserState.accessToken).toBe("old-token");
    expect(await common.ensureFreshAccessToken()).toBe(true);
  });

  it.each([401, 403])("a %i refresh ends the session", async (status) => {
    fetchMock.mockResolvedValue(respond(status));

    expect(await common.ensureFreshAccessToken()).toBe(false);
    expect(common.browserState.accessToken).toBeNull();
  });

  it("a 403 ACCOUNT_BANNED refresh ends the session as banned", async () => {
    fetchMock.mockResolvedValue(respond(403, { error: "ACCOUNT_BANNED" }));
    const ended = vi.fn();
    const listener = (e: Event) => ended((e as CustomEvent).detail);
    window.addEventListener("kaiku:session-ended", listener);

    expect(await common.refreshAccessToken()).toBe(false);
    window.removeEventListener("kaiku:session-ended", listener);

    expect(common.browserState.accessToken).toBeNull();
    expect(ended).toHaveBeenCalledWith({ reason: "banned" });
  });

  it("a 503 refresh during reconnect keeps the session and schedules a retry", async () => {
    vi.useFakeTimers();
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    fetchMock.mockResolvedValue(respond(503));
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    const open = vi.fn<() => Promise<void>>().mockResolvedValue(undefined);
    const onStatus = vi.fn();
    const supervisor = createConnectionSupervisor({
      driver: { open, close: vi.fn(), sendPing: vi.fn() },
      mode: "managed",
      onStatus,
      onRecovered: vi.fn().mockResolvedValue(undefined),
      ensureFreshToken: () => common.ensureFreshAccessToken(),
    });
    try {
      await supervisor.start();
      open.mockRejectedValueOnce(new Error("server restarting"));

      supervisor.notifyClosed(false);
      await vi.advanceTimersByTimeAsync(1000);

      expect(fetchMock).toHaveBeenCalled();
      expect(open).toHaveBeenCalledTimes(2);
      expect(expired).not.toHaveBeenCalled();
      expect(common.browserState.accessToken).toBe("old-token");
      expect(onStatus).toHaveBeenLastCalledWith("reconnecting", 2);
    } finally {
      supervisor.stop();
      window.removeEventListener("kaiku:session-expired", expired);
    }
  });

  it("a scheduled refresh that hits a 503 keeps the session and retries", async () => {
    vi.useFakeTimers();
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    try {
      fetchMock.mockResolvedValue(respond(503));
      common.scheduleTokenRefresh();

      // Token is within 60 s of expiry: the timer fires after the 10 s floor.
      await vi.advanceTimersByTimeAsync(10_000);
      expect(fetchMock).toHaveBeenCalledTimes(1);
      expect(expired).not.toHaveBeenCalled();
      expect(common.browserState.accessToken).toBe("old-token");

      // Retried 30 s later; a 401 then ends the session.
      fetchMock.mockResolvedValue(respond(401));
      await vi.advanceTimersByTimeAsync(29_000);
      expect(fetchMock).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1_000);
      expect(fetchMock).toHaveBeenCalledTimes(2);
      expect(expired).toHaveBeenCalledTimes(1);
      expect(common.browserState.accessToken).toBeNull();
    } finally {
      window.removeEventListener("kaiku:session-expired", expired);
    }
  });
});
