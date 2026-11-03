import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: false }));
const invoke = vi.hoisted(() => vi.fn());
const fetchMock = vi.hoisted(() => {
  // No session restore at import: the module-level refresh would hit fetch.
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
  attemptTotp,
  cancelAttempt,
  collectAttempt,
  createQrAttempt,
  passwordLogin,
  pollAttempt,
} from "../loginAttempts";
import { browserState, clearBrowserTokens, httpRequest, isPreSessionPath } from "../common";

function json(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(body === undefined ? "" : JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json", ...headers },
  });
}

const attempt = {
  attempt_id: "44444444-4444-4444-8444-444444444444",
  kind: "qr",
  state: "open",
  expires_at: "2026-11-03T12:10:00Z",
  qr: { payload: "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=abc", expires_at: "2026-11-03T12:01:00Z" },
  totp_available: false,
};

function lastCall(): { url: string; init: RequestInit; body: unknown } {
  const [url, init] = fetchMock.mock.calls[fetchMock.mock.calls.length - 1] as [string, RequestInit];
  return { url, init, body: init.body ? JSON.parse(String(init.body)) : undefined };
}

beforeEach(() => {
  fetchMock.mockReset();
  invoke.mockReset();
  tauriMode.on = false;
  browserState.serverUrl = "https://chat.example.com";
  clearBrowserTokens();
});

afterEach(() => {
  clearBrowserTokens();
});

