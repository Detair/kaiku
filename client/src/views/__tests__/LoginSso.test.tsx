import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const authorizeMock = vi.hoisted(() => vi.fn());
const loginWithOidcMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  login: vi.fn(),
  fetchServerSettings: vi.fn().mockResolvedValue({
    oidc_enabled: true,
    oidc_providers: [{ slug: "idp", display_name: "My IdP", icon_hint: null }],
    auth_methods: { local: true },
    registration_policy: "open",
  }),
  getDefaultServerUrl: vi.fn().mockResolvedValue(null),
  oidcAuthorize: authorizeMock,
}));

vi.mock("@/stores/auth", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/stores/auth")>()),
  loginWithOidc: loginWithOidcMock,
}));

import { isSessionRestoreBlocked, setSessionRestoreBlocked } from "@/lib/tauri/common";
import Login from "../Login";
import Register from "../Register";

function fakePopup() {
  return { closed: false, close: vi.fn(), location: { href: "" } };
}

for (const [name, View, path] of [
  ["Login", Login, "/"],
  ["Register", Register, "/"],
] as const) {
  describe(`${name} SSO popup`, () => {
    let openSpy: ReturnType<typeof vi.spyOn>;
    beforeEach(() => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
    });
    afterEach(() => {
      cleanup();
      vi.useRealTimers();
      openSpy?.mockRestore();
      authorizeMock.mockReset();
      loginWithOidcMock.mockReset();
      window.history.pushState({}, "", "/");
    });

    async function clickSso() {
      render(() => (
        <Router>
          <Route path={path} component={View} />
        </Router>
      ));
      const btn = await screen.findByText(/My IdP/);
      fireEvent.click(btn.closest("button")!);
      return btn.closest("button")!;
    }

    it("shows an error and re-enables the button when the popup is blocked", async () => {
      openSpy = vi.spyOn(window, "open").mockReturnValue(null);
      const btn = await clickSso();
      await waitFor(() => expect(screen.getByText(/sign-in window was blocked/)).toBeTruthy());
      expect(authorizeMock).not.toHaveBeenCalled();
      expect((btn as HTMLButtonElement).disabled).toBe(false);
    });

    it("opens the popup before the authorize request resolves, then navigates it", async () => {
      const popup = fakePopup();
      openSpy = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      let resolveAuthorize!: (v: unknown) => void;
      authorizeMock.mockReturnValue(new Promise((r) => (resolveAuthorize = r)));
      await clickSso();
      expect(openSpy).toHaveBeenCalledTimes(1);
      expect(openSpy.mock.calls[0][0]).toBe("about:blank");
      expect(popup.location.href).toBe("");
      resolveAuthorize({ mode: "browser", authUrl: "http://localhost:3000/auth/x" });
      await waitFor(() => expect(popup.location.href).toBe("http://localhost:3000/auth/x"));
    });

    it("closes the blank popup when the authorize request fails", async () => {
      const popup = fakePopup();
      openSpy = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      authorizeMock.mockRejectedValue(new Error("boom"));
      await clickSso();
      await waitFor(() => expect(popup.close).toHaveBeenCalled());
    });

    async function openedPopup() {
      const popup = fakePopup();
      openSpy = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      authorizeMock.mockResolvedValue({ mode: "browser", authUrl: "http://localhost:3000/auth/x" });
      const btn = await clickSso();
      await waitFor(() => expect(popup.location.href).not.toBe(""));
      return { popup, btn: btn as HTMLButtonElement };
    }
    const callback = (source: unknown) =>
      new MessageEvent("message", {
        origin: window.location.origin,
        source: source as MessageEventSource,
        data: { type: "oidc-callback", access_token: "tok", expires_in: 900 },
      });

    it("shows the reload hint when the popup closes without a result", async () => {
      const { popup, btn } = await openedPopup();
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(2000);
      await waitFor(() => expect(screen.getByText(/Reload this page to continue/)).toBeTruthy());
      expect(btn.disabled).toBe(true);
      expect(loginWithOidcMock).not.toHaveBeenCalled();
    });

    it("Reload lifts the logout's restore block, then reloads the page", async () => {
      const { popup } = await openedPopup();
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(2000);
      await waitFor(() => expect(screen.getByText("Reload")).toBeTruthy());
      setSessionRestoreBlocked(true);
      const reload = vi.fn(() => expect(isSessionRestoreBlocked()).toBe(false));
      const original = window.location;
      Object.defineProperty(window, "location", { configurable: true, value: { ...original, reload } });
      try {
        fireEvent.click(screen.getByText("Reload"));
      } finally {
        Object.defineProperty(window, "location", { configurable: true, value: original });
        setSessionRestoreBlocked(false);
      }
      expect(reload).toHaveBeenCalledTimes(1);
    });

    it("Cancel hides the hint and re-enables the button, flag untouched", async () => {
      const { popup, btn } = await openedPopup();
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(2000);
      await waitFor(() => expect(screen.getByText("Cancel")).toBeTruthy());
      setSessionRestoreBlocked(true);
      try {
        fireEvent.click(screen.getByText("Cancel"));
        expect(isSessionRestoreBlocked()).toBe(true);
      } finally {
        setSessionRestoreBlocked(false);
      }
      expect(screen.queryByText(/Reload this page/)).toBeNull();
      expect(btn.disabled).toBe(false);
    });

    it("a callback message within the close grace still signs in", async () => {
      loginWithOidcMock.mockResolvedValue(undefined);
      const { popup } = await openedPopup();
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(600);
      window.dispatchEvent(callback(popup));
      await vi.advanceTimersByTimeAsync(2000);
      expect(loginWithOidcMock).toHaveBeenCalledTimes(1);
      expect(screen.queryByText(/Reload this page/)).toBeNull();
    });

    it("ignores a callback message from another window", async () => {
      await openedPopup();
      window.dispatchEvent(callback({}));
      await vi.advanceTimersByTimeAsync(0);
      expect(loginWithOidcMock).not.toHaveBeenCalled();
    });

    it("stops watching and closes the popup when the view unmounts", async () => {
      const { popup } = await openedPopup();
      cleanup();
      expect(popup.close).toHaveBeenCalled();
      window.dispatchEvent(callback(popup));
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(3000);
      expect(loginWithOidcMock).not.toHaveBeenCalled();
    });

    it("unmount while the authorize request is pending never starts watching", async () => {
      const popup = fakePopup();
      openSpy = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      let resolveAuthorize!: (v: unknown) => void;
      authorizeMock.mockReturnValue(new Promise((r) => (resolveAuthorize = r)));
      await clickSso();
      cleanup();
      resolveAuthorize({ mode: "browser", authUrl: "http://localhost:3000/auth/x" });
      await vi.advanceTimersByTimeAsync(0);
      expect(popup.location.href).toBe("");
      window.dispatchEvent(callback(popup));
      await vi.advanceTimersByTimeAsync(0);
      expect(loginWithOidcMock).not.toHaveBeenCalled();
    });
  });
}
