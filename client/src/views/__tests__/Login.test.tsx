// Same Solid-instance note as i18n/__tests__/locale.dom.test.tsx: components
// render through @solidjs/testing-library with the real store and i18n.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const loginMock = vi.hoisted(() => vi.fn());
const settingsMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  login: loginMock,
  fetchServerSettings: settingsMock,
  getDefaultServerUrl: vi.fn().mockResolvedValue(null),
  oidcAuthorize: vi.fn(),
}));

import Login from "../Login";
import { HttpError } from "@/lib/tauri/common";
import { setAuthState } from "@/stores/auth";

afterEach(() => {
  cleanup();
  window.history.pushState({}, "", "/");
  setAuthState({ error: null, mfaRequired: false, isLoading: false });
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
    await waitFor(() => expect(screen.getByText("Invalid username or password.")).toBeTruthy());
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