describe("login attempt wrappers (browser)", () => {
  it("createQrAttempt sends no client_challenge in the browser", async () => {
    fetchMock.mockResolvedValueOnce(json(201, attempt));
    const a = await createQrAttempt("https://chat.example.com");
    const { url, init, body } = lastCall();
    expect(url).toBe("https://chat.example.com/auth/login-attempts");
    expect(init.method).toBe("POST");
    expect(body).toEqual({});
    expect(init.credentials).toBe("include");
    expect(a.attempt_id).toBe(attempt.attempt_id);
  });

  it("passwordLogin returns the LoginStep of a 202 and never sends mfa_code", async () => {
    fetchMock.mockResolvedValueOnce(
      json(202, { status: "confirmation_required", attempt: { ...attempt, kind: "password" } }),
    );
    const step = await passwordLogin("https://chat.example.com", "alice", "pw");
    const { url, body } = lastCall();
    expect(url).toBe("https://chat.example.com/auth/login");
    expect(body).toEqual({ username: "alice", password: "pw" });
    expect(step.status).toBe("confirmation_required");
    expect(step.attempt.kind).toBe("password");
  });

  it("pollAttempt sends wait and no verifier in the browser", async () => {
    fetchMock.mockResolvedValueOnce(json(200, { ...attempt, qr: null }));
    const a = await pollAttempt(attempt.attempt_id, 25);
    const { url, body, init } = lastCall();
    expect(url).toBe(`https://chat.example.com/auth/login-attempts/${attempt.attempt_id}/poll`);
    expect(body).toEqual({ wait: 25 });
    expect(init.credentials).toBe("include");
    expect(a.qr).toBeNull();
  });

  it("collectAttempt stores the access token and schedules a refresh", async () => {
    vi.useFakeTimers();
    try {
      fetchMock
        .mockResolvedValueOnce(
          json(200, {
            access_token: "at-1",
            expires_in: 900,
            token_type: "Bearer",
            setup_required: false,
            session_id: "s1",
            assurance: "probation",
            absolute_expires_at: "2026-12-03T00:00:00Z",
          }),
        )
        .mockResolvedValueOnce(json(200, { id: "u1", username: "alice" }));
      const result = await collectAttempt(attempt.attempt_id);
      expect(browserState.accessToken).toBe("at-1");
      expect(browserState.refreshTimer).not.toBeNull();
      expect(result.user.username).toBe("alice");
      const me = lastCall();
      expect(me.url).toBe("https://chat.example.com/auth/me");
      expect((me.init.headers as Record<string, string>).Authorization).toBe("Bearer at-1");
    } finally {
      vi.useRealTimers();
    }
  });

  const authBody = {
    access_token: "at-late",
    expires_in: 900,
    token_type: "Bearer",
    setup_required: false,
    session_id: "s9",
    assurance: "probation",
    absolute_expires_at: "2026-12-03T00:00:00Z",
  };

  it("a collect the caller no longer wants is ended, never taken over", async () => {
    let wanted = true;
    fetchMock.mockImplementation(async (url: string) => {
      if (String(url).endsWith("/collect")) {
        wanted = false; // the user left while the server answered
        return json(200, authBody);
      }
      return new Response(null, { status: 204 });
    });
    await expect(collectAttempt(attempt.attempt_id, () => wanted)).rejects.toMatchObject({
      status: 410,
      code: "LOGIN_ATTEMPT_ENDED",
      details: { state: "cancelled" },
    });
    expect(browserState.accessToken).toBeNull();
    const urls = fetchMock.mock.calls.map(([u]) => String(u));
    expect(urls).not.toContain("https://chat.example.com/auth/me");
    const { url, init } = lastCall();
    expect(url).toBe("https://chat.example.com/auth/logout");
    expect(init.credentials).toBe("omit");
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer at-late");
  });

  it("a TOTP step abandoned during the profile load is ended too", async () => {
    let wanted = true;
    fetchMock.mockImplementation(async (url: string) => {
      if (String(url).endsWith("/totp")) return json(200, authBody);
      if (String(url).endsWith("/auth/me")) {
        wanted = false;
        return json(200, { id: "u1", username: "alice" });
      }
      return new Response(null, { status: 204 });
    });
    await expect(attemptTotp(attempt.attempt_id, "123456", () => wanted)).rejects.toMatchObject({
      code: "LOGIN_ATTEMPT_ENDED",
    });
    expect(browserState.accessToken).toBeNull();
    expect(lastCall().url).toBe("https://chat.example.com/auth/logout");
  });

  it("the profile is loaded from the server the collect went to", async () => {
    fetchMock.mockImplementation(async (url: string) => {
      if (String(url).endsWith("/collect")) {
        // Another sign-in picked a different server meanwhile.
        browserState.serverUrl = "https://other.example.org";
        return json(200, authBody);
      }
      return json(200, { id: "u1", username: "alice" });
    });
    await collectAttempt(attempt.attempt_id);
    expect(lastCall().url).toBe("https://chat.example.com/auth/me");
    expect(browserState.accessToken).toBe("at-late");
    clearBrowserTokens();
  });

  it("a failed profile load ends the new session and keeps no token", async () => {
    fetchMock.mockImplementation(async (url: string) => {
      if (String(url).endsWith("/collect")) return json(200, authBody);
      if (String(url).endsWith("/auth/me")) return json(503, { error: "SERVICE_UNAVAILABLE", message: "x" });
      return new Response(null, { status: 204 });
    });
    await expect(collectAttempt(attempt.attempt_id)).rejects.toMatchObject({ status: 503, code: "SERVICE_UNAVAILABLE" });
    expect(browserState.accessToken).toBeNull();
    expect(lastCall().url).toBe("https://chat.example.com/auth/logout");
  });

  it("attemptTotp and cancelAttempt post to their routes", async () => {
    fetchMock.mockResolvedValueOnce(json(401, { error: "INVALID_MFA", message: "x" }));
    await expect(attemptTotp(attempt.attempt_id, "123456")).rejects.toMatchObject({ code: "INVALID_MFA" });
    expect(lastCall().body).toEqual({ code: "123456" });
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await cancelAttempt(attempt.attempt_id);
    expect(lastCall().url).toBe(`https://chat.example.com/auth/login-attempts/${attempt.attempt_id}/cancel`);
  });

  it("TOO_MANY_ATTEMPTS keeps Retry-After on the error", async () => {
    fetchMock.mockResolvedValueOnce(
      json(429, { error: "TOO_MANY_ATTEMPTS", message: "slow down" }, { "Retry-After": "120" }),
    );
    await expect(passwordLogin("https://chat.example.com", "a", "b")).rejects.toMatchObject({
      status: 429,
      code: "TOO_MANY_ATTEMPTS",
      retryAfter: 120,
    });
  });

  it("unknown enum values decode as \"unknown\"", async () => {
    fetchMock.mockResolvedValueOnce(json(200, { ...attempt, state: "teleported", kind: "smoke" }));
    const a = await pollAttempt(attempt.attempt_id, 0);
    expect(a.state).toBe("unknown");
    expect(a.kind).toBe("unknown");
  });
});

