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
