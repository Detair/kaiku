import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ startAccountRequest: vi.fn(), requestOidcAuthorize: vi.fn() }));
vi.mock("@/lib/tauri/accountRequests", async (orig) => ({ ...(await orig<object>()), ...api }));
vi.mock("@/lib/tauri/auth", async (orig) => ({ ...(await orig<object>()), fetchServerSettings: vi.fn().mockResolvedValue(null) }));

import RecoverAccount from "../RecoverAccount";
import { HttpError } from "@/lib/tauri/common";
import { requestOf, routed } from "./accountRequestTestUtils";

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  window.history.pushState({}, "", "/recover");
});
afterEach(cleanup);

function fill() {
  render(routed(RecoverAccount, "/recover"));
  fireEvent.input(screen.getByLabelText("Username"), { target: { value: "alice" } });
  fireEvent.input(screen.getByLabelText("Recovery code"), { target: { value: "abcde fghjk mnp" } });
  fireEvent.input(screen.getByLabelText("Password"), { target: { value: "pw-123456" } });
}

describe("RecoverAccount", () => {
  it("shows the stolen-phone advice and the no-codes hint", () => {
    render(routed(RecoverAccount, "/recover"));
    expect(screen.getByText("Phone stolen, and someone may know its screen lock? Ask an admin to remove it now.")).toBeTruthy();
    expect(screen.getByText("No recovery codes? Only an admin can help.")).toBeTruthy();
  });

  it("recover sends lost all_key_devices when checked", async () => {
    api.startAccountRequest.mockResolvedValue(requestOf());
    fill();
    fireEvent.click(screen.getByLabelText("I lost my phone"));
    fireEvent.click(screen.getByRole("button", { name: "Start recovery" }));
    await waitFor(() => expect(api.startAccountRequest).toHaveBeenCalled());
    expect(api.startAccountRequest.mock.calls[0][1]).toEqual({
      username: "alice",
      purpose: "recovery",
      recovery_code: "ABCDEFGHJKMNP",
      second: { type: "password", password: "pw-123456" },
      lost: "all_key_devices",
    });
    await screen.findByTestId("elsewhere");
  });

  it("every refusal shows the same not-accepted text", async () => {
    const text = "We couldn't start a recovery with these details. Check the username and recovery code, or ask an admin.";
    for (const err of [new HttpError(401, "unknown user", "INVALID_CREDENTIALS"), new HttpError(401, "bad code", "INVALID_CREDENTIALS")]) {
      api.startAccountRequest.mockRejectedValueOnce(err);
      fill();
      fireEvent.click(screen.getByRole("button", { name: "Start recovery" }));
      expect(await screen.findByText(text)).toBeTruthy();
      cleanup();
    }
  });

  it("REQUEST_FACTOR_INVALID shows codes left", async () => {
    api.startAccountRequest.mockRejectedValueOnce(new HttpError(401, "x", "REQUEST_FACTOR_INVALID", { recovery_codes_left: 4 }));
    fill();
    fireEvent.click(screen.getByRole("button", { name: "Start recovery" }));
    expect(await screen.findByText("That recovery code is now used up, but the second step was wrong. 4 codes left.")).toBeTruthy();
  });

  it("NO_INDEPENDENT_CANCELLER shows the recovery-contact rule", async () => {
    api.startAccountRequest.mockRejectedValueOnce(new HttpError(409, "x", "NO_INDEPENDENT_CANCELLER"));
    fill();
    fireEvent.click(screen.getByRole("button", { name: "Start recovery" }));
    expect(await screen.findByText("Name and confirm a recovery contact first, or use the host CLI.")).toBeTruthy();
  });
});

describe("RecoverAccount review round", () => {
  it("an empty second factor never goes out with the recovery code (Argon review 10 #1)", async () => {
    render(routed(RecoverAccount, "/recover"));
    fireEvent.input(screen.getByLabelText("Username"), { target: { value: "alice" } });
    fireEvent.input(screen.getByLabelText("Recovery code"), { target: { value: "abcde fghjk mnp" } });
    fireEvent.click(screen.getByRole("button", { name: "Start recovery" }));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(api.startAccountRequest).not.toHaveBeenCalled();
  });
});
