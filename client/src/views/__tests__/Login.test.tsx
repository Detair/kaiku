// Same Solid-instance note as i18n/__tests__/locale.dom.test.tsx: components
// render through @solidjs/testing-library with the real store and i18n.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const loginMock = vi.hoisted(() => vi.fn());
const settingsMock = vi.hoisted(() => vi.fn());

const createQrMock = vi.hoisted(() => vi.fn());
const pollMock = vi.hoisted(() => vi.fn());
const cancelMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  passwordLogin: loginMock,
  fetchServerSettings: settingsMock,
  getDefaultServerUrl: vi.fn().mockResolvedValue(null),
  oidcAuthorize: vi.fn(),
  createQrAttempt: createQrMock,
  pollAttempt: pollMock,
  cancelAttempt: cancelMock,
  collectAttempt: vi.fn(),
  attemptTotp: vi.fn(),
}));

import Login from "../Login";
import { HttpError } from "@/lib/tauri/common";
import { setAuthState } from "@/stores/auth";
import { leaveFlow } from "@/stores/loginFlow";

afterEach(async () => {
  cleanup();
  await leaveFlow();
  window.history.pushState({}, "", "/");
  setAuthState({ error: null, isLoading: false });
  createQrMock.mockReset();
  pollMock.mockReset();
  cancelMock.mockReset();
  loginMock.mockReset();
  settingsMock.mockReset();
  settingsMock.mockResolvedValue(null);
});

function serverSettings(registration_policy: string) {
  return {
    oidc_enabled: false,
    oidc_providers: [],
    auth_methods: { local: true },
    registration_policy,
  };
}

async function renderLogin(): Promise<void> {
  render(() => (
    <Router>
      <Route path="/" component={Login} />
    </Router>
  ));
  await screen.findByPlaceholderText("Enter your username");
}

async function submit(): Promise<void> {
  render(() => (
    <Router>
      <Route path="/" component={Login} />
    </Router>
  ));
  fireEvent.input(await screen.findByPlaceholderText("Enter your username"), {
    target: { value: "alice" },
  });
  fireEvent.input(screen.getByPlaceholderText("Enter your password"), {
    target: { value: "wrong" },
  });
  fireEvent.click(screen.getByTestId("login-submit"));
}

