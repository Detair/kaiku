import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { isSessionRestoreBlocked, setSessionRestoreBlocked } from "../tauri/common";
import { passkeySignIn } from "../tauri/passkeys";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

beforeEach(() => {
  setSessionRestoreBlocked(true);
  vi.stubGlobal("PublicKeyCredential", {});
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url.endsWith("/auth/passkeys/login/start")) {
        return json({ ceremony_id: "c", options: { publicKey: { challenge: "AQ", rpId: "chat.example.com" } } });
      }
      return json({ error: "PASSKEY_INVALID", message: "x" }, 400);
    }),
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  setSessionRestoreBlocked(false);
});

describe("passkeySignIn (browser)", () => {
  it("a conditional request that never completes keeps the logout's restore block", async () => {
    // Review I1: autofill on the login page must not re-enable the restore
    // of a session that logged out while the server was unreachable.
    vi.stubGlobal("navigator", {
      credentials: { get: vi.fn().mockRejectedValue(new DOMException("no", "NotAllowedError")) },
    });
    await expect(
      passkeySignIn("http://127.0.0.1:9", { mediation: "conditional", signal: new AbortController().signal }),
    ).rejects.toMatchObject({ code: "PASSKEY_CANCELLED" });
    expect(isSessionRestoreBlocked()).toBe(true);
  });

  it("a refused finish keeps the restore block too", async () => {
    vi.stubGlobal("navigator", {
      credentials: {
        get: vi.fn().mockResolvedValue({
          id: "x",
          rawId: new Uint8Array([1]).buffer,
          type: "public-key",
          response: {
            clientDataJSON: new Uint8Array([1]).buffer,
            authenticatorData: new Uint8Array([1]).buffer,
            signature: new Uint8Array([1]).buffer,
            userHandle: null,
          },
        }),
      },
    });
    await expect(
      passkeySignIn("http://127.0.0.1:9", { mediation: "optional", signal: new AbortController().signal }),
    ).rejects.toMatchObject({ code: "PASSKEY_INVALID" });
    expect(isSessionRestoreBlocked()).toBe(true);
  });
});
