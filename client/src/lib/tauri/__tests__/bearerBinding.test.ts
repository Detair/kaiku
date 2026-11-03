// Argon review 5 #1 (Fable: "the bearer never goes to a typed URL"):
// settled. A pre-session page naming another server (useBrowserServer) used
// to pair the new base URL with the old session's bearer. The bearer is now
// bound to the server that issued it.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

import { browserState, clearBrowserTokens, httpRequest, setBrowserToken } from "../common";
import { useBrowserServer } from "../preSession";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

beforeEach(() => {
  fetchMock.mockReset();
  fetchMock.mockImplementation(async () => json(200, {}));
});
afterEach(() => clearBrowserTokens());

describe("the browser bearer stays with its server", () => {
  it("a pre-session page naming another server never gets the session's bearer", async () => {
    setBrowserToken("https://chat.example.com", "at-session", 900);
    useBrowserServer("https://other.example.org");
    await httpRequest("GET", "/api/guilds");
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("https://other.example.org/api/guilds");
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined();
  });

  it("a token without a bound server is never sent (fail closed, Fable R5)", async () => {
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "at-unbound";
    browserState.tokenServerUrl = null;
    await httpRequest("GET", "/api/guilds");
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect((init.headers as Record<string, string>).Authorization).toBeUndefined();
  });

  it("the session's own server still gets it", async () => {
    setBrowserToken("https://chat.example.com/", "at-session", 900);
    await httpRequest("GET", "/api/guilds");
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer at-session");
    expect(browserState.serverUrl).toBe("https://chat.example.com");
  });

  it("a refresh goes to the session's server, not the named one", async () => {
    setBrowserToken("https://chat.example.com", "at-session", 900);
    useBrowserServer("https://other.example.org");
    fetchMock.mockImplementation(async () =>
      json(200, { access_token: "at-2", expires_in: 900, token_type: "Bearer", setup_required: false }),
    );
    const { refreshAccessToken } = await import("../common");
    await refreshAccessToken();
    expect(fetchMock.mock.calls[0][0]).toBe("https://chat.example.com/auth/refresh");
  });
});
