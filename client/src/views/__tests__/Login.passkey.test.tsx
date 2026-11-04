// Passkey sign-in on the login page (LP Task 12, C29).
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const passkeySignInMock = vi.hoisted(() => vi.fn());
const tauriMode = vi.hoisted(() => ({ on: false }));
const conditional = vi.hoisted(() => ({ available: true, gate: null as Promise<void> | null }));
const signalUnknownMock = vi.hoisted(() => vi.fn());

const pollMock = vi.hoisted(() => vi.fn());
const cancelMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", async () => {
  const passkeys = await vi.importActual<typeof import("@/lib/tauri/passkeys")>("@/lib/tauri/passkeys");
  return {
    // C27: the password answers a sign-in step, never a session.
    passwordLogin: vi.fn(),
    createQrAttempt: vi.fn(),
    pollAttempt: pollMock,
    cancelAttempt: cancelMock,
    collectAttempt: vi.fn(),
    attemptTotp: vi.fn(),
    fetchServerSettings: vi.fn().mockResolvedValue(null),
    getDefaultServerUrl: vi.fn().mockResolvedValue(null),
    oidcAuthorize: vi.fn(),
    passkeySignIn: passkeySignInMock,
    PasskeyError: passkeys.PasskeyError,
  };
});
vi.mock("@/lib/tauri/common", async () => {
  const actual = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { ...actual, isTauri: () => tauriMode.on };
});
vi.mock("@/lib/webauthn", async () => {
  const actual = await vi.importActual<typeof import("@/lib/webauthn")>("@/lib/webauthn");
  return {
    ...actual,
    isWebAuthnAvailable: () => true,
    isConditionalMediationAvailable: () =>
      (conditional.gate ?? Promise.resolve()).then(() => conditional.available),
    signalUnknown: signalUnknownMock,
  };
});

import Login from "../Login";
import { PasskeyError } from "@/lib/tauri/passkeys";
import { setAuthState } from "@/stores/auth";
import { leaveFlow } from "@/stores/loginFlow";

/** A sign-in that never settles until aborted. */
function pendingUntilAbort() {
  passkeySignInMock.mockImplementation(
    (_url: string, o: { signal: AbortSignal }) =>
      new Promise((_, reject) => {
        o.signal.addEventListener("abort", () => reject(new PasskeyError("ABORTED")));
      }),
  );
}

async function renderLogin() {
  const r = render(() => (
    <Router>
      <Route path="/" component={Login} />
    </Router>
  ));
  await screen.findByPlaceholderText("Enter your username");
  return r;
}

beforeEach(() => {
  tauriMode.on = false;
  conditional.available = true;
  conditional.gate = null;
});

