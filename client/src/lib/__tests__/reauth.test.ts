import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("@/lib/tauri", async () => {
  // The real class, so the shared errorText() recognises it.
  const { HttpError } = await vi.importActual<typeof import("@/lib/tauri/common")>(
    "@/lib/tauri/common",
  );
  return {
    HttpError,
    getServerUrl: () => "https://chat.example",
    isTauri: () => false,
    getReauthMethods: vi.fn(),
    startOidcReauth: vi.fn(),
  };
});

import { HttpError } from "@/lib/tauri";
import {
  infoFromServer,
  waitForBrowserReauth,
  reauthErrorText,
  isRejectedCode,
  defaultReauthStrategy,
} from "../reauth";
import { errorText } from "@/i18n/errors";
import { LOGIN_ERRORS } from "@/lib/authErrors";

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
  type FakePopup = { closed: boolean; location: { href: string }; close: () => void };
  let popup: FakePopup;
  beforeEach(() => {
    popup = { closed: false, location: { href: "" }, close: vi.fn() };
  });
  afterEach(() => vi.useRealTimers());

  const post = (data: unknown, origin = "https://chat.example", source: unknown = popup) =>
    window.dispatchEvent(
      new MessageEvent("message", { data, origin, source: source as MessageEventSource }),
    );
  const start = (signal = new AbortController().signal, ms?: number) =>
    waitForBrowserReauth(popup as unknown as Window, "https://idp/auth", signal, ms);
  const ok = { type: "oidc-reauth-callback", success: true, error_code: null, expires_at: null };

  it("navigates the placeholder popup to the provider url", () => {
    void start().catch(() => {});
    expect(popup.location.href).toBe("https://idp/auth");
  });

  it("resolves on a success message from the server origin and popup", async () => {
    const p = start();
    post(ok);
    await expect(p).resolves.toBeUndefined();
  });

  it("rejects with the error code on failure", async () => {
    const p = start();
    post({ ...ok, success: false, error_code: "REAUTH_STALE" });
    await expect(p).rejects.toMatchObject({ code: "REAUTH_STALE" });
  });

  it("ignores messages from another origin", async () => {
    const p = start();
    post(ok, "https://evil.example");
    post(ok);
    await expect(p).resolves.toBeUndefined();
  });

  it("ignores messages from another window", async () => {
    const p = start();
    let settled = false;
    p.then(
      () => (settled = true),
      () => (settled = true),
    );
    post(ok, "https://chat.example", {});
    await new Promise((r) => setTimeout(r, 10));
    expect(settled).toBe(false);
    post(ok);
    await p;
  });

  it("ignores messages missing the nullable fields", async () => {
    const p = start();
    post({ type: "oidc-reauth-callback", success: true });
    post(ok);
    await expect(p).resolves.toBeUndefined();
  });

  it("closes the popup when the result arrives", async () => {
    const p = start();
    post(ok);
    await p;
    expect(popup.close).toHaveBeenCalled();
  });

  it("cleans up when navigating the popup throws", async () => {
    const throwing = {
      closed: false,
      close: vi.fn(),
      location: {
        set href(_v: string) {
          throw new Error("blocked");
        },
      },
    };
    const remove = vi.spyOn(window, "removeEventListener");
    await expect(
      waitForBrowserReauth(throwing as unknown as Window, "u", new AbortController().signal),
    ).rejects.toMatchObject({ code: "REAUTH_POPUP_BLOCKED" });
    expect(remove).toHaveBeenCalledWith("message", expect.any(Function));
    expect(throwing.close).toHaveBeenCalled();
    remove.mockRestore();
  });

  it("ignores malformed messages", async () => {
    const p = start();
    post({ ...ok, success: "yes" });
    post({ ...ok, type: "oidc-reauth-callback-x" });
    post({ ...ok, error_code: 5 });
    post(ok);
    await expect(p).resolves.toBeUndefined();
  });

  it("aborting stops listening, closes the popup and never resolves later", async () => {
    const ac = new AbortController();
    const p = start(ac.signal);
    ac.abort();
    await expect(p).rejects.toMatchObject({ name: "AbortError" });
    expect(popup.close).toHaveBeenCalled();
    post(ok); // late callback: nothing happens
  });

  it("times out after the ceiling", async () => {
    vi.useFakeTimers();
    const p = start(new AbortController().signal, 1000);
    const assertion = expect(p).rejects.toMatchObject({ code: "REAUTH_TIMEOUT" });
    await vi.advanceTimersByTimeAsync(1100);
    await assertion;
    expect(popup.close).toHaveBeenCalled();
  });

  it("rejects as cancelled when the popup is closed", async () => {
    vi.useFakeTimers();
    const p = start();
    const assertion = expect(p).rejects.toMatchObject({ code: "REAUTH_CANCELLED" });
    popup.closed = true;
    await vi.advanceTimersByTimeAsync(600);
    await assertion;
  });
});

