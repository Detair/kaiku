import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route, useLocation } from "@solidjs/router";

const mocks = vi.hoisted(() => ({ register: vi.fn(), settings: vi.fn() }));

vi.mock("@/lib/tauri", () => ({
  fetchServerSettings: mocks.settings,
  oidcAuthorize: vi.fn(),
}));
vi.mock("@/stores/auth", async (orig) => ({
  ...(await orig<object>()),
  register: mocks.register,
}));

import Register from "../Register";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  window.history.pushState({}, "", "/");
});

function Landing() {
  const location = useLocation();
  return <div data-testid="landed">{location.pathname}</div>;
}

async function registerAs(search: string): Promise<void> {
  mocks.settings.mockResolvedValue({
    oidc_enabled: false,
    oidc_providers: [],
    auth_methods: { local: true },
    registration_policy: "invite_only",
  });
  mocks.register.mockResolvedValue({ id: "u1", username: "newbie" });
  window.history.pushState({}, "", "/register" + search);
  render(() => (
    <Router>
      <Route path="/register" component={Register} />
      <Route path="*" component={Landing} />
    </Router>
  ));
  fireEvent.input(await screen.findByTestId("register-username"), { target: { value: "newbie" } });
  fireEvent.input(screen.getByTestId("register-password"), { target: { value: "longenough1" } });
  fireEvent.input(screen.getByTestId("register-password-confirm"), {
    target: { value: "longenough1" },
  });
  fireEvent.click(screen.getByTestId("register-submit"));
}

describe("Register", () => {
  it("register returns to the invite from returnUrl", async () => {
    await registerAs("?returnUrl=" + encodeURIComponent("/invite/AbCd1234"));
    await waitFor(() => expect(screen.getByTestId("landed").textContent).toBe("/invite/AbCd1234"));
  });

  it("register ignores an external returnUrl", async () => {
    await registerAs("?returnUrl=" + encodeURIComponent("//evil.example/x"));
    await waitFor(() => expect(screen.getByTestId("landed").textContent).toBe("/"));
  });
});
