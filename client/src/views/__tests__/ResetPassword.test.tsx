import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";

import ResetPassword from "../ResetPassword";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

async function submitWithResponse(status: number, body: unknown): Promise<void> {
  vi.stubGlobal(
    "fetch",
    vi.fn().mockResolvedValue({ ok: false, status, json: async () => body }),
  );
  render(() => (
    <Router>
      <Route path="/" component={ResetPassword} />
    </Router>
  ));
  fireEvent.input(await screen.findByPlaceholderText("Paste reset code from email"), {
    target: { value: "abc123" },
  });
  fireEvent.input(screen.getByPlaceholderText("Enter new password"), {
    target: { value: "newpassword1" },
  });
  fireEvent.input(screen.getByPlaceholderText("Confirm new password"), {
    target: { value: "newpassword1" },
  });
  fireEvent.submit(document.querySelector("form")!);
}

describe("ResetPassword", () => {
  it("maps INVALID_TOKEN to the reset-code sentence", async () => {
    await submitWithResponse(400, { error: "INVALID_TOKEN", message: "Invalid token" });
    await waitFor(() =>
      expect(screen.getByText("That reset code is invalid or has expired.")).toBeTruthy(),
    );
  });

  it("ignores a non-string error field and uses the fallback", async () => {
    await submitWithResponse(400, { error: { nested: true }, message: "raw" });
    await waitFor(() => expect(screen.getByText("Could not reset the password. Please try again.")).toBeTruthy());
    expect(screen.queryByText("raw")).toBeNull();
  });
});
