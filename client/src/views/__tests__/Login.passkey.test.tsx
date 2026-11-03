// Passkey sign-in on the login page (LP Task 12, C29).
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const passkeySignInMock = vi.hoisted(() => vi.fn());
const tauriMode = vi.hoisted(() => ({ on: false }));
const conditional = vi.hoisted(() => ({ available: true }));
const signalUnknownMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", async () => {
  const passkeys = await vi.importActual<typeof import("@/lib/tauri/passkeys")>("@/lib/tauri/passkeys");
  return {
    login: vi.fn(),
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
    isConditionalMediationAvailable: () => Promise.resolve(conditional.available),
    signalUnknown: signalUnknownMock,
  };
});

import Login from "../Login";
import { PasskeyError } from "@/lib/tauri/passkeys";
import { setAuthState } from "@/stores/auth";

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
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  passkeySignInMock.mockReset();
  signalUnknownMock.mockReset();
  setAuthState({ error: null, mfaRequired: false, isLoading: false });
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
    vi.mocked(tauri.login).mockRejectedValueOnce(new Error("INVALID_CREDENTIALS"));
    pendingUntilAbort();
    await renderLogin();
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(1));
    fireEvent.input(screen.getByPlaceholderText("Enter your username"), { target: { value: "fabian" } });
    fireEvent.input(screen.getByPlaceholderText("Enter your password"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Login" }));
    await waitFor(() => expect(tauri.login).toHaveBeenCalled());
    await waitFor(() => expect(passkeySignInMock).toHaveBeenCalledTimes(2));
    const again = passkeySignInMock.mock.calls[1][1] as { mediation: string };
    expect(again.mediation).toBe("conditional");
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
