import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as Common from "../tauri/common";
import type * as Auth from "../tauri/auth";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

const fetchMock = vi.fn();

function respond(status: number, body: unknown = {}) {
  const text = body === null ? "" : JSON.stringify(body);
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: "",
    json: vi.fn().mockResolvedValue(body),
    text: vi.fn().mockResolvedValue(text),
  };
}

function info(token: string) {
  return {
    server_url: "https://srv",
    access_token: token,
    expires_at_ms: Date.now() + 900_000,
  };
}

type FetchInit = { headers: Record<string, string>; credentials?: string };

function fetchInit(call: number): FetchInit {
  return fetchMock.mock.calls[call][1] as FetchInit;
}

describe("native auth", () => {
  let common: typeof Common;
  let auth: typeof Auth;

  beforeEach(async () => {
    invokeMock.mockReset();
    listenMock.mockReset().mockResolvedValue(() => {});
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    (window as unknown as Record<string, unknown>).__TAURI__ = {};
    vi.resetModules();
    common = await import("../tauri/common");
    auth = await import("../tauri/auth");
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI__;
    vi.unstubAllGlobals();
  });

  it("login in native mode returns AuthResult and caches auth info", async () => {
    const result = {
      user: { id: "u1", username: "alice" },
      setup_required: false,
      password_change_required: false,
      persisted: true,
    };
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "login") return result;
      if (cmd === "get_auth_info") return info("tok");
      throw new Error(`unexpected ${cmd}`);
    });

    const got = await auth.login("https://srv", "alice", "pw");

    expect(got).toEqual(result);
    expect(invokeMock.mock.calls[0][0]).toBe("login");
    expect(invokeMock.mock.calls[1][0]).toBe("get_auth_info");
    expect(common.getServerUrl()).toBe("https://srv");
    expect((await common.getAuthContext()).accessToken).toBe("tok");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("httpRequest in native mode uses the Rust base URL and bearer", async () => {
    common.setNativeAuthInfo(info("tok"));
    fetchMock.mockResolvedValue(respond(200, { ok: true }));

    await common.httpRequest("GET", "/api/guilds");

    expect(String(fetchMock.mock.calls[0][0])).toMatch(/^https:\/\/srv\/api\//);
    const init = fetchInit(0);
    expect(init.headers["Authorization"]).toBe("Bearer tok");
    expect(init.headers["X-Refresh-Token"]).toBeUndefined();
    expect(init.credentials).not.toBe("include");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("httpRequest retries once after 401 via refresh_session", async () => {
    common.setNativeAuthInfo(info("tok"));
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "refresh_session") return info("tok2");
      throw new Error(`unexpected ${cmd}`);
    });
    fetchMock
      .mockResolvedValueOnce(respond(401, { error: "UNAUTHORIZED" }))
      .mockResolvedValueOnce(respond(200, { ok: true }));

    await expect(common.httpRequest("GET", "/api/guilds")).resolves.toEqual({
      ok: true,
    });
    expect(fetchInit(1).headers["Authorization"]).toBe("Bearer tok2");

    // A second 401 after the refresh is thrown, not retried again.
    fetchMock.mockReset();
    invokeMock.mockClear();
    fetchMock.mockResolvedValue(respond(401, { error: "UNAUTHORIZED" }));

    await expect(common.httpRequest("GET", "/api/guilds")).rejects.toMatchObject({
      status: 401,
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("refresh_session");
  });

  it("refresh_session returning null dispatches kaiku:session-expired", async () => {
    common.setNativeAuthInfo(info("tok"));
    invokeMock.mockResolvedValue(null);
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    try {
      expect(await common.refreshAccessToken()).toBe(false);
      expect(expired).toHaveBeenCalledTimes(1);
      expect(await common.ensureFreshAccessToken()).toBe(false);
    } finally {
      window.removeEventListener("kaiku:session-expired", expired);
    }
  });

  it("a failed refresh_session call keeps the session", async () => {
    common.setNativeAuthInfo({ ...info("tok"), expires_at_ms: Date.now() + 10_000 });
    invokeMock.mockRejectedValue(new Error("Connection failed: offline"));
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    try {
      expect(await common.refreshAccessToken()).toBe(false);
      expect(await common.ensureFreshAccessToken()).toBe(true);
      expect(expired).not.toHaveBeenCalled();
      expect((await common.getAuthContext()).accessToken).toBe("tok");
    } finally {
      window.removeEventListener("kaiku:session-expired", expired);
    }
  });

  it("listSessions in native mode goes through session_request", async () => {
    const sessions = { sessions: [] };
    invokeMock.mockResolvedValue(sessions);

    await expect(auth.listSessions()).resolves.toEqual(sessions);

    expect(invokeMock).toHaveBeenCalledWith("session_request", {
      method: "GET",
      path: "/auth/sessions",
      body: null,
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("native auth events update the cache and expire the session", async () => {
    const handlers: Record<string, (e: { payload: unknown }) => void> = {};
    listenMock.mockImplementation(async (name: string, cb: (e: { payload: unknown }) => void) => {
      handlers[name] = cb;
      return () => {};
    });
    const expired = vi.fn();
    window.addEventListener("kaiku:session-expired", expired);
    try {
      await common.initNativeAuthEvents();
      await common.initNativeAuthEvents();
      expect(listenMock).toHaveBeenCalledTimes(2);

      handlers["auth:token_refreshed"]({ payload: info("tok3") });
      expect((await common.getAuthContext()).accessToken).toBe("tok3");

      handlers["auth:session_expired"]({ payload: null });
      expect(expired).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("kaiku:session-expired", expired);
    }
  });
});

describe("browser auth", () => {
  it("httpRequest in browser mode still sends credentials and no X-Refresh-Token", async () => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    delete (window as unknown as Record<string, unknown>).__TAURI__;
    // Importing common.ts starts a cookie session-restore refresh.
    fetchMock.mockResolvedValue(respond(401));
    vi.resetModules();
    const common = await import("../tauri/common");
    await common.refreshAccessToken();
    fetchMock.mockReset();
    common.browserState.accessToken = "browser-tok";
    fetchMock.mockResolvedValue(respond(200, { ok: true }));

    await common.httpRequest("GET", "/api/guilds");

    const init = fetchInit(0);
    expect(init.credentials).toBe("include");
    expect(init.headers["Authorization"]).toBe("Bearer browser-tok");
    expect(init.headers["X-Refresh-Token"]).toBeUndefined();
    expect(invokeMock).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
