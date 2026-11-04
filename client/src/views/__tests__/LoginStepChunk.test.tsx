// Flash r3 p3 #2: the sign-in steps are lazy chunks (no Suspense, so nothing
// throws while they load; Solid's lazy renders nothing until then). If one
// cannot load, the page says so and offers a reload and the way back.
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

vi.mock("@/components/auth/login/QrLoginPanel", () => {
  throw new Error("chunk load failed");
});
const api = vi.hoisted(() => ({ createQrAttempt: vi.fn(), pollAttempt: vi.fn(), cancelAttempt: vi.fn() }));
vi.mock("@/lib/tauri", async () => ({
  ...api,
  passwordLogin: vi.fn(),
  collectAttempt: vi.fn(),
  attemptTotp: vi.fn(),
  fetchServerSettings: vi.fn().mockResolvedValue(null),
  getDefaultServerUrl: vi.fn().mockResolvedValue(null),
  oidcAuthorize: vi.fn(),
  passkeySignIn: vi.fn(() => new Promise(() => {})),
}));

import Login from "../Login";
import { leaveFlow } from "@/stores/loginFlow";

afterEach(async () => {
  cleanup();
  await leaveFlow();
  vi.restoreAllMocks();
});

describe("a sign-in step whose chunk cannot load", () => {
  it("offers a reload and the way back to the form", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    api.cancelAttempt.mockResolvedValue(undefined);
    api.pollAttempt.mockReturnValue(new Promise(() => {}));
    api.createQrAttempt.mockResolvedValue({
      attempt_id: "55555555-5555-4555-8555-555555555555",
      kind: "qr",
      state: "open",
      expires_at: new Date(Date.now() + 600_000).toISOString(),
      qr: { payload: "kaiku://login/v1?o=x&t=a", expires_at: "" },
      totp_available: false,
    });
    render(() => (
      <Router>
        <Route path="/" component={Login} />
      </Router>
    ));
    fireEvent.click(await screen.findByRole("button", { name: "Sign in with your phone" }));
    expect((await screen.findByRole("alert")).textContent).toContain("could not be loaded");
    expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByPlaceholderText("Enter your username")).toBeTruthy();
    expect(api.cancelAttempt).toHaveBeenCalled();
  });
});
