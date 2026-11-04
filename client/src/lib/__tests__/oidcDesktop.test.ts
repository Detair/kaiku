// Desktop OIDC flows can be cancelled from the webview (Argon review part 7
// #2); a pre-L1b token answer surfaces as SERVER_OUTDATED (Fable M1); the
// request popup only loads a safe provider page (Argon part 5 #5).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const tauriMode = vi.hoisted(() => ({ on: true }));
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (orig) => ({
  ...(await orig<object>()),
  isTauri: () => tauriMode.on,
  invoke,
}));

import { oidcAuthorize, nativeAuthError } from "../tauri/auth";
import { requestOidcAuthorize } from "../tauri/accountRequests";
import { requestOidcCode, safeProviderUrl } from "../requestOidc";
import { errorText } from "@/i18n/errors";
import { LOGIN_ERRORS, REGISTER_ERRORS } from "../authErrors";

beforeEach(() => {
  tauriMode.on = true;
  invoke.mockReset();
});
afterEach(() => vi.restoreAllMocks());

describe("desktop OIDC cancel", () => {
  it("oidcAuthorize names an attempt and an abort cancels it in Rust", async () => {
    let finish!: (v: unknown) => void;
    invoke.mockImplementation((cmd: string) =>
      cmd === "oidc_authorize" ? new Promise((r) => (finish = r)) : Promise.resolve(undefined),
    );
    const ctl = new AbortController();
    const p = oidcAuthorize("https://chat.example.com", "kc", ctl.signal);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("oidc_authorize", expect.anything()));
    const args = invoke.mock.calls[0][1] as { attemptId: string };
    expect(args.attemptId).toMatch(/^[0-9a-f-]{36}$/);
    ctl.abort();
    expect(invoke).toHaveBeenCalledWith("oidc_cancel", { attemptId: args.attemptId });
    finish({
      status: "confirmation_required",
      attempt: { attempt_id: "44444444-4444-4444-8444-444444444444", kind: "oidc", state: "open", expires_at: "", qr: null, totp_available: false },
    });
    await p;
  });

  it("the request factor is cancellable too", async () => {
    let fail!: (e: unknown) => void;
    invoke.mockImplementation((cmd: string) =>
      cmd === "request_oidc_factor" ? new Promise((_r, rej) => (fail = rej)) : Promise.resolve(undefined),
    );
    const ctl = new AbortController();
    const p = requestOidcAuthorize("https://chat.example.com", "kc", ctl.signal);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("request_oidc_factor", expect.anything()));
    const { attemptId } = invoke.mock.calls[0][1] as { attemptId: string };
    ctl.abort();
    expect(invoke).toHaveBeenCalledWith("oidc_cancel", { attemptId });
    fail("OIDC_CANCELLED");
    await expect(p).rejects.toMatchObject({ code: "OIDC_CANCELLED" });
  });

  it("an aborted signal starts nothing", async () => {
    const ctl = new AbortController();
    ctl.abort();
    await expect(oidcAuthorize("https://chat.example.com", "kc", ctl.signal)).rejects.toMatchObject({
      code: "OIDC_CANCELLED",
    });
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("SERVER_OUTDATED", () => {
  it("is worded for sign-in and registration", () => {
    const err = nativeAuthError("SERVER_OUTDATED");
    const text = "This server is older than this app and cannot sign you in yet. Ask your admin to update it.";
    expect(errorText(err, "auth.login.failed", LOGIN_ERRORS)).toBe(text);
    expect(errorText(err, "auth.register.failed", REGISTER_ERRORS)).toBe(text);
  });
});

describe("request OIDC popup", () => {
  it("only loads https provider pages (or http on loopback)", () => {
    expect(safeProviderUrl("https://idp.example.com/auth?x=1", "https://chat.example.com")).toBe(
      "https://idp.example.com/auth?x=1",
    );
    expect(safeProviderUrl("http://localhost:8080/a", "https://chat.example.com")).toBe("http://localhost:8080/a");
    expect(safeProviderUrl("javascript:alert(1)", "https://chat.example.com")).toBeNull();
    expect(safeProviderUrl("data:text/html,x", "https://chat.example.com")).toBeNull();
    expect(safeProviderUrl("http://idp.example.com/a", "https://chat.example.com")).toBeNull();
    expect(safeProviderUrl("https://u:p@idp.example.com/a", "https://chat.example.com")).toBeNull();
  });

  it("refuses a javascript: URL and closes the popup", async () => {
    tauriMode.on = false;
    const popup = { closed: false, close: vi.fn(), location: { href: "" } };
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ url: "javascript:alert(1)" }), { status: 200, headers: { "Content-Type": "application/json" } }),
    );
    vi.stubGlobal("fetch", fetchMock);
    await expect(requestOidcCode("https://chat.example.com", "kc")).rejects.toThrow("OIDC_FAILED");
    expect(popup.location.href).toBe("");
    expect(popup.close).toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  it("closes the popup once the code arrives", async () => {
    tauriMode.on = false;
    const popup = { closed: false, close: vi.fn(), location: { href: "" } };
    vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        new Response(JSON.stringify({ url: "https://idp.example.com/auth" }), { status: 200, headers: { "Content-Type": "application/json" } }),
      ),
    );
    const p = requestOidcCode("https://chat.example.com", "kc");
    await vi.waitFor(() => expect(popup.location.href).toBe("https://idp.example.com/auth"));
    window.dispatchEvent(
      new MessageEvent("message", {
        origin: "https://chat.example.com",
        source: popup as unknown as MessageEventSource,
        data: { type: "oidc-request-callback", oidc_code: "one-time" },
      }),
    );
    await expect(p).resolves.toBe("one-time");
    expect(popup.close).toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
});
