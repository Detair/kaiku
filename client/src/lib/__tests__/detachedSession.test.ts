import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type * as Common from "../tauri/common";
import type * as Auth from "../tauri/auth";

function respond(status: number, body: unknown = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: "",
    json: vi.fn().mockResolvedValue(body),
  };
}

const fetchMock = vi.fn();
let common: typeof Common;
let auth: typeof Auth;

beforeAll(async () => {
  vi.stubGlobal("fetch", fetchMock);
  // Importing common.ts starts a session-restore refresh; let it settle.
  fetchMock.mockResolvedValue(respond(401));
  common = await import("../tauri/common");
  auth = await import("../tauri/auth");
  await common.refreshAccessToken();
});

describe("detached session helpers (SSO popup restore)", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    common.clearBrowserTokens();
  });
  afterEach(() => common.clearBrowserTokens());

  it("a refresh for A leaves B's active token and refresh timer alone", async () => {
    await auth.oidcCompleteLogin(window.location.origin, "token-b", 900);
    const timer = common.browserState.refreshTimer;
    fetchMock.mockResolvedValue(respond(200, { access_token: "token-a", expires_in: 900 }));

    await expect(auth.refreshDetached()).resolves.toEqual({ accessToken: "token-a", expiresIn: 900 });

    expect(common.getAccessToken()).toBe("token-b");
    expect(common.browserState.refreshTimer).toBe(timer);
    expect(fetchMock.mock.calls[0][0]).toMatch(/\/auth\/refresh$/);
    expect(fetchMock.mock.calls[0][1]).toMatchObject({ method: "POST", credentials: "include" });
  });

  it("a rejected refresh returns null and clears nothing", async () => {
    await auth.oidcCompleteLogin(window.location.origin, "token-b", 900);
    fetchMock.mockResolvedValue(respond(401));

    await expect(auth.refreshDetached()).resolves.toBeNull();

    expect(common.getAccessToken()).toBe("token-b");
  });

  it("reads /auth/me with the given token, not the active one", async () => {
    await auth.oidcCompleteLogin(window.location.origin, "token-b", 900);
    fetchMock.mockResolvedValue(respond(200, { id: "a", username: "alice", password_change_required: true }));

    const me = await auth.getCurrentUserWithToken("token-a");

    expect(me.user.id).toBe("a");
    expect(me.password_change_required).toBe(true);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toMatch(/\/auth\/me$/);
    expect(init.headers.Authorization).toBe("Bearer token-a");
  });

  it("revokes a discarded session with its own token", async () => {
    fetchMock.mockResolvedValue(respond(204));

    await auth.revokeDetachedSession("token-a");

    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toMatch(/\/auth\/logout$/);
    expect(init).toMatchObject({ method: "POST", credentials: "include" });
    expect(init.headers.Authorization).toBe("Bearer token-a");
  });

  it("a failed revoke does not throw", async () => {
    fetchMock.mockRejectedValue(new TypeError("offline"));
    await expect(auth.revokeDetachedSession("token-a")).resolves.toBeUndefined();
  });
});