describe("Login", () => {
  it("shows the invalid-credentials sentence for INVALID_CREDENTIALS", async () => {
    loginMock.mockRejectedValue(new HttpError(401, "Invalid credentials", "INVALID_CREDENTIALS"));
    await submit();
    await waitFor(() =>
      expect(
        screen.getByText(
          "Wrong username or password, or password sign-in is not available for this account. Use a passkey or 'Sign in with phone', or ask an admin.",
        ),
      ).toBeTruthy(),
    );
  });

  it("TOO_MANY_ATTEMPTS shows the wait from Retry-After", async () => {
    loginMock.mockRejectedValue(new HttpError(429, "slow", "TOO_MANY_ATTEMPTS", {}, 300));
    await submit();
    await waitFor(() => expect(screen.getByText("Too many tries. Try again in 5 minutes.")).toBeTruthy());
  });

  it("password login shows the confirmation QR on confirmation_required", async () => {
    pollMock.mockReturnValue(new Promise(() => {}));
    loginMock.mockResolvedValue({
      status: "confirmation_required",
      attempt: {
        attempt_id: "44444444-4444-4444-8444-444444444444",
        kind: "password",
        state: "open",
        expires_at: new Date(Date.now() + 600_000).toISOString(),
        qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
        totp_available: true,
      },
    });
    await submit();
    await waitFor(() => expect(screen.getByText("Open Kaiku on your phone and scan this code.")).toBeTruthy());
    expect(screen.getByRole("button", { name: "Phone not at hand?" })).toBeTruthy();
    expect(loginMock).toHaveBeenCalledWith(expect.any(String), "alice", "wrong");
  });

  it("orders passkey, phone, SSO and password, with the recovery links", async () => {
    await renderLogin();
    const phone = screen.getByRole("button", { name: "Sign in with your phone" });
    const form = screen.getByTestId("login-submit");
    expect(phone.compareDocumentPosition(form) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(document.querySelector('a[href="/recover"]')?.textContent).toBe("I lost my phone / forgot my password");
    expect(document.querySelector('a[href="/secure-account"]')?.textContent).toBe("Secure my account");
  });

  it("forgot-password link and routes are gone", async () => {
    await renderLogin();
    expect(document.querySelector('a[href="/forgot-password"]')).toBeNull();
    const views = import.meta.glob("../*.tsx");
    expect(Object.keys(views).some((k) => /ForgotPassword|ResetPassword/.test(k))).toBe(false);
  });

  it("no typed sign-in code is offered", async () => {
    pollMock.mockReturnValue(new Promise(() => {}));
    createQrMock.mockResolvedValue({
      attempt_id: "44444444-4444-4444-8444-444444444444",
      kind: "qr",
      state: "open",
      expires_at: new Date(Date.now() + 600_000).toISOString(),
      qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
      totp_available: false,
    });
    await renderLogin();
    fireEvent.click(screen.getByRole("button", { name: "Sign in with your phone" }));
    await screen.findByRole("heading", { name: "Sign in with your phone" });
    expect(document.querySelector('input[autocomplete="one-time-code"]')).toBeNull();
    expect(document.querySelector("input")).toBeNull();
    // Back leaves the flow and cancels the attempt.
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => expect(cancelMock).toHaveBeenCalledWith("44444444-4444-4444-8444-444444444444"));
  });

  it("never shows a bare status line for a proxy error", async () => {
    loginMock.mockRejectedValue(new HttpError(502, "HTTP 502: Bad Gateway"));
    await submit();
    await waitFor(() =>
      expect(
        screen.getByText("Something went wrong on the server. Please try again in a moment."),
      ).toBeTruthy(),
    );
    expect(screen.queryByText(/502/)).toBeNull();
  });

  it("register link hidden when registration is closed", async () => {
    settingsMock.mockResolvedValue(serverSettings("closed"));
    await renderLogin();
    await waitFor(() => expect(settingsMock).toHaveBeenCalled());
    await waitFor(() => expect(settingsMock).toHaveResolved());
    expect(screen.queryByText("Register")).toBeNull();
    expect(document.querySelector('a[href^="/register"]')).toBeNull();
  });

  it("register link shown for invite-only server when arriving with an invite", async () => {
    settingsMock.mockResolvedValue(serverSettings("invite_only"));
    window.history.pushState({}, "", "/?returnUrl=" + encodeURIComponent("/invite/AbCd1234"));
    await renderLogin();
    await waitFor(() => expect(document.querySelector('a[href^="/register"]')).not.toBeNull());
    window.history.pushState({}, "", "/");
  });

  it("register link hidden for invite-only server without invite context", async () => {
    settingsMock.mockResolvedValue(serverSettings("invite_only"));
    await renderLogin();
    await waitFor(() => expect(settingsMock).toHaveResolved());
    expect(document.querySelector('a[href^="/register"]')).toBeNull();
  });

  it("register link shown when open", async () => {
    settingsMock.mockResolvedValue(serverSettings("open"));
    await renderLogin();
    await waitFor(() => expect(document.querySelector('a[href^="/register"]')).not.toBeNull());
  });

  it("register link carries the invite returnUrl", async () => {
    settingsMock.mockResolvedValue(serverSettings("invite_only"));
    window.history.pushState({}, "", "/?returnUrl=" + encodeURIComponent("/invite/AbCd1234"));
    await renderLogin();
    await waitFor(() => {
      const href = document.querySelector('a[href^="/register"]')?.getAttribute("href");
      expect(href).toBe("/register?returnUrl=" + encodeURIComponent("/invite/AbCd1234"));
    });
    window.history.pushState({}, "", "/");
  });

  it("malformed percent in returnUrl does not throw and hides the invite link", async () => {
    settingsMock.mockResolvedValue(serverSettings("invite_only"));
    window.history.pushState({}, "", "/?returnUrl=%25ZZ%E0%A4%A");
    await renderLogin();
    await waitFor(() => expect(settingsMock).toHaveResolved());
    expect(document.querySelector('a[href^="/register"]')).toBeNull();
    window.history.pushState({}, "", "/");
  });
});
