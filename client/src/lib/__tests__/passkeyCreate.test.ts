// Passkey creation for setup links, requests and tickets (C29): LP's
// WebAuthn helpers, not the removed L2a seam.
import { describe, it, expect, vi, afterEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on }));

import { createPasskeyCredential, PasskeyError } from "../tauri/passkeys";

const options = {
  publicKey: {
    challenge: "AQID",
    rp: { id: "chat.example.com", name: "Kaiku" },
    user: { id: "-_8", name: "oma", displayName: "Oma" },
    pubKeyCredParams: [{ type: "public-key", alg: -7 }],
  },
};

function browser(create: (o: CredentialCreationOptions) => Promise<unknown>) {
  vi.stubGlobal("PublicKeyCredential", {});
  vi.stubGlobal("navigator", { ...navigator, credentials: { get: vi.fn(), create: vi.fn(create) } });
}

afterEach(() => {
  vi.unstubAllGlobals();
  tauriMode.on = false;
});

describe("createPasskeyCredential", () => {
  it("creates from the C29 options and answers the credential JSON", async () => {
    let seen: CredentialCreationOptions | undefined;
    browser(async (o) => {
      seen = o;
      return { toJSON: () => ({ id: "cred", type: "public-key" }) };
    });
    const signal = new AbortController().signal;
    await expect(createPasskeyCredential(options, signal)).resolves.toEqual({ id: "cred", type: "public-key" });
    expect(Array.from(new Uint8Array(seen!.publicKey!.challenge as ArrayBuffer))).toEqual([1, 2, 3]);
    expect(seen!.signal).toBe(signal);
  });

  it("a cancelled sheet is PASSKEY_CANCELLED", async () => {
    browser(async () => {
      throw new DOMException("no", "NotAllowedError");
    });
    await expect(createPasskeyCredential(options, new AbortController().signal)).rejects.toMatchObject({
      code: "PASSKEY_CANCELLED",
    });
  });

  it("the desktop app never creates a passkey", async () => {
    tauriMode.on = true;
    browser(async () => ({}));
    const err = await createPasskeyCredential(options, new AbortController().signal).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(PasskeyError);
    expect((err as PasskeyError).code).toBe("PASSKEY_NOT_SUPPORTED");
  });
});
