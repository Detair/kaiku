// Review round 1: stale attempts, server URL, signals, availability.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { browserState } from "../tauri/common";
import { passkeySignIn, PasskeyError } from "../tauri/passkeys";

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
let finishAnswer: () => Response;

beforeEach(() => {
  calls = [];
  browserState.serverUrl = "https://home.example.com";
  browserState.accessToken = "current-token";
  finishAnswer = () =>
    json({ access_token: "late-token", expires_in: 900, setup_required: false });
  vi.stubGlobal("PublicKeyCredential", {});
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit) => {
      calls.push({ url, init });
      if (url.endsWith("/auth/passkeys/login/start")) {
        return json({ ceremony_id: "c", options: { publicKey: { challenge: "AQ", rpId: "chat.example.com" } } });
      }
      if (url.endsWith("/auth/passkeys/login/finish")) return finishAnswer();
      if (url.endsWith("/auth/logout")) return new Response(null, { status: 204 });
      return json({ id: "u" });
    }),
  );
  vi.stubGlobal("navigator", { credentials: { get: vi.fn().mockResolvedValue(assertion) } });
});
afterEach(() => {
  vi.unstubAllGlobals();
  browserState.accessToken = null;
});

const run = (o: Partial<Parameters<typeof passkeySignIn>[1]> = {}) =>
  passkeySignIn("https://chat.example.com", {
    mediation: "conditional",
    signal: new AbortController().signal,
    ...o,
  });

describe("passkeySignIn round 1", () => {
  it("a stale attempt never installs its session and ends it", async () => {
    // Codex M1: another sign-in completed while this finish was in flight.
    let current = true;
    finishAnswer = () => {
      current = false;
      return json({ access_token: "late-token", expires_in: 900, setup_required: false });
    };
    await expect(run({ isCurrent: () => current })).rejects.toMatchObject({ code: "ABORTED" });
    expect(browserState.accessToken).toBe("current-token");
    const logout = calls.find((c) => c.url.endsWith("/auth/logout"));
    expect(logout).toBeTruthy();
    expect((logout!.init.headers as Record<string, string>).Authorization).toBe("Bearer late-token");
    expect(logout!.init.credentials).toBe("omit");
  });

  it("an attempt that went stale before finish sends nothing", async () => {
    await expect(run({ isCurrent: () => false })).rejects.toMatchObject({ code: "ABORTED" });
    expect(calls.some((c) => c.url.endsWith("/login/finish"))).toBe(false);
  });

  it("the server URL in use is untouched until the sign-in completes", async () => {
    // Argon M4: autofill on the login page must not repoint the client.
    (navigator.credentials.get as ReturnType<typeof vi.fn>).mockRejectedValue(
      new DOMException("no", "NotAllowedError"),
    );
    await expect(run()).rejects.toBeInstanceOf(PasskeyError);
    expect(browserState.serverUrl).toBe("https://home.example.com");
    expect(calls[0].url).toBe("https://chat.example.com/auth/passkeys/login/start");
  });

  it("the user name is read when the credential was chosen", async () => {
    let typed = "";
    (navigator.credentials.get as ReturnType<typeof vi.fn>).mockImplementation(async () => {
      typed = "fabian";
      return assertion;
    });
    await run({ username: () => typed });
    const finish = calls.find((c) => c.url.endsWith("/login/finish"))!;
    expect(JSON.parse(finish.init.body as string).username).toBe("fabian");
  });

  it("PASSKEY_UNKNOWN signals the credential; PASSKEY_PENDING does not", async () => {
    const signal = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("PublicKeyCredential", { signalUnknownCredential: signal });
    finishAnswer = () => json({ error: "PASSKEY_UNKNOWN", message: "x" }, 400);
    await expect(run()).rejects.toMatchObject({ code: "PASSKEY_UNKNOWN" });
    expect(signal).toHaveBeenCalledWith({ rpId: "chat.example.com", credentialId: "Y3JlZA" });
    signal.mockClear();
    finishAnswer = () => json({ error: "PASSKEY_PENDING", message: "x", ready_at: "2026-11-05T10:00:00Z" }, 409);
    await expect(run()).rejects.toMatchObject({ code: "PASSKEY_PENDING", readyAt: "2026-11-05T10:00:00Z" });
    expect(signal).not.toHaveBeenCalled();
  });

  it("without WebAuthn the code is PASSKEY_NOT_SUPPORTED", async () => {
    vi.stubGlobal("PublicKeyCredential", undefined);
    await expect(run()).rejects.toMatchObject({ code: "PASSKEY_NOT_SUPPORTED" });
  });
});
