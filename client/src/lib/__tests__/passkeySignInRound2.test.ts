// Review round 2 (Codex 1.1, Argon B R1): a passkey sign-in that goes stale
// while finish or /auth/me is in flight changes no global auth state and
// ends the session it minted.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { browserState } from "../tauri/common";
import { passkeySignIn } from "../tauri/passkeys";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

const assertion = {
  id: "Y3JlZA",
  rawId: new Uint8Array([1]).buffer,
  type: "public-key",
  response: {
    clientDataJSON: new Uint8Array([1]).buffer,
    authenticatorData: new Uint8Array([1]).buffer,
    signature: new Uint8Array([1]).buffer,
    userHandle: null,
  },
};

let calls: { url: string; init: RequestInit }[] = [];
let onFinish: () => void;
let onMe: () => void;
let ctl: AbortController;
let current: boolean;

beforeEach(() => {
  calls = [];
  current = true;
  ctl = new AbortController();
  onFinish = () => {};
  onMe = () => {};
  browserState.serverUrl = "https://home.example.com";
  browserState.accessToken = "current-token";
  localStorage.setItem("serverUrl", "https://home.example.com");
  localStorage.setItem("kaiku:skip-session-restore", "1");
  vi.stubGlobal("PublicKeyCredential", {});
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit = {}) => {
      calls.push({ url, init });
      const abortable = () => {
        if (init.signal?.aborted) throw new DOMException("aborted", "AbortError");
      };
      if (url.endsWith("/auth/passkeys/login/start")) {
        return json({ ceremony_id: "c", options: { publicKey: { challenge: "AQ", rpId: "chat.example.com" } } });
      }
      if (url.endsWith("/auth/passkeys/login/finish")) {
        onFinish();
        abortable();
        return json({ access_token: "late-token", expires_in: 900, setup_required: false });
      }
      if (url.endsWith("/auth/logout")) return new Response(null, { status: 204 });
      if (url.endsWith("/auth/me")) {
        onMe();
        abortable();
        return json({ id: "u", username: "fabian" });
      }
      return json({});
    }),
  );
  vi.stubGlobal("navigator", { credentials: { get: vi.fn().mockResolvedValue(assertion) } });
});
afterEach(() => {
  vi.unstubAllGlobals();
  browserState.accessToken = null;
  localStorage.clear();
});

const run = () =>
  passkeySignIn("https://chat.example.com", {
    mediation: "optional",
    signal: ctl.signal,
    isCurrent: () => current,
  });

/** The attempt was invalidated: nothing global changed, the session ended. */
function expectUntouchedAndEnded() {
  expect(browserState.accessToken).toBe("current-token");
  expect(browserState.serverUrl).toBe("https://home.example.com");
  expect(localStorage.getItem("serverUrl")).toBe("https://home.example.com");
  expect(localStorage.getItem("kaiku:skip-session-restore")).toBe("1");
  const logout = calls.find((c) => c.url === "https://chat.example.com/auth/logout");
  expect(logout).toBeTruthy();
  expect((logout!.init.headers as Record<string, string>).Authorization).toBe("Bearer late-token");
}

describe("passkeySignIn round 2", () => {
  it("an attempt invalidated during finish lets it land and ends that session", async () => {
    onFinish = () => {
      current = false;
      ctl.abort();
    };
    await expect(run()).rejects.toMatchObject({ code: "ABORTED" });
    expectUntouchedAndEnded();
  });

  it("an attempt invalidated during /auth/me changes nothing and ends the session", async () => {
    onMe = () => {
      current = false;
      ctl.abort();
    };
    await expect(run()).rejects.toMatchObject({ code: "ABORTED" });
    expectUntouchedAndEnded();
  });

  it("a current attempt installs the session after /auth/me with the new token", async () => {
    const result = await run();
    expect(result.user).toMatchObject({ id: "u" });
    const me = calls.find((c) => c.url.endsWith("/auth/me"))!;
    expect(me.url).toBe("https://chat.example.com/auth/me");
    expect((me.init.headers as Record<string, string>).Authorization).toBe("Bearer late-token");
    expect(browserState.accessToken).toBe("late-token");
    expect(browserState.serverUrl).toBe("https://chat.example.com");
    expect(localStorage.getItem("serverUrl")).toBe("https://chat.example.com");
    expect(localStorage.getItem("kaiku:skip-session-restore")).toBeNull();
  });
});
