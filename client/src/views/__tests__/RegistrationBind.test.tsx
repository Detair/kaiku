import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";

const tauriMode = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on }));
const api = vi.hoisted(() => ({
  getRegistration: vi.fn(),
  startRegistrationTotp: vi.fn(),
  verifyRegistrationTotp: vi.fn(),
  startRegistrationPasskey: vi.fn(),
  finishRegistrationPasskey: vi.fn(),
}));
vi.mock("@/lib/tauri/registration", async (orig) => ({ ...(await orig<object>()), ...api }));
const panel = vi.hoisted(() => vi.fn());
vi.mock("@/components/settings/security/EnrollmentIssuerPanel", () => ({
  default: (p: { issuer: unknown }) => {
    panel(p.issuer);
    return <div data-testid="issuer-panel" />;
  },
}));

import RegistrationBind from "../RegistrationBind";
import { routed } from "./accountRequestTestUtils";

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  panel.mockReset();
  tauriMode.on = false;
  api.getRegistration.mockResolvedValue({ expires_at: new Date(Date.now() + 25 * 60_000).toISOString(), bound: false });
  window.history.pushState({}, "", "/register/bind");
});
afterEach(cleanup);

const mount = () => render(routed(RegistrationBind, "/register/bind"));

describe("RegistrationBind", () => {
  it("countdown warns before deletion", async () => {
    mount();
    expect(await screen.findByText("Finish within 25 minutes, or the account is deleted.")).toBeTruthy();
  });

  it("passkey choice is hidden in the desktop app", async () => {
    tauriMode.on = true;
    mount();
    await screen.findByRole("button", { name: "Use the Kaiku app on your phone" });
    expect(screen.queryByRole("button", { name: "Use a passkey" })).toBeNull();
  });

  it("phone choice uses the registration issuer", async () => {
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Use the Kaiku app on your phone" }));
    expect(await screen.findByTestId("issuer-panel")).toBeTruthy();
    expect(panel).toHaveBeenCalledWith({ kind: "registration" });
  });

  it("authenticator choice shows recovery codes once", async () => {
    api.startRegistrationTotp.mockResolvedValue({ secret: "S", otpauth_uri: "otpauth://totp/Kaiku:neu?secret=S" });
    api.verifyRegistrationTotp.mockResolvedValue(["ABCDEFGHJKMNP"]);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Use an authenticator app" }));
    fireEvent.input(await screen.findByLabelText("Code from your authenticator app"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(await screen.findByText("ABCDE FGHJK MNP")).toBeTruthy();
    expect(api.verifyRegistrationTotp).toHaveBeenCalledWith("123456");
    fireEvent.click(screen.getByRole("button", { name: "I wrote them down" }));
    expect(screen.getByText("Sign in with your password and a code.")).toBeTruthy();
  });

  it("a bound registration jumps to the sign-in hint", async () => {
    api.getRegistration.mockResolvedValue({ expires_at: "", bound: true });
    mount();
    expect(await screen.findByRole("link", { name: "Sign in" })).toBeTruthy();
  });
});
