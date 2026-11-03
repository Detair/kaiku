import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const fetchMock = vi.hoisted(() => {
  localStorage.setItem("kaiku:skip-session-restore", "1");
  const f = vi.fn();
  globalThis.fetch = f as unknown as typeof fetch;
  return f;
});

const order = vi.hoisted(() => [] as string[]);
vi.mock("@/stores/websocket", async (orig) => ({
  ...(await orig<object>()),
  disconnect: vi.fn(async () => {
    order.push("ws-stop");
  }),
}));

const passkeySignIn = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/passkeys", async (orig) => ({
  ...(await orig<object>()),
  passkeySignIn,
  canUsePasskeys: () => true,
}));

import FullSessionRequiredDialog, { showFullSessionRequired } from "../FullSessionRequiredDialog";
import { resetMeSecurity } from "@/stores/security";
import { browserState, FULL_SESSION_REQUIRED_EVENT, httpRequest } from "@/lib/tauri/common";
import { replaceSessionWithPasskey, setAuthState } from "@/stores/auth";

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => {
  cleanup();
  fetchMock.mockReset();
  passkeySignIn.mockReset();
  setAuthState({ user: null });
  vi.useRealTimers();
});

describe("FullSessionRequiredDialog", () => {
  it("FULL_SESSION_REQUIRED opens the dialog once", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const seen = vi.fn();
    window.addEventListener(FULL_SESSION_REQUIRED_EVENT, seen);
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "at";
    fetchMock.mockImplementation(async () => json(403, { error: "FULL_SESSION_REQUIRED", message: "x" }));
    render(() => <FullSessionRequiredDialog />);
    await expect(httpRequest("DELETE", "/api/approval-devices/x")).rejects.toMatchObject({
      code: "FULL_SESSION_REQUIRED",
    });
    await expect(httpRequest("DELETE", "/api/approval-devices/y")).rejects.toMatchObject({ status: 403 });
    expect(seen).toHaveBeenCalledTimes(1);
    expect(screen.getByText("This needs a sign-in with a passkey.")).toBeTruthy();
    expect(screen.getByText("Or do it in the Kaiku app on your phone.")).toBeTruthy();
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(2100);
    await expect(httpRequest("DELETE", "/api/approval-devices/z")).rejects.toBeTruthy();
    expect(seen).toHaveBeenCalledTimes(2);
    window.removeEventListener(FULL_SESSION_REQUIRED_EVENT, seen);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("replaceSessionWithPasskey ends the old session with credentials omit", async () => {
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "old-token";
    const signIn = passkeySignIn.mockImplementation(async () => {
      browserState.accessToken = "new-token";
      return {
        user: { id: "u1", session: { id: "s2", assurance: "full" } },
        setup_required: false,
        password_change_required: false,
      };
    });
    order.length = 0;
    fetchMock.mockImplementation(async (url: string) => {
      if (String(url).endsWith("/auth/logout")) order.push("logout-old");
      return new Response(null, { status: 204 });
    });
    await replaceSessionWithPasskey();
    // The old socket is closed before its session ends, so its SESSION_REVOKED cannot end the new one.
    expect(order.indexOf("ws-stop")).toBeGreaterThanOrEqual(0);
    expect(order.indexOf("ws-stop")).toBeLessThan(order.indexOf("logout-old"));
    expect(signIn).toHaveBeenCalledWith("https://chat.example.com", expect.objectContaining({ mediation: "optional" }));
    const logout = fetchMock.mock.calls.find(([url]) => String(url).endsWith("/auth/logout"));
    expect(logout).toBeTruthy();
    const init = logout![1] as RequestInit;
    expect(init.method).toBe("POST");
    expect(init.credentials).toBe("omit");
    expect((init.headers as Record<string, string>).Authorization).toBe("Bearer old-token");
  });

  it("a newer sign-in makes a running chat-only passkey sign-in stale", async () => {
    // LP's attempt generations also cover the probation banner's sign-in.
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "old-token";
    setAuthState({ user: { id: "u1", username: "a", session: { id: "s1", assurance: "probation" } } as never });
    let finish!: (v: unknown) => void;
    passkeySignIn.mockImplementationOnce(() => new Promise((r) => (finish = r)));
    passkeySignIn.mockImplementationOnce(() => new Promise(() => {}));
    fetchMock.mockImplementation(async () => new Response(null, { status: 204 }));
    const first = replaceSessionWithPasskey();
    await waitFor(() => expect(passkeySignIn).toHaveBeenCalledTimes(1));
    const o = passkeySignIn.mock.calls[0][1] as { signal: AbortSignal; isCurrent: () => boolean };
    void replaceSessionWithPasskey();
    await waitFor(() => expect(passkeySignIn).toHaveBeenCalledTimes(2));
    expect(o.signal.aborted).toBe(true);
    expect(o.isCurrent()).toBe(false);
    finish({ user: { id: "u1", session: { id: "s2", assurance: "full" } }, setup_required: false, password_change_required: false });
    await expect(first).rejects.toMatchObject({ code: "ABORTED" });
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith("/auth/logout"))).toBe(false);
  });

  it("a passkey of another account reloads the app instead of mixing two accounts", async () => {
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "old-token";
    setAuthState({ user: { id: "u1", username: "a", session: { id: "s1", assurance: "probation" } } as never });
    const reload = vi.fn();
    const original = window.location;
    Object.defineProperty(window, "location", { configurable: true, value: { ...original, reload } });
    passkeySignIn.mockImplementation(async () => {
      browserState.accessToken = "new-token";
      return { user: { id: "u2", session: { id: "s9", assurance: "full" } }, setup_required: false, password_change_required: false };
    });
    fetchMock.mockImplementation(async () => new Response(null, { status: 204 }));
    try {
      await replaceSessionWithPasskey();
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
    }
    expect(reload).toHaveBeenCalledTimes(1);
  });
});

describe("FullSessionRequiredDialog passkey offer (Argon review)", () => {
  function overview(passkeys: number) {
    return {
      session: { assurance: "probation", auth_methods: ["qr"] },
      password: null,
      totp: null,
      recovery_codes_left: 0,
      key_devices: 1,
      passkeys,
      may_approve: true,
      bootstrap_open: false,
      recovery_contact: null,
      open_request: null,
      prompts: [],
    };
  }

  it("offers the passkey sign-in only when the account has a passkey", async () => {
    resetMeSecurity();
    browserState.serverUrl = "https://chat.example.com";
    browserState.accessToken = "at";
    fetchMock.mockImplementation(async () => json(200, overview(0)));
    render(() => <FullSessionRequiredDialog />);
    showFullSessionRequired();
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByText("Or do it in the Kaiku app on your phone.")).toBeTruthy());
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByRole("button", { name: "Sign in with a passkey" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    cleanup();

    resetMeSecurity();
    fetchMock.mockImplementation(async () => json(200, overview(1)));
    render(() => <FullSessionRequiredDialog />);
    showFullSessionRequired();
    await waitFor(() => expect(screen.getByRole("button", { name: "Sign in with a passkey" })).toBeTruthy());
    const loaded = fetchMock.mock.calls.some(([url]) => String(url).endsWith("/api/me/security"));
    expect(loaded).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
  });
});
