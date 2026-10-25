// Same Solid-instance note as i18n/__tests__/locale.dom.test.tsx: components
// render through @solidjs/testing-library with the real store and i18n.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

const loginMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  login: loginMock,
  fetchServerSettings: vi.fn().mockResolvedValue(null),
  getDefaultServerUrl: vi.fn().mockResolvedValue(null),
  oidcAuthorize: vi.fn(),
}));

import Login from "../Login";
import { HttpError } from "@/lib/tauri/common";
import { setAuthState } from "@/stores/auth";

afterEach(() => {
  cleanup();
  setAuthState({ error: null, mfaRequired: false, isLoading: false });
  loginMock.mockReset();
});

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
});
