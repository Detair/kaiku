import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const tauriMode = vi.hoisted(() => ({ on: false }));
vi.mock("@tauri-apps/api/core", async (orig) => ({ ...(await orig<object>()), isTauri: () => tauriMode.on }));
const api = vi.hoisted(() => ({
  getCurrentRequest: vi.fn(),
  withdrawRequest: vi.fn(),
  finishRequest: vi.fn(),
  setRequestPassword: vi.fn(),
  startRequestTotp: vi.fn(),
  verifyRequestTotp: vi.fn(),
  setRequestRecoveryContact: vi.fn(),
}));
vi.mock("@/lib/tauri/accountRequests", async (orig) => ({ ...(await orig<object>()), ...api }));
const panel = vi.hoisted(() => vi.fn());
vi.mock("@/components/settings/security/EnrollmentIssuerPanel", () => ({
  default: (p: { issuer: unknown; onDone: (e: unknown) => void }) => {
    panel(p.issuer);
    return (
      <div data-testid="issuer-panel">
        <button type="button" onClick={() => p.onDone({ state: "done" })}>
          panel-done
        </button>
      </div>
    );
  },
}));

import AccountRequestView from "../AccountRequestView";
import { requestOf, routed } from "./accountRequestTestUtils";

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  panel.mockReset();
  tauriMode.on = false;
  window.history.pushState({}, "", "/account-request");
});
afterEach(cleanup);

const mount = () => render(routed(AccountRequestView, "/account-request"));

describe("AccountRequestView", () => {
  it("waiting request shows ready time and withdraw", async () => {
    api.getCurrentRequest.mockResolvedValue(requestOf());
    api.withdrawRequest.mockResolvedValue(undefined);
    mount();
    expect(await screen.findByText(/^Ready at .*We told your devices, your recovery contact and the admins\.$/)).toBeTruthy();
    api.getCurrentRequest.mockResolvedValue(requestOf({ state: "withdrawn" }));
    fireEvent.click(screen.getByRole("button", { name: "Withdraw request" }));
    await waitFor(() => expect(api.withdrawRequest).toHaveBeenCalled());
  });

  it("ready request stages a phone through the request issuer", async () => {
    api.getCurrentRequest.mockResolvedValue(requestOf({ state: "ready", expires_at: "2026-11-05T08:00:00Z" }));
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Set up a phone" }));
    expect(await screen.findByTestId("issuer-panel")).toBeTruthy();
    expect(panel).toHaveBeenCalledWith({ kind: "request" });
    expect(screen.getByText(/^Finish by /)).toBeTruthy();
  });

  it("finish shows recovery codes once", async () => {
    api.getCurrentRequest.mockResolvedValue(
      requestOf({ state: "ready", staged: { authenticator: { type: "key_device", label: "Pixel 8" }, password: false, totp: false, recovery_contact: null } }),
    );
    api.finishRequest.mockResolvedValue({ request: requestOf({ state: "completed" }), recovery_codes: ["ABCDEFGHJKMNP"] });
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Finish" }));
    expect(await screen.findByText("ABCDE FGHJK MNP")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "I wrote them down" }));
    expect(screen.queryByText("ABCDE FGHJK MNP")).toBeNull();
    expect(screen.getByRole("link", { name: "Sign in now" })).toBeTruthy();
  });

  it("void request explains and offers start again", async () => {
    api.getCurrentRequest.mockResolvedValue(requestOf({ state: "void" }));
    mount();
    expect(await screen.findByText("This request is no longer valid: something about the account changed while it waited.")).toBeTruthy();
    expect(screen.getByRole("link", { name: "Start again" }).getAttribute("href")).toBe("/recover");
  });

  it("desktop hides the passkey item and never receives the request secret", async () => {
    tauriMode.on = true;
    api.getCurrentRequest.mockResolvedValue({ ...requestOf({ state: "ready" }) });
    mount();
    expect(await screen.findByText("To add a passkey, use a browser.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Add a passkey in this browser" })).toBeNull();
    expect(document.body.textContent).not.toMatch(/request_secret/);
  });
});

describe("AccountRequestView review round (Argon review 9 #1, #4, 6 #8)", () => {
  it("a phone set up here goes back to the choices, where Finish is", async () => {
    api.getCurrentRequest.mockResolvedValue(requestOf({ state: "ready" }));
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Set up a phone" }));
    fireEvent.click(await screen.findByRole("button", { name: "panel-done" }));
    expect(await screen.findByRole("button", { name: "Finish" })).toBeTruthy();
    expect(screen.queryByTestId("issuer-panel")).toBeNull();
  });

  it("a failed TOTP start goes back with the reason", async () => {
    const { HttpError } = await import("@/lib/tauri/common");
    api.getCurrentRequest.mockResolvedValue(requestOf({ state: "ready" }));
    api.startRequestTotp.mockRejectedValue(new HttpError(503, "x", "SERVICE_UNAVAILABLE"));
    mount();
    fireEvent.click(await screen.findByRole("button", { name: /authenticator app/i }));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Finish" })).toBeTruthy();
  });

  it("the password form has a way back", async () => {
    api.getCurrentRequest.mockResolvedValue(requestOf({ state: "ready" }));
    mount();
    fireEvent.click(await screen.findByRole("button", { name: /password/i }));
    fireEvent.click(await screen.findByRole("button", { name: "Back" }));
    expect(await screen.findByRole("button", { name: "Finish" })).toBeTruthy();
  });

  it("a request that cannot be loaded offers a new start", async () => {
    const { HttpError } = await import("@/lib/tauri/common");
    api.getCurrentRequest.mockRejectedValue(new HttpError(404, "x", "NO_PRE_SESSION_SECRET"));
    mount();
    expect(await screen.findByRole("link", { name: /Start again/i })).toBeTruthy();
    expect(screen.queryByText("Loading...")).toBeNull();
  });

  it("withdrawing does not read the request again (its secret is gone)", async () => {
    api.getCurrentRequest.mockResolvedValue(requestOf());
    api.withdrawRequest.mockResolvedValue(undefined);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw request" }));
    await waitFor(() => expect(api.withdrawRequest).toHaveBeenCalled());
    await new Promise((r) => setTimeout(r, 0));
    expect(api.getCurrentRequest).toHaveBeenCalledTimes(1);
  });
});

