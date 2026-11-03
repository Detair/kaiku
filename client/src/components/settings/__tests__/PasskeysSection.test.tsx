import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  listPasskeys: vi.fn(),
  stepUpWithPasskey: vi.fn(),
  registerPasskey: vi.fn(),
  renamePasskey: vi.fn(),
  deletePasskey: vi.fn(),
}));
const signalRemoved = vi.hoisted(() => vi.fn());
const tauriMode = vi.hoisted(() => ({ on: false }));

vi.mock("@/lib/tauri/passkeys", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/passkeys")),
  ...api,
}));
vi.mock("@/lib/webauthn", async () => ({
  ...(await vi.importActual<object>("@/lib/webauthn")),
  signalRemoved,
}));
vi.mock("@/lib/tauri/common", async () => ({
  ...(await vi.importActual<object>("@/lib/tauri/common")),
  isTauri: () => tauriMode.on,
}));

import PasskeysSection from "../PasskeysSection";
import { PasskeyError } from "@/lib/tauri/passkeys";

const ACTIVE = {
  id: "p-1",
  label: "Laptop",
  state: "active",
  created_at: "2026-11-01T10:00:00Z",
  last_used_at: "2026-11-02T10:00:00Z",
  ready_at: null,
  backup_eligible: true,
  backup_state: true,
  transports: [],
  aaguid: null,
};
const PENDING = { ...ACTIVE, id: "p-2", label: "Phone", state: "pending", ready_at: "2026-11-05T10:00:00Z", backup_state: false, last_used_at: null };
const REMOVED = { rp_id: "chat.example.com", user_handle: "dQ", credential_id: "Yw", remaining_credential_ids: [] };

function list(...passkeys: unknown[]) {
  api.listPasskeys.mockResolvedValue({ rp_id: "chat.example.com", user_handle: "dQ", passkeys });
}

beforeEach(() => {
  tauriMode.on = false;
  for (const f of Object.values(api)) f.mockReset();
  signalRemoved.mockReset();
  api.stepUpWithPasskey.mockResolvedValue({ action: "passkey.add", target: "", via: "passkey", expires_at: "x" });
});
afterEach(cleanup);

describe("PasskeysSection", () => {
  it("lists passkeys with synced badge and last use", async () => {
    list(ACTIVE);
    render(() => <PasskeysSection />);
    expect(await screen.findByText("Laptop")).toBeTruthy();
    expect(screen.getByText("Synced")).toBeTruthy();
    expect(screen.getByText(/Last used/)).toBeTruthy();
  });

  it("pending passkey shows ready time without actions", async () => {
    list(ACTIVE, PENDING);
    render(() => <PasskeysSection />);
    expect(await screen.findByText(/Ready at/)).toBeTruthy();
    const row = screen.getByText("Phone").closest("li")!;
    expect(row.querySelectorAll("button").length).toBe(0);
  });

  it("add with an existing passkey steps up then registers", async () => {
    list(ACTIVE);
    api.registerPasskey.mockResolvedValue({ ...ACTIVE, id: "p-3", label: "New" });
    render(() => <PasskeysSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Add a passkey" }));
    await waitFor(() => expect(api.registerPasskey).toHaveBeenCalledOnce());
    expect(api.stepUpWithPasskey).toHaveBeenCalledWith("passkey.add", null, expect.any(AbortSignal));
    expect(api.stepUpWithPasskey.mock.invocationCallOrder[0]).toBeLessThan(
      api.registerPasskey.mock.invocationCallOrder[0],
    );
  });

  it("no add button without a passkey", async () => {
    list();
    render(() => <PasskeysSection />);
    expect(await screen.findByText("No passkeys yet.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Add a passkey" })).toBeNull();
  });

  it("remove steps up for that passkey, deletes and signals", async () => {
    list(ACTIVE, { ...ACTIVE, id: "p-9", label: "Other" });
    api.deletePasskey.mockResolvedValue(REMOVED);
    render(() => <PasskeysSection />);
    fireEvent.click((await screen.findAllByRole("button", { name: "Remove" }))[0]);
    fireEvent.click(screen.getByRole("button", { name: "Remove passkey" }));
    await waitFor(() => expect(signalRemoved).toHaveBeenCalledWith(REMOVED));
    expect(api.stepUpWithPasskey).toHaveBeenCalledWith("passkey.remove", "p-1", expect.any(AbortSignal));
    expect(api.deletePasskey).toHaveBeenCalledWith("p-1");
  });

  it("last passkey removal shows the add-another-first message", async () => {
    list(ACTIVE);
    api.deletePasskey.mockRejectedValue(new PasskeyError("LAST_STRONG_AUTHENTICATOR"));
    render(() => <PasskeysSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove passkey" }));
    expect(await screen.findByText("This is your only passkey. Add another one first.")).toBeTruthy();
    expect(signalRemoved).not.toHaveBeenCalled();
  });

  it("rename validates length", async () => {
    list(ACTIVE);
    api.renamePasskey.mockResolvedValue({ ...ACTIVE, label: "Work" });
    render(() => <PasskeysSection />);
    fireEvent.click(await screen.findByRole("button", { name: "Rename" }));
    const input = screen.getByLabelText("Passkey name");
    fireEvent.input(input, { target: { value: "x".repeat(65) } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("The name must be 1 to 64 characters.")).toBeTruthy();
    expect(api.renamePasskey).not.toHaveBeenCalled();
    fireEvent.input(input, { target: { value: "Work" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(api.renamePasskey).toHaveBeenCalledWith("p-1", "Work"));
  });

  it("desktop shows no add button and the browser notice", async () => {
    tauriMode.on = true;
    list(ACTIVE);
    render(() => <PasskeysSection />);
    expect(await screen.findByText("To add a passkey, open Kaiku in your browser or in the Android app.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Add a passkey" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(screen.getByText("Kaiku opens your browser to confirm with a passkey.")).toBeTruthy();
  });
});
