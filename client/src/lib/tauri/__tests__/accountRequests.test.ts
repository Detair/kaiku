import { describe, it, expect, vi, beforeEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: false }));
const invoke = vi.hoisted(() => vi.fn());
const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  invoke,
  isTauri: () => tauriMode.on,
}));

import {
  finishRequest,
  getCurrentRequest,
  requestOidcAuthorize,
  setRequestPassword,
  setRequestRecoveryContact,
  startAccountRequest,
  startRequestTotp,
  verifyRequestTotp,
  withdrawRequest,
} from "../accountRequests";
import { browserState } from "../common";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

const request = { id: "r1", kind: "self", purpose: "recovery", state: "waiting", created_at: "a", ready_at: "b", expires_at: null, factors: ["recovery_code", "password"], lost_key_devices: 0, staged: { authenticator: null, password: false, totp: false, recovery_contact: null } };

beforeEach(() => {
  fetchMock.mockReset();
  invoke.mockReset();
  tauriMode.on = false;
  browserState.serverUrl = "https://chat.example.com";
  browserState.accessToken = "at";
});

describe("account request wrappers", () => {
  it("start posts the body without a bearer and keeps cookies", async () => {
    fetchMock.mockResolvedValueOnce(json(201, { request }));
    const r = await startAccountRequest("https://chat.example.com", {
      username: "alice",
      purpose: "recovery",
      recovery_code: "ABCDEFGHJKMNP",
      second: { type: "password", password: "pw" },
      lost: "all_key_devices",
    });
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("https://chat.example.com/auth/account-requests");
    expect(init.credentials).toBe("include");
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined();
    expect(JSON.parse(String(init.body)).lost).toBe("all_key_devices");
    expect(r.state).toBe("waiting");
  });

  it("request client routes", async () => {
    fetchMock.mockResolvedValueOnce(json(200, { ...request, state: "gone_fishing" }));
    expect((await getCurrentRequest()).state).toBe("unknown");
    const routes: [() => Promise<unknown>, string, string, unknown][] = [
      [() => withdrawRequest(), "DELETE", "/auth/account-requests/current", undefined],
      [() => setRequestPassword("pw"), "PUT", "/auth/account-requests/current/password", { password: "pw" }],
      [() => startRequestTotp(), "POST", "/auth/account-requests/current/totp", undefined],
      [() => verifyRequestTotp("123456"), "POST", "/auth/account-requests/current/totp/verify", { code: "123456" }],
      [() => setRequestRecoveryContact("bob"), "PUT", "/auth/account-requests/current/recovery-contact", { username: "bob" }],
      [() => finishRequest(), "POST", "/auth/account-requests/current/finish", undefined],
    ];
    for (const [run, method, path, body] of routes) {
      fetchMock.mockResolvedValueOnce(json(200, { request, recovery_codes: [] }));
      await run();
      const [url, init] = fetchMock.mock.lastCall as [string, RequestInit];
      expect(url).toBe(`https://chat.example.com${path}`);
      expect(init.method).toBe(method);
      if (body !== undefined) expect(JSON.parse(String(init.body))).toEqual(body);
    }
  });

  it("desktop keeps the request secret in Rust", async () => {
    tauriMode.on = true;
    invoke.mockResolvedValueOnce({ request });
    await startAccountRequest("https://chat.example.com", {
      username: "alice",
      purpose: "bootstrap",
      first: { type: "password", password: "pw" },
      totp_code: "123456",
    });
    expect(invoke).toHaveBeenCalledWith("account_request_start", {
      serverUrl: "https://chat.example.com",
      body: { username: "alice", purpose: "bootstrap", first: { type: "password", password: "pw" }, totp_code: "123456" },
    });
    invoke.mockResolvedValueOnce(request);
    await getCurrentRequest();
    expect(invoke).toHaveBeenLastCalledWith("pre_session_request", {
      kind: "request",
      method: "GET",
      path: "/auth/account-requests/current",
      body: null,
    });
    invoke.mockResolvedValueOnce({ oidc_code: "c" });
    expect(await requestOidcAuthorize("https://chat.example.com", "kc")).toEqual({ mode: "tauri", oidcCode: "c" });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("browser OIDC factor returns the provider url", async () => {
    fetchMock.mockResolvedValueOnce(json(200, { url: "https://idp.example.com/x" }));
    expect(await requestOidcAuthorize("https://chat.example.com", "kc")).toEqual({
      mode: "browser",
      url: "https://idp.example.com/x",
    });
    expect(fetchMock.mock.calls[0][0]).toBe("https://chat.example.com/auth/account-requests/oidc/authorize/kc");
  });
});