describe("defaultReauthStrategy.reauthWithOidc", () => {
  afterEach(() => vi.restoreAllMocks());

  it("starts nothing for an already cancelled attempt", async () => {
    const open = vi.spyOn(window, "open");
    const { startOidcReauth } = await import("@/lib/tauri");
    (startOidcReauth as ReturnType<typeof vi.fn>).mockClear();
    const ac = new AbortController();
    ac.abort();
    await expect(
      defaultReauthStrategy.reauthWithOidc("kc", { mfaCode: "123456", signal: ac.signal }),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(startOidcReauth).not.toHaveBeenCalled();
    expect(open).not.toHaveBeenCalled();
  });

  it("fails before any request when the popup is blocked", async () => {
    vi.spyOn(window, "open").mockReturnValue(null);
    const { startOidcReauth } = await import("@/lib/tauri");
    (startOidcReauth as ReturnType<typeof vi.fn>).mockClear();
    await expect(
      defaultReauthStrategy.reauthWithOidc("kc", { signal: new AbortController().signal }),
    ).rejects.toMatchObject({ code: "REAUTH_POPUP_BLOCKED" });
    expect(startOidcReauth).not.toHaveBeenCalled();
  });

  it("opens the placeholder synchronously and closes it when authorize fails", async () => {
    const popup = { closed: false, close: vi.fn(), location: { href: "" } };
    const open = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    const { startOidcReauth } = await import("@/lib/tauri");
    (startOidcReauth as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new HttpError(401, "x", "INVALID_CREDENTIALS"),
    );
    const p = defaultReauthStrategy.reauthWithOidc("kc", {
      signal: new AbortController().signal,
    });
    expect(open).toHaveBeenCalledTimes(1); // before any await
    await expect(p).rejects.toMatchObject({ code: "INVALID_CREDENTIALS" });
    expect(popup.close).toHaveBeenCalled();
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
    expect(reauthErrorText(new HttpError(503, "down", "SERVICE_UNAVAILABLE"))).toContain(
      "cannot do this",
    );
    expect(reauthErrorText(new HttpError(503, "x", "service_unavailable"))).toContain(
      "cannot do this",
    );
    expect(reauthErrorText(new HttpError(0, "x", "invalid_credentials"))).toContain(
      "not correct",
    );
    expect(reauthErrorText(new HttpError(0, "x", "OIDC_PROVIDER_ERROR"))).toContain(
      "provider reported a problem",
    );
    expect(reauthErrorText(new Error("boom"))).toBe("Could not confirm. Please try again.");
  });

  it("goes through the shared errorText mapping (R12)", () => {
    // Common cases come from errorText(), not from a second mapping.
    expect(reauthErrorText(new TypeError("fetch failed"))).toBe(
      "Can't reach the server. Check your connection.",
    );
    expect(reauthErrorText(new HttpError(500, "boom"))).toBe(
      "Something went wrong on the server. Please try again in a moment.",
    );
    // A wrapped failure is unwrapped the same way.
    expect(
      reauthErrorText(new Error("wrapped", { cause: new HttpError(0, "x", "REAUTH_STALE") })),
    ).toContain("too old");
  });

  it("sign-in words a rejected authenticator code the same way (C15)", () => {
    expect(errorText(new HttpError(401, "x", "INVALID_MFA"), "admin.reauth.error.generic", LOGIN_ERRORS)).toBe(
      "That code didn't work. If you just used it, wait for the next code and try again.",
    );
  });

  it("normalises lowercase codes of a wrapped error too", () => {
    const wrapped = new Error("wrapped", {
      cause: new HttpError(401, "x", "invalid_credentials"),
    });
    expect(reauthErrorText(wrapped)).toBe("That password or code is not correct.");
    expect(isRejectedCode(wrapped)).toBe(true);
    const deeper = new Error("outer", { cause: new Error("inner", { cause: new HttpError(0, "x", "reauth_stale") }) });
    expect(reauthErrorText(deeper)).toContain("too old");
    expect(isRejectedCode(new Error("plain"))).toBe(false);
  });

  it("maps the server's wrong-code codes like a wrong password", () => {
    expect(reauthErrorText(new HttpError(401, "x", "INVALID_MFA"))).toBe(
      "That password or code is not correct.",
    );
    expect(reauthErrorText(new HttpError(401, "x", "invalid_mfa_code"))).toBe(
      "That password or code is not correct.",
    );
  });
});
