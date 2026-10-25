import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";

vi.mock("@/lib/tauri", () => {
  class HttpError extends Error {
    constructor(
      public status: number,
      message: string,
      public code?: string,
    ) {
      super(message);
    }
  }
  return { HttpError };
});

import { HttpError } from "@/lib/tauri";
import ReauthDialog from "../ReauthDialog";
import type { ReauthMethod } from "@/lib/reauth";

const PW: ReauthMethod[] = [{ kind: "password" }];
const OIDC: ReauthMethod = { kind: "oidc", providerId: "kc", label: "Keycloak" };

function setup(over: Partial<Parameters<typeof ReauthDialog>[0]> = {}) {
  const onSubmit = vi.fn().mockResolvedValue(undefined);
  const onClose = vi.fn();
  render(() => (
    <ReauthDialog
      title="Unlock"
      mode="password"
      mfaEnabled={false}
      methods={PW}
      onSubmit={onSubmit}
      onClose={onClose}
      {...over}
    />
  ));
  return { onSubmit, onClose };
}

const type = (label: RegExp, value: string) =>
  fireEvent.input(screen.getByLabelText(label), { target: { value } });

describe("ReauthDialog", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("requires a code when MFA is enabled", async () => {
    const { onSubmit } = setup({ mfaEnabled: true });
    type(/password/i, "pw");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("authenticator"),
    );
    expect(onSubmit).not.toHaveBeenCalled();

    type(/authenticator code/i, "123456");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({ password: "pw", mfa_code: "123456" }),
    );
  });

  it("shows invalid password error", async () => {
    const onSubmit = vi
      .fn()
      .mockRejectedValue(new HttpError(401, "nope", "INVALID_CREDENTIALS"));
    setup({ onSubmit });
    type(/password/i, "bad");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toBe(
        "That password or code is not correct.",
      ),
    );
  });

  it("shows a rate limit message for 429", async () => {
    const onSubmit = vi.fn().mockRejectedValue(new HttpError(429, "slow"));
    setup({ onSubmit });
    type(/password/i, "x");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("Too many attempts"),
    );
  });

  it("mfaOnly mode asks only for the code and sends an empty password", async () => {
    const { onSubmit } = setup({ mode: "mfaOnly", mfaEnabled: true, methods: [] });
    expect(screen.queryByLabelText(/your password/i)).toBeNull();
    type(/authenticator code/i, "654321");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({ password: "", mfa_code: "654321" }),
    );
  });

  it("strips whitespace from the code", async () => {
    const { onSubmit } = setup({ mode: "mfaOnly", mfaEnabled: true, methods: [] });
    type(/authenticator code/i, " 123 456 ");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({ password: "", mfa_code: "123456" }),
    );
  });

  it("password-only: no OIDC button", () => {
    setup();
    expect(screen.getByLabelText(/your password/i)).toBeTruthy();
    expect(screen.queryByText(/Confirm with/)).toBeNull();
  });

  it("OIDC-only with the path not switched on: unavailable text, no form", () => {
    setup({ methods: [] });
    expect(screen.getByText(/not available for this account/i)).toBeTruthy();
    expect(screen.queryByLabelText(/your password/i)).toBeNull();
  });

  it("OIDC-only: provider button, no password field", async () => {
    const onOidc = vi.fn().mockResolvedValue(undefined);
    setup({ methods: [OIDC], onOidc });
    expect(screen.queryByLabelText(/your password/i)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Confirm with Keycloak" }));
    await waitFor(() => expect(onOidc).toHaveBeenCalledWith(OIDC, null));
  });

  it("OIDC path needs the authenticator code when MFA is on and passes it", async () => {
    const onOidc = vi.fn().mockResolvedValue(undefined);
    setup({ methods: [OIDC], onOidc, mfaEnabled: true });
    fireEvent.click(screen.getByRole("button", { name: "Confirm with Keycloak" }));
    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(onOidc).not.toHaveBeenCalled();
    type(/authenticator code/i, "111 222");
    fireEvent.click(screen.getByRole("button", { name: "Confirm with Keycloak" }));
    await waitFor(() => expect(onOidc).toHaveBeenCalledWith(OIDC, "111222"));
  });

  it("REAUTH_OIDC_REQUIRED switches to the provider method", async () => {
    const onSubmit = vi
      .fn()
      .mockRejectedValue(new HttpError(400, "x", "REAUTH_OIDC_REQUIRED"));
    setup({ methods: [...PW, OIDC], onSubmit, onOidc: vi.fn() });
    type(/password/i, "pw");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(screen.queryByLabelText(/your password/i)).toBeNull());
    expect(screen.getByRole("button", { name: "Confirm with Keycloak" })).toBeTruthy();
  });

  it("REAUTH_UNAVAILABLE shows the unavailable text", async () => {
    const onSubmit = vi
      .fn()
      .mockRejectedValue(new HttpError(400, "x", "REAUTH_UNAVAILABLE"));
    setup({ onSubmit });
    type(/password/i, "pw");
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(screen.getByText(/not available for this account/i)).toBeTruthy(),
    );
  });

  it("both methods: password form and provider button", () => {
    setup({ methods: [...PW, OIDC], onOidc: vi.fn() });
    expect(screen.getByLabelText(/your password/i)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Confirm with Keycloak" })).toBeTruthy();
  });
});
