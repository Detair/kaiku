// One sign-in choice at a time (Codex review M1, Argon 9 #3, 10 #4): a late
// SSO result for a choice the user left is never applied, its attempt is
// cancelled, and its desktop flow is cancelled in Rust.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const tauriMode = vi.hoisted(() => ({ on: true }));
const api = vi.hoisted(() => ({
  oidcAuthorize: vi.fn(),
  createQrAttempt: vi.fn(),
  pollAttempt: vi.fn(),
  cancelAttempt: vi.fn(),
  passwordLogin: vi.fn(),
  register: vi.fn(),
}));

vi.mock("@/lib/tauri", async () => {
  const passkeys = await vi.importActual<typeof import("@/lib/tauri/passkeys")>("@/lib/tauri/passkeys");
  return {
    ...api,
    collectAttempt: vi.fn(),
    attemptTotp: vi.fn(),
    discardAdoptedSession: vi.fn(),
    fetchServerSettings: vi.fn().mockResolvedValue({
      oidc_enabled: true,
      oidc_providers: [{ slug: "kc", display_name: "Keycloak", icon_hint: null }],
      auth_methods: { local: true },
      registration_policy: "open",
    }),
    getDefaultServerUrl: vi.fn().mockResolvedValue(null),
    passkeySignIn: vi.fn(() => new Promise(() => {})),
    PasskeyError: passkeys.PasskeyError,
  };
});
vi.mock("@/lib/tauri/common", async () => {
  const actual = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { ...actual, isTauri: () => tauriMode.on };
});

import Login from "../Login";
import Register from "../Register";
import { leaveFlow, loginFlow } from "@/stores/loginFlow";
import { setAuthState } from "@/stores/auth";

const SSO_ATTEMPT = "66666666-6666-4666-8666-666666666666";
const QR_ATTEMPT = "55555555-5555-4555-8555-555555555555";

function ssoStep() {
  return {
    status: "confirmation_required",
    attempt: {
      attempt_id: SSO_ATTEMPT,
      kind: "oidc",
      state: "open",
      expires_at: new Date(Date.now() + 600_000).toISOString(),
      qr: { payload: "kaiku://login/v1?o=x&t=b", expires_at: "" },
      totp_available: false,
    },
  };
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

beforeEach(() => {
  tauriMode.on = true;
  for (const f of Object.values(api)) f.mockReset();
  api.cancelAttempt.mockResolvedValue(undefined);
  api.pollAttempt.mockReturnValue(new Promise(() => {}));
});

afterEach(async () => {
  cleanup();
  await leaveFlow();
  setAuthState({ error: null, isLoading: false, user: null });
});

async function renderAt(path: string) {
  window.history.pushState({}, "", path);
  render(() => (
    <Router>
      <Route path="/login" component={Login} />
      <Route path="/register" component={Register} />
    </Router>
  ));
}

describe("one sign-in choice at a time", () => {
  it("a late SSO result after choosing the phone is dropped and its attempt cancelled", async () => {
    const sso = deferred<unknown>();
    api.oidcAuthorize.mockReturnValue(sso.promise);
    api.createQrAttempt.mockResolvedValue({
      attempt_id: QR_ATTEMPT,
      kind: "qr",
      state: "open",
      expires_at: new Date(Date.now() + 600_000).toISOString(),
      qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
      totp_available: false,
    });
    await renderAt("/login");
    fireEvent.click(await screen.findByRole("button", { name: /Continue with Keycloak/ }));
    await waitFor(() => expect(api.oidcAuthorize).toHaveBeenCalled());
    const signal = api.oidcAuthorize.mock.calls[0][2] as AbortSignal;
    fireEvent.click(screen.getByRole("button", { name: "Sign in with your phone" }));
    await screen.findByRole("heading", { name: "Sign in with your phone" });
    // The SSO wait was cancelled at once (desktop: oidc_cancel).
    expect(signal.aborted).toBe(true);
    sso.resolve({ mode: "tauri", step: ssoStep() });
    await waitFor(() => expect(api.cancelAttempt).toHaveBeenCalledWith(SSO_ATTEMPT));
    const f = loginFlow();
    expect(f.step).toBe("qr");
    expect(f.step === "qr" && f.attempt.attempt_id).toBe(QR_ATTEMPT);
  });

  it("an SSO result for a page the user left is dropped", async () => {
    const sso = deferred<unknown>();
    api.oidcAuthorize.mockReturnValue(sso.promise);
    await renderAt("/login");
    fireEvent.click(await screen.findByRole("button", { name: /Continue with Keycloak/ }));
    await waitFor(() => expect(api.oidcAuthorize).toHaveBeenCalled());
    const signal = api.oidcAuthorize.mock.calls[0][2] as AbortSignal;
    cleanup();
    expect(signal.aborted).toBe(true);
    sso.resolve({ mode: "tauri", step: ssoStep() });
    await waitFor(() => expect(api.cancelAttempt).toHaveBeenCalledWith(SSO_ATTEMPT));
    expect(loginFlow().step).toBe("form");
  });

  it("Register: a desktop SSO sign-up the user left never navigates or applies", async () => {
    const sso = deferred<unknown>();
    api.oidcAuthorize.mockReturnValue(sso.promise);
    await renderAt("/register");
    fireEvent.click(await screen.findByRole("button", { name: /Keycloak/ }));
    await waitFor(() => expect(api.oidcAuthorize).toHaveBeenCalled());
    cleanup();
    sso.resolve({ mode: "tauri", step: ssoStep() });
    await waitFor(() => expect(api.cancelAttempt).toHaveBeenCalledWith(SSO_ATTEMPT));
    expect(loginFlow().step).toBe("form");
    expect(window.location.pathname).toBe("/register");
  });
});
