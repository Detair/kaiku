import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ setPassword: vi.fn(), removePassword: vi.fn(), getMeSecurity: vi.fn() }));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import PasswordSection from "../PasswordSection";
import { HttpError } from "@/lib/tauri/common";
import { refreshMeSecurity } from "@/stores/security";
import { setAuthState } from "@/stores/auth";
import { security, signInAs } from "./securityTestUtils";

async function mount(sec = security()) {
  api.getMeSecurity.mockResolvedValue(sec);
  await refreshMeSecurity();
  render(() => <PasswordSection />);
}

async function submit(pw: string) {
  fireEvent.click(screen.getByRole("button", { name: "Change password" }));
  fireEvent.input(await screen.findByLabelText("New password"), { target: { value: pw } });
  fireEvent.input(screen.getByLabelText("Repeat the new password"), { target: { value: pw } });
  fireEvent.click(screen.getByRole("button", { name: "Save password" }));
}

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  signInAs("full");
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

describe("PasswordSection", () => {
  it("password rejected maps each reason", async () => {
    await mount();
    const texts: Record<string, string> = {
      length: "Use at least 8 characters.",
      breached: "This password is too common or was leaked. Choose another one.",
      context: "Don't use your name or the server's name.",
    };
    for (const [reason, text] of Object.entries(texts)) {
      api.setPassword.mockRejectedValueOnce(new HttpError(400, "x", "PASSWORD_REJECTED", { reason }));
      await submit("some-password-1");
      expect(await screen.findByText(text)).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    }
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "password.set", target: "" });
  });

  it("a lowercase password_rejected code still names the reason (Argon review 3 #6)", async () => {
    await mount();
    api.setPassword.mockRejectedValueOnce(new HttpError(400, "x", "password_rejected", { reason: "breached" }));
    await submit("some-password-1");
    expect(await screen.findByText("This password is too common or was leaked. Choose another one.")).toBeTruthy();
  });

  it("saves with a password.set step-up", async () => {
    await mount();
    api.setPassword.mockResolvedValue(undefined);
    await submit("a-good-new-password");
    await waitFor(() => expect(api.setPassword).toHaveBeenCalledWith("a-good-new-password"));
  });

  it("password section explains when a password has no use", async () => {
    await mount(security({ password: null, totp: null, may_approve: false }));
    expect(
      screen.getByText("A password has no use on this account: sign in with your phone or a passkey."),
    ).toBeTruthy();
  });

  it("compromised password asks for a new one", async () => {
    await mount(security({ password: { state: "compromised" } }));
    expect(screen.getByText("This password was marked unsafe. Set a new one.")).toBeTruthy();
  });

  it("removing the password offers to sign out everywhere else (Argon review 3 #7)", async () => {
    const { signOutOthersPrompt, closeSignOutOthers } = await import("@/stores/security");
    closeSignOutOthers();
    api.removePassword.mockResolvedValue(undefined);
    await mount(security({ password: { state: "active" }, passkeys: 1 }));
    fireEvent.click(screen.getByRole("button", { name: "Remove password" }));
    await waitFor(() => expect(api.removePassword).toHaveBeenCalled());
    await waitFor(() => expect(signOutOthersPrompt()).toBe(true));
    closeSignOutOthers();
  });

  it("remove steps up with password.remove", async () => {
    await mount();
    api.removePassword.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: "Remove password" }));
    await waitFor(() => expect(api.removePassword).toHaveBeenCalled());
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "password.remove" });
  });
});
