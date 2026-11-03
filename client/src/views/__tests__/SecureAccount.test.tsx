import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ startAccountRequest: vi.fn(), requestOidcAuthorize: vi.fn() }));
vi.mock("@/lib/tauri/accountRequests", async (orig) => ({ ...(await orig<object>()), ...api }));
vi.mock("@/lib/tauri/auth", async (orig) => ({ ...(await orig<object>()), fetchServerSettings: vi.fn().mockResolvedValue(null) }));

import SecureAccount from "../SecureAccount";
import { requestOf, routed } from "./accountRequestTestUtils";

beforeEach(() => {
  api.startAccountRequest.mockReset();
  window.history.pushState({}, "", "/secure-account");
});
afterEach(cleanup);

describe("SecureAccount", () => {
  it("bootstrap start needs password and code", async () => {
    api.startAccountRequest.mockResolvedValue(requestOf({ purpose: "bootstrap" }));
    render(routed(SecureAccount, "/secure-account"));
    fireEvent.input(screen.getByLabelText("Username"), { target: { value: "alice" } });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("Enter your password and a code from your authenticator app.")).toBeTruthy();
    expect(api.startAccountRequest).not.toHaveBeenCalled();
    fireEvent.input(screen.getByLabelText("Password"), { target: { value: "pw-123456" } });
    fireEvent.input(screen.getByLabelText("Code from your authenticator app"), { target: { value: "123 456" } });
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(api.startAccountRequest.mock.calls[0][1]).toEqual({
        username: "alice",
        purpose: "bootstrap",
        first: { type: "password", password: "pw-123456" },
        totp_code: "123456",
      }),
    );
  });
});

describe("SecureAccount review round (Argon review 10 #2, Fable m5)", () => {
  it("a 401 is worded for this page, not for a recovery code", async () => {
    const { HttpError } = await import("@/lib/tauri/common");
    const { requestStartError } = await import("../requestErrors");
    const text = requestStartError(new HttpError(401, "x", "INVALID_CREDENTIALS"), "bootstrap");
    expect(text).not.toContain("recovery code");
    expect(text).toContain("authenticator code");
  });

  it("with several providers the user picks one (Argon review 10 #2)", async () => {
    const auth = await import("@/lib/tauri/auth");
    vi.mocked(auth.fetchServerSettings).mockResolvedValue({
      oidc_enabled: true,
      oidc_providers: [
        { slug: "kc", display_name: "Keycloak", icon_hint: null },
        { slug: "gh", display_name: "GitHub", icon_hint: "github" },
      ],
      auth_methods: { local: true, oidc: true },
      registration_policy: "closed",
    } as never);
    render(routed(SecureAccount, "/secure-account"));
    fireEvent.click(await screen.findByLabelText("My sign-in provider"));
    const select = (await screen.findByLabelText("Sign-in provider")) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "gh" } });
    expect(select.value).toBe("gh");
    vi.mocked(auth.fetchServerSettings).mockResolvedValue(null as never);
  });
});

