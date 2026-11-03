import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  setRecoveryContact: vi.fn(),
  removeRecoveryContact: vi.fn(),
  listContactFor: vi.fn(),
  confirmContactFor: vi.fn(),
  getMeSecurity: vi.fn(),
}));
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), ...api }));
const friends = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/social", async (orig) => ({ ...(await orig<object>()), getFriends: friends }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));

import RecoveryContactSection from "../RecoveryContactSection";
import { HttpError } from "@/lib/tauri/common";
import { refreshMeSecurity } from "@/stores/security";
import { setAuthState } from "@/stores/auth";
import { security, signInAs } from "./securityTestUtils";

const contact = { user_id: "c1", username: "bob", display_name: "Bob", named_at: "2026-10-01T00:00:00Z", confirmed_at: "2026-10-02T00:00:00Z", counts_from: "2026-10-03T00:00:00Z" };

beforeEach(async () => {
  for (const f of Object.values(api)) f.mockReset();
  stepUp.mockClear();
  friends.mockResolvedValue([{ user_id: "c2", username: "carol", display_name: "Carol" }]);
  api.listContactFor.mockResolvedValue([]);
  signInAs("full");
});
afterEach(() => {
  cleanup();
  setAuthState({ user: null });
});

async function mount(sec = security()) {
  api.getMeSecurity.mockResolvedValue(sec);
  await refreshMeSecurity();
  render(() => <RecoveryContactSection />);
}

describe("RecoveryContactSection", () => {
  it("recovery contact shows counts from", async () => {
    await mount(security({ recovery_contact: contact }));
    expect(screen.getByText("Bob")).toBeTruthy();
    expect(screen.getByText(/^Counts from /)).toBeTruthy();
  });

  it("an unconfirmed contact waits for confirmation", async () => {
    await mount(security({ recovery_contact: { ...contact, confirmed_at: null, counts_from: null } }));
    expect(screen.getByText("Waiting for confirmation")).toBeTruthy();
  });

  it("choosing a contact steps up for that user; an admin is refused", async () => {
    await mount();
    api.setRecoveryContact.mockRejectedValueOnce(new HttpError(400, "x", "RECOVERY_CONTACT_INVALID"));
    fireEvent.click(screen.getByRole("button", { name: "Choose a recovery contact" }));
    fireEvent.click(await screen.findByRole("button", { name: "Carol (carol)" }));
    expect(await screen.findByText("Choose someone who is not an admin.")).toBeTruthy();
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "recovery_contact.set", target: "c2", targetLabel: "Carol" });
  });

  it("remove uses an empty target", async () => {
    await mount(security({ recovery_contact: contact }));
    api.removeRecoveryContact.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: "Remove recovery contact" }));
    await waitFor(() => expect(api.removeRecoveryContact).toHaveBeenCalled());
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "recovery_contact.set", target: "" });
  });

  it("confirming a contact needs no step-up", async () => {
    api.listContactFor.mockResolvedValue([{ user_id: "o1", username: "oma", display_name: "Oma", named_at: "", confirmed_at: null }]);
    api.confirmContactFor.mockResolvedValue(undefined);
    await mount();
    fireEvent.click(await screen.findByRole("button", { name: "I will help Oma" }));
    await waitFor(() => expect(api.confirmContactFor).toHaveBeenCalledWith("o1"));
    expect(stepUp).not.toHaveBeenCalled();
  });
});
