import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const authorizeMock = vi.hoisted(() => vi.fn());
const restoreMock = vi.hoisted(() => vi.fn());
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
  restoreSessionAfterPopup: restoreMock,
  loginWithOidc: loginWithOidcMock,
}));

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
      restoreMock.mockReset();
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

    it("tries to restore the session when the popup closes without a result", async () => {
      const popup = fakePopup();
      openSpy = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      authorizeMock.mockResolvedValue({ mode: "browser", authUrl: "http://localhost:3000/auth/x" });
      restoreMock.mockResolvedValue(true);
      await clickSso();
      await waitFor(() => expect(popup.location.href).not.toBe(""));
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(600);
      await waitFor(() => expect(restoreMock).toHaveBeenCalledTimes(1));
    });

    it("resets quietly when the popup closes and there is no session", async () => {
      const popup = fakePopup();
      openSpy = vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
      authorizeMock.mockResolvedValue({ mode: "browser", authUrl: "http://localhost:3000/auth/x" });
      restoreMock.mockResolvedValue(false);
      const btn = await clickSso();
      await waitFor(() => expect(popup.location.href).not.toBe(""));
      popup.closed = true;
      await vi.advanceTimersByTimeAsync(600);
      await waitFor(() => expect((btn as HTMLButtonElement).disabled).toBe(false));
      expect(screen.queryByText(/failed/i)).toBeNull();
    });
  });
}
