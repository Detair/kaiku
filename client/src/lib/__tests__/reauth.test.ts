import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("@/lib/tauri", () => {
  class HttpError extends Error {
    constructor(
      public status: number,
      message: string,
      public code?: string,
    ) {
      super(message);
    }
  }
  return {
    HttpError,
    getServerUrl: () => "https://chat.example",
    getReauthMethods: vi.fn(),
    startOidcReauth: vi.fn(),
  };
});

import { HttpError } from "@/lib/tauri";
import { infoFromServer, waitForBrowserReauth, reauthErrorText } from "../reauth";

const kc = [{ provider_id: "kc", display_name: "Keycloak" }];

describe("infoFromServer", () => {
  it("password only", () => {
    expect(infoFromServer({ password: true, mfa_required: false, oidc: [] })).toEqual({
      methods: [{ kind: "password" }],
      mfaRequired: false,
    });
  });
  it("oidc only", () => {
    expect(infoFromServer({ password: false, mfa_required: true, oidc: kc })).toEqual({
      methods: [{ kind: "oidc", providerId: "kc", label: "Keycloak" }],
      mfaRequired: true,
    });
  });
  it("both", () => {
    expect(infoFromServer({ password: true, mfa_required: false, oidc: kc }).methods).toEqual([
      { kind: "password" },
      { kind: "oidc", providerId: "kc", label: "Keycloak" },
    ]);
  });
  it("none", () => {
    expect(infoFromServer({ password: false, mfa_required: false, oidc: [] }).methods).toEqual([]);
  });
});

describe("waitForBrowserReauth", () => {
  const popup = { closed: false } as { closed: boolean };
  beforeEach(() => {
    popup.closed = false;
    vi.spyOn(window, "open").mockReturnValue(popup as Window);
  });
  afterEach(() => vi.restoreAllMocks());

  const post = (data: unknown, origin = "https://chat.example") =>
    window.dispatchEvent(new MessageEvent("message", { data, origin }));

  it("resolves on a success message from the server origin", async () => {
    const p = waitForBrowserReauth("https://idp/auth");
    post({ type: "oidc-reauth-callback", success: true, expires_at: "x" });
    await expect(p).resolves.toBeUndefined();
  });

  it("rejects with the error code on failure", async () => {
    const p = waitForBrowserReauth("https://idp/auth");
    post({ type: "oidc-reauth-callback", success: false, error_code: "REAUTH_STALE" });
    await expect(p).rejects.toMatchObject({ code: "REAUTH_STALE" });
  });

  it("ignores messages from another origin", async () => {
    const p = waitForBrowserReauth("https://idp/auth");
    let settled = false;
    p.then(
      () => (settled = true),
      () => (settled = true),
    );
    post({ type: "oidc-reauth-callback", success: true }, "https://evil.example");
    await new Promise((r) => setTimeout(r, 10));
    expect(settled).toBe(false);
    post({ type: "oidc-reauth-callback", success: true });
    await expect(p).resolves.toBeUndefined();
  });

  it("rejects as cancelled when the popup is blocked", async () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    await expect(waitForBrowserReauth("https://idp/auth")).rejects.toMatchObject({
      code: "REAUTH_CANCELLED",
    });
  });
});

describe("reauthErrorText", () => {
  it("maps codes and statuses without showing server text", () => {
    expect(reauthErrorText(new HttpError(401, "secret server text", "INVALID_CREDENTIALS"))).toBe(
      "That password or code is not correct.",
    );
    expect(reauthErrorText(new HttpError(0, "x", "REAUTH_STALE"))).toContain("too old");
    expect(reauthErrorText(new HttpError(429, "slow"))).toContain("Too many attempts");
    expect(reauthErrorText(new HttpError(403, "no"))).toContain("not allowed");
    expect(reauthErrorText(new HttpError(503, "down"))).toContain("cannot do this");
    expect(reauthErrorText(new Error("boom"))).toBe("Could not confirm. Please try again.");
  });
});
