import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

import { browserState, clearBrowserTokens, refreshAccessToken, SESSION_ENDED_EVENT } from "../common";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

const tokens = { access_token: "at-2", expires_in: 900, token_type: "Bearer", setup_required: false };

beforeEach(() => {
  fetchMock.mockReset();
  browserState.serverUrl = "https://chat.example.com";
  browserState.accessToken = "at-1";
});

afterEach(() => {
  clearBrowserTokens();
  Reflect.deleteProperty(navigator, "locks");
});

describe("refresh across tabs", () => {
  it("refresh runs under a Web Lock when available", async () => {
    const order: string[] = [];
    const request = vi.fn(async (name: string, cb: () => Promise<unknown>) => {
      order.push(`lock:${name}`);
      const r = await cb();
      order.push("unlock");
      return r;
    });
    Object.defineProperty(navigator, "locks", { configurable: true, value: { request } });
    fetchMock.mockImplementation(async () => {
      order.push("fetch");
      return json(200, tokens);
    });
    expect(await refreshAccessToken()).toBe(true);
    expect(order).toEqual(["lock:kaiku-refresh", "fetch", "unlock"]);
    expect(browserState.accessToken).toBe("at-2");
  });

  it("refresh falls back to single flight without navigator.locks", async () => {
    let resolve!: (r: Response) => void;
    fetchMock.mockImplementation(() => new Promise<Response>((r) => (resolve = r)));
    const a = refreshAccessToken();
    const b = refreshAccessToken();
    expect(a).toBe(b);
    resolve(json(200, tokens));
    expect(await a).toBe(true);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("SESSION_REUSED ends the session with reason reused", async () => {
    const ended = vi.fn();
    window.addEventListener(SESSION_ENDED_EVENT, ended);
    fetchMock.mockImplementation(async () => json(401, { error: "SESSION_REUSED", message: "x" }));
    expect(await refreshAccessToken()).toBe(false);
    expect(ended).toHaveBeenCalledTimes(1);
    expect((ended.mock.calls[0][0] as CustomEvent).detail).toEqual({ reason: "reused" });
    expect(browserState.accessToken).toBeNull();
    window.removeEventListener(SESSION_ENDED_EVENT, ended);
  });
});