describe("pre-session paths", () => {
  it("are recognised by prefix and exact match", () => {
    for (const p of [
      "/auth/login",
      "/auth/register",
      "/auth/refresh",
      "/auth/login-attempts",
      "/auth/login-attempts/x/poll",
      "/auth/account-requests",
      "/auth/account-requests/oidc/authorize/kc",
      "/auth/account-requests/current/finish",
      "/auth/registration",
      "/auth/registration/totp/verify",
      "/auth/enrollments/open",
      "/auth/enrollments/status",
      "/auth/enrollments/passkey/start",
      "/auth/device-login",
      "/auth/passkeys/login/start",
      "/auth/login?x=1",
    ]) {
      expect(isPreSessionPath(p), p).toBe(true);
    }
    for (const p of [
      "/auth/me",
      "/auth/enrollments",
      "/auth/enrollments/55555555-5555-4555-8555-555555555555",
      "/api/account-requests/x/cancel",
      "/api/me/account-requests",
      "/auth/logins",
      "/auth/passkeys",
    ]) {
      expect(isPreSessionPath(p), p).toBe(false);
    }
  });

  it("pre-session paths get no Authorization header and no refresh retry on 401", async () => {
    browserState.accessToken = "at-live";
    fetchMock.mockResolvedValueOnce(json(401, { error: "INVALID_CREDENTIALS", message: "no" }));
    await expect(httpRequest("POST", "/auth/login-attempts/x/poll", { wait: 1 })).rejects.toMatchObject({
      status: 401,
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const headers = lastCall().init.headers as Record<string, string>;
    expect(headers.Authorization).toBeUndefined();
  });
});

describe("login attempt wrappers (desktop)", () => {
  it("Tauri mode calls login_attempt_create, login_attempt_poll, login_attempt_collect with literal names", async () => {
    tauriMode.on = true;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "login_attempt_create") return attempt;
      if (cmd === "login_attempt_poll") return { ...attempt, state: "approved", qr: null };
      if (cmd === "login_attempt_collect") {
        return { user: { id: "u1" }, setup_required: false, password_change_required: false };
      }
      if (cmd === "get_auth_info") return null;
      throw new Error(`unexpected ${cmd}`);
    });
    await createQrAttempt("https://chat.example.com/");
    expect(invoke).toHaveBeenCalledWith("login_attempt_create", { serverUrl: "https://chat.example.com" });
    const polled = await pollAttempt(attempt.attempt_id, 25);
    expect(invoke).toHaveBeenCalledWith("login_attempt_poll", { attemptId: attempt.attempt_id, wait: 25 });
    expect(polled.state).toBe("approved");
    await collectAttempt(attempt.attempt_id);
    expect(invoke).toHaveBeenCalledWith("login_attempt_collect", { attemptId: attempt.attempt_id });
    expect(invoke).toHaveBeenCalledWith("get_auth_info");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("desktop password login sends no mfa_code and maps a bare code", async () => {
    tauriMode.on = true;
    invoke.mockResolvedValueOnce({ status: "totp_required", attempt: { ...attempt, kind: "password" } });
    const step = await passwordLogin("https://chat.example.com", "alice", "pw");
    expect(invoke).toHaveBeenCalledWith("login", {
      request: { server_url: "https://chat.example.com", username: "alice", password: "pw" },
    });
    expect(step.status).toBe("totp_required");
    invoke.mockRejectedValueOnce("TOO_MANY_ATTEMPTS");
    await expect(passwordLogin("https://chat.example.com", "a", "b")).rejects.toMatchObject({
      code: "TOO_MANY_ATTEMPTS",
    });
  });
});