/** A promise and its resolve function. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

afterEach(async () => {
  cleanup();
  await leaveFlow();
  vi.useRealTimers();
  passkeySignInMock.mockReset();
  signalUnknownMock.mockReset();
  pollMock.mockReset();
  cancelMock.mockReset();
  setAuthState({ error: null, isLoading: false });
});

describe("Login with a passkey", () => {
  it("passkey button is rendered before the password form", async () => {
    pendingUntilAbort();
    await renderLogin();
    const button = screen.getByRole("button", { name: "Sign in with passkey" });
    const username = screen.getByPlaceholderText("Enter your username");
    expect(button.compareDocumentPosition(username) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('username input has autocomplete "username webauthn"', async () => {
    pendingUntilAbort();
    await renderLogin();
    expect(screen.getByPlaceholderText("Enter your username").getAttribute("autocomplete")).toBe(
      "username webauthn",
    );
  });

  it("conditional request starts when available and is aborted on unmount", async () => {
    pendingUntilAbort();
    const r = await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledOnce());
    const o = passkeySignInMock.mock.calls[0][1] as { mediation: string; signal: AbortSignal };
    expect(o.mediation).toBe("conditional");
    r.unmount();
    expect(o.signal.aborted).toBe(true);
  });

  it("no conditional request without conditional mediation", async () => {
    conditional.available = false;
    pendingUntilAbort();
    await renderLogin();
    await new Promise((res) => setTimeout(res, 20));
    expect(passkeySignInMock).not.toHaveBeenCalled();
  });

  it("conditional ceremony restarts before 300 s", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    const first = passkeySignInMock.mock.calls[0][1] as { signal: AbortSignal };
    await vi.advanceTimersByTimeAsync(270_000);
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
    expect(first.signal.aborted).toBe(true);
  });

  it("button aborts the conditional request and runs a modal one with the username hint", async () => {
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledOnce());
    const conditionalCall = passkeySignInMock.mock.calls[0][1] as { signal: AbortSignal };
    fireEvent.input(screen.getByPlaceholderText("Enter your username"), { target: { value: "fabian" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
    expect(conditionalCall.signal.aborted).toBe(true);
    const modal = passkeySignInMock.mock.calls[1][1] as { mediation: string; username?: string };
    expect(modal.mediation).toBe("optional");
    expect(modal.username).toBe("fabian");
  });

  it("conditional autofill restarts after a failed attempt", async () => {
    // Review Argon M4.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    passkeySignInMock.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    pendingUntilAbort();
    passkeySignInMock.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    await vi.advanceTimersByTimeAsync(2_500);
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
  });

  it("autofill comes back after a failed password sign-in", async () => {
    // Earlier review m2: the password submit stopped autofill for good.
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.passwordLogin).mockRejectedValueOnce(new Error("INVALID_CREDENTIALS"));
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    fireEvent.input(screen.getByPlaceholderText("Enter your username"), { target: { value: "fabian" } });
    fireEvent.input(screen.getByPlaceholderText("Enter your password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Login" }));
    await waitFor(() => expect(tauri.passwordLogin).toHaveBeenCalled());
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
    const again = passkeySignInMock.mock.calls[1][1] as { mediation: string };
    expect(again.mediation).toBe("conditional");
  });

  it("a server change restarts autofill once, after a pause, with a fresh backoff", async () => {
    // Review round 2 (Argon B R2).
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const tauri = await import("@/lib/tauri");
    const override = deferred<string | null>();
    vi.mocked(tauri.getDefaultServerUrl).mockReturnValueOnce(override.promise);
    passkeySignInMock.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    passkeySignInMock.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    pendingUntilAbort();
    passkeySignInMock.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    passkeySignInMock.mockRejectedValueOnce(new PasskeyError("PASSKEY_CANCELLED"));
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    await vi.advanceTimersByTimeAsync(2_100); // first retry after 2 s; the next pause would be 8 s
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
    override.resolve("https://other.example.com");
    await vi.advanceTimersByTimeAsync(200);
    expect(passkeySignInMock).toHaveBeenCalledTimes(2); // not at once
    await vi.advanceTimersByTimeAsync(500);
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(3));
    expect(passkeySignInMock.mock.calls[2][0]).toBe("https://other.example.com");
    // The new server starts with the first pause again (2 s, not 8 s).
    await vi.advanceTimersByTimeAsync(2_100);
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(4));
  });

  it("a password sign-in cancels a pending autofill restart", async () => {
    // Review round 3 (Fable): the server-change timer must not start an
    // autofill ceremony while the password sign-in runs.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const tauri = await import("@/lib/tauri");
    const override = deferred<string | null>();
    vi.mocked(tauri.getDefaultServerUrl).mockReturnValueOnce(override.promise);
    vi.mocked(tauri.passwordLogin).mockReturnValueOnce(new Promise(() => {}));
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    override.resolve("https://other.example.com");
    await vi.advanceTimersByTimeAsync(100);
    fireEvent.input(screen.getByPlaceholderText("Enter your username"), { target: { value: "fabian" } });
    fireEvent.input(screen.getByPlaceholderText("Enter your password"), { target: { value: "secret-pw" } });
    fireEvent.click(screen.getByRole("button", { name: "Login" }));
    await waitFor(() => expect(tauri.passwordLogin).toHaveBeenCalled());
    await vi.advanceTimersByTimeAsync(1_000);
    expect(passkeySignInMock).toHaveBeenCalledTimes(1);
  });

  it("an autofill start waiting for availability never runs beside the button's ceremony", async () => {
    // Review round 2 (Argon B R2): a generation token across the await.
    const gate = deferred<void>();
    conditional.gate = gate.promise;
    pendingUntilAbort();
    await renderLogin();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    gate.resolve();
    await new Promise((res) => setTimeout(res, 20));
    expect(passkeySignInMock).toHaveBeenCalledTimes(1);
    expect((passkeySignInMock.mock.calls[0][1] as { mediation: string }).mediation).toBe("optional");
  });

  it("autofill stops while the confirmation step replaces the form and returns with it", async () => {
    // L2a: a password sign-in is a step (C27); the form, and with it autofill,
    // comes back when the user leaves the step.
    const tauri = await import("@/lib/tauri");
    pollMock.mockReturnValue(new Promise(() => {}));
    cancelMock.mockResolvedValue(undefined);
    vi.mocked(tauri.passwordLogin).mockResolvedValueOnce({
      status: "confirmation_required",
      attempt: {
        attempt_id: "44444444-4444-4444-8444-444444444444",
        kind: "password",
        state: "open",
        expires_at: new Date(Date.now() + 600_000).toISOString(),
        qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
        totp_available: false,
      },
    } as never);
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    const first = passkeySignInMock.mock.calls[0][1] as { signal: AbortSignal };
    fireEvent.input(screen.getByPlaceholderText("Enter your username"), { target: { value: "fabian" } });
    fireEvent.input(screen.getByPlaceholderText("Enter your password"), { target: { value: "secret-pw" } });
    fireEvent.click(screen.getByRole("button", { name: "Login" }));
    await screen.findByText("Open Kaiku on your phone and scan this code.");
    expect(first.signal.aborted).toBe(true);
    expect(passkeySignInMock).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await screen.findByPlaceholderText("Enter your username");
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
    expect((passkeySignInMock.mock.calls[1][1] as { mediation: string }).mediation).toBe("conditional");
  });

  it("use a passkey instead on the phone QR leaves the attempt and runs the button's ceremony", async () => {
    const tauri = await import("@/lib/tauri");
    pollMock.mockReturnValue(new Promise(() => {}));
    cancelMock.mockResolvedValue(undefined);
    vi.mocked(tauri.createQrAttempt).mockResolvedValueOnce({
      attempt_id: "55555555-5555-4555-8555-555555555555",
      kind: "qr",
      state: "open",
      expires_at: new Date(Date.now() + 600_000).toISOString(),
      qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
      totp_available: false,
    } as never);
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Sign in with your phone" }));
    fireEvent.click(await screen.findByRole("button", { name: "Use a passkey instead" }));
    await waitFor(() => expect(cancelMock).toHaveBeenCalledWith("55555555-5555-4555-8555-555555555555"));
    await waitFor(() =>
      expect(passkeySignInMock.mock.calls.some(([, o]) => (o as { mediation: string }).mediation === "optional")).toBe(true),
    );
    await new Promise((res) => setTimeout(res, 20));
    // Only the button's ceremony runs: no autofill beside it.
    const calls = passkeySignInMock.mock.calls;
    const last = calls[calls.length - 1][1] as { mediation: string; signal: AbortSignal };
    expect(last.mediation).toBe("optional");
    expect(last.signal.aborted).toBe(false);
  });

  const ssoSettings = {
    oidc_enabled: true,
    oidc_providers: [{ slug: "kc", display_name: "Keycloak", icon_hint: null }],
    auth_methods: { local: true },
    registration_policy: "closed",
  };
  /** A popup the SSO flow can point at the provider. */
  function fakePopup() {
    return vi
      .spyOn(window, "open")
      .mockReturnValue({ closed: false, close: vi.fn(), location: { href: "" } } as unknown as Window);
  }
  const conditionalCalls = () =>
    passkeySignInMock.mock.calls.filter(([, o]) => (o as { mediation: string }).mediation === "conditional").length;

  it("a modal sign-in aborted by a newer SSO choice does not bring autofill back (Codex r3)", async () => {
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(ssoSettings as never);
    vi.mocked(tauri.oidcAuthorize).mockReturnValue(new Promise(() => {}));
    const open = fakePopup();
    try {
      pendingUntilAbort();
      await renderLogin();
      await waitFor(() => expect(conditionalCalls()).toBe(1));
      fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
      await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
      fireEvent.click(await screen.findByRole("button", { name: /Continue with Keycloak/ }));
      await waitFor(() => expect(tauri.oidcAuthorize).toHaveBeenCalled());
      await new Promise((r) => setTimeout(r, 30));
      // Only the first autofill: nothing runs beside the SSO sign-in.
      expect(conditionalCalls()).toBe(1);
    } finally {
      open.mockRestore();
      vi.mocked(tauri.fetchServerSettings).mockResolvedValue(null as never);
    }
  });

  it("a server change during an SSO sign-in leaves the popup and starts no autofill (Fable r2 follow-up a)", async () => {
    const tauri = await import("@/lib/tauri");
    const override = deferred<string | null>();
    vi.mocked(tauri.getDefaultServerUrl).mockReturnValueOnce(override.promise);
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(ssoSettings as never);
    vi.mocked(tauri.oidcAuthorize).mockReturnValue(new Promise(() => {}));
    const close = vi.fn();
    const open = vi
      .spyOn(window, "open")
      .mockReturnValue({ closed: false, close, location: { href: "" } } as unknown as Window);
    try {
      pendingUntilAbort();
      await renderLogin();
      await waitFor(() => expect(conditionalCalls()).toBe(1));
      fireEvent.click(await screen.findByRole("button", { name: /Continue with Keycloak/ }));
      await waitFor(() => expect(tauri.oidcAuthorize).toHaveBeenCalled());
      override.resolve("https://other.example.com");
      await new Promise((r) => setTimeout(r, 700));
      expect(close).not.toHaveBeenCalled();
      expect(conditionalCalls()).toBe(1);
    } finally {
      open.mockRestore();
      vi.mocked(tauri.fetchServerSettings).mockResolvedValue(null as never);
    }
  });

  it("a failed phone start that stays on the form brings autofill back (Fable R4)", async () => {
    const tauri = await import("@/lib/tauri");
    const { HttpError } = await import("@/lib/tauri/common");
    vi.mocked(tauri.createQrAttempt).mockRejectedValueOnce(new HttpError(503, "x", "SERVICE_UNAVAILABLE"));
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(conditionalCalls()).toBe(1));
    fireEvent.click(screen.getByRole("button", { name: "Sign in with your phone" }));
    await waitFor(() => expect(tauri.createQrAttempt).toHaveBeenCalled());
    await waitFor(() => expect(conditionalCalls()).toBe(2));
  });

  it("a failed SSO start that stays on the form brings autofill back (Fable R4)", async () => {
    const tauri = await import("@/lib/tauri");
    vi.mocked(tauri.fetchServerSettings).mockResolvedValue(ssoSettings as never);
    vi.mocked(tauri.oidcAuthorize).mockRejectedValueOnce(new Error("OIDC_FAILED"));
    const open = fakePopup();
    try {
      pendingUntilAbort();
      await renderLogin();
      await waitFor(() => expect(conditionalCalls()).toBe(1));
      fireEvent.click(await screen.findByRole("button", { name: /Continue with Keycloak/ }));
      await waitFor(() => expect(tauri.oidcAuthorize).toHaveBeenCalled());
      await waitFor(() => expect(conditionalCalls()).toBe(2));
    } finally {
      open.mockRestore();
      vi.mocked(tauri.fetchServerSettings).mockResolvedValue(null as never);
    }
  });

  it("PASSKEY_UNKNOWN shows the removed message", async () => {
    conditional.available = false;
    passkeySignInMock.mockRejectedValue(
      new PasskeyError("PASSKEY_UNKNOWN", "Y3JlZA", "chat.example.com"),
    );
    await renderLogin();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
    await screen.findByText("This passkey was removed from Kaiku. Sign in another way.");
    // The credential is signalled where the server answered (passkeySignIn).
  });

  it("PASSKEY_PENDING shows the ready time and signals nothing", async () => {
    conditional.available = false;
    passkeySignInMock.mockRejectedValue(
      new PasskeyError("PASSKEY_PENDING", "Y3JlZA", "chat.example.com", "2026-11-05T10:00:00Z"),
    );
    await renderLogin();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
    await screen.findByText(/This passkey is ready at/);
    expect(signalUnknownMock).not.toHaveBeenCalled();
  });

  it("desktop on Linux shows LP's Firefox hint while the browser runs", async () => {
    tauriMode.on = true;
    const ua = vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (X11; Linux x86_64) Kaiku");
    pendingUntilAbort();
    await renderLogin();
    expect(screen.queryByText(/No phone option in Firefox\?/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
    expect(await screen.findByText(/No phone option in Firefox\?/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    ua.mockRestore();
  });

  it("desktop invokes passkey_sign_in and shows the browser hint", async () => {
    tauriMode.on = true;
    pendingUntilAbort();
    await renderLogin();
    expect(passkeySignInMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with passkey" }));
    await screen.findByText(/Continue in your browser/);
    const o = passkeySignInMock.mock.calls[0][1] as { signal: AbortSignal };
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(o.signal.aborted).toBe(true);
  });
});
