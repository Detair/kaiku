import { describe, it, expect, vi, afterEach } from "vitest";
import {
  credentialToJSON,
  creationOptionsFromJSON,
  requestOptionsFromJSON,
  signalRemoved,
  signalUnknown,
  webAuthnErrorCode,
} from "../webauthn";

const bytes = (...b: number[]) => new Uint8Array(b).buffer;
const view = (b: ArrayBuffer | ArrayBufferView) =>
  Array.from(new Uint8Array(b instanceof ArrayBuffer ? b : (b as ArrayBufferView).buffer));

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("webauthn JSON helpers", () => {
  it("requestOptionsFromJSON decodes challenge and allowCredentials", () => {
    const o = requestOptionsFromJSON({
      challenge: "AQID",
      allowCredentials: [{ type: "public-key", id: "BAU" }],
      userVerification: "required",
      rpId: "chat.example.com",
    });
    expect(view(o.challenge as ArrayBuffer)).toEqual([1, 2, 3]);
    expect(view(o.allowCredentials![0].id as ArrayBuffer)).toEqual([4, 5]);
    expect(o.userVerification).toBe("required");
    expect(o.rpId).toBe("chat.example.com");
  });

  it("creationOptionsFromJSON decodes user.id and excludeCredentials", () => {
    const o = creationOptionsFromJSON({
      challenge: "AQID",
      rp: { id: "chat.example.com", name: "Kaiku" },
      user: { id: "-_8", name: "alice", displayName: "Alice" },
      pubKeyCredParams: [{ type: "public-key", alg: -7 }],
      excludeCredentials: [{ type: "public-key", id: "BgcI" }],
      authenticatorSelection: { residentKey: "required", userVerification: "required" },
    });
    expect(view(o.user.id as ArrayBuffer)).toEqual([0xfb, 0xff]);
    expect(view(o.excludeCredentials![0].id as ArrayBuffer)).toEqual([6, 7, 8]);
    expect(o.authenticatorSelection?.residentKey).toBe("required");
  });

  it("credentialToJSON encodes base64url without padding", () => {
    const cred = {
      id: "x",
      rawId: bytes(0xfb, 0xff),
      type: "public-key",
      authenticatorAttachment: "platform",
      getClientExtensionResults: () => ({}),
      response: {
        clientDataJSON: bytes(1),
        authenticatorData: bytes(2, 3),
        signature: bytes(0xff),
        userHandle: bytes(9, 9, 9, 9),
      },
    } as unknown as PublicKeyCredential;
    const j = credentialToJSON(cred) as { rawId: string; response: Record<string, string> };
    expect(j.rawId).toBe("-_8");
    expect(j.response.clientDataJSON).toBe("AQ");
    expect(j.response.signature).toBe("_w");
    expect(j.response.userHandle).toBe("CQkJCQ");
    expect(JSON.stringify(j)).not.toContain("=");
  });

  it("native parse and toJSON are used when present", () => {
    const parsed = { challenge: new ArrayBuffer(1) };
    const parse = vi.fn(() => parsed);
    vi.stubGlobal("PublicKeyCredential", { parseRequestOptionsFromJSON: parse });
    expect(requestOptionsFromJSON({ challenge: "AQ" })).toBe(parsed);
    expect(parse).toHaveBeenCalledOnce();
    const toJSON = vi.fn(() => ({ native: true }));
    expect(credentialToJSON({ toJSON } as unknown as PublicKeyCredential)).toEqual({ native: true });
  });
});

describe("webAuthnErrorCode", () => {
  it("NotAllowedError maps to PASSKEY_CANCELLED", () => {
    expect(webAuthnErrorCode(new DOMException("no", "NotAllowedError"))).toBe("PASSKEY_CANCELLED");
  });
  it("InvalidStateError maps to PASSKEY_ALREADY_REGISTERED", () => {
    expect(webAuthnErrorCode(new DOMException("dup", "InvalidStateError"))).toBe(
      "PASSKEY_ALREADY_REGISTERED",
    );
  });
  it("AbortError maps to ABORTED and others to PASSKEY_FAILED", () => {
    expect(webAuthnErrorCode(new DOMException("a", "AbortError"))).toBe("ABORTED");
    expect(webAuthnErrorCode(new DOMException("n", "NotSupportedError"))).toBe("PASSKEY_NOT_SUPPORTED");
    expect(webAuthnErrorCode(new Error("x"))).toBe("PASSKEY_FAILED");
  });
});

describe("signals", () => {
  const removed = {
    rp_id: "chat.example.com",
    user_handle: "dXNlcg",
    credential_id: "Y3JlZA",
    remaining_credential_ids: ["b3RoZXI"],
  };

  it("signalRemoved calls both signal APIs when present", async () => {
    const unknown = vi.fn().mockResolvedValue(undefined);
    const all = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("PublicKeyCredential", {
      signalUnknownCredential: unknown,
      signalAllAcceptedCredentials: all,
    });
    await signalRemoved(removed);
    expect(unknown).toHaveBeenCalledWith({ rpId: "chat.example.com", credentialId: "Y3JlZA" });
    expect(all).toHaveBeenCalledWith({
      rpId: "chat.example.com",
      userId: "dXNlcg",
      allAcceptedCredentialIds: ["b3RoZXI"],
    });
  });

  it("signalRemoved is a no-op without them", async () => {
    vi.stubGlobal("PublicKeyCredential", {});
    await expect(signalRemoved(removed)).resolves.toBeUndefined();
    vi.stubGlobal("PublicKeyCredential", undefined);
    await expect(signalRemoved(removed)).resolves.toBeUndefined();
  });

  it("signalUnknown never throws", async () => {
    vi.stubGlobal("PublicKeyCredential", {
      signalUnknownCredential: vi.fn().mockRejectedValue(new Error("boom")),
    });
    await expect(signalUnknown("chat.example.com", "Y3JlZA")).resolves.toBeUndefined();
  });
});
