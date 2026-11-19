import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  adminRemoveAuthenticator: vi.fn(),
  adminSetMayApprove: vi.fn(),
  adminSetFactors: vi.fn(),
  adminSetRecoveryContact: vi.fn(),
  listAdminSecurityEvents: vi.fn(),
}));
vi.mock("@/lib/tauri/adminSecurity", async (orig) => ({ ...(await orig<object>()), ...api }));
const cancel = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri/security", async (orig) => ({ ...(await orig<object>()), cancelAccountRequest: cancel }));
const stepUp = vi.hoisted(() => vi.fn(async (_req: unknown, run: () => Promise<unknown>) => run()));
vi.mock("@/lib/stepUp", async (orig) => ({ ...(await orig<object>()), withStepUp: stepUp }));
const adminStore = vi.hoisted(() => ({
  adminState: { users: [] as unknown[] },
  loadUserDetails: vi.fn(),
}));
vi.mock("@/stores/admin", () => adminStore);
const toast = vi.hoisted(() => vi.fn());
vi.mock("@/components/ui/Toast", () => ({ showToast: toast }));

import UserSecurityPanel from "../UserSecurityPanel";
import type { UserDetailsResponse } from "@/lib/types";
import { HttpError } from "@/lib/tauri/common";

function detail(over: Partial<UserDetailsResponse> = {}): UserDetailsResponse {
  return {
    id: "oma",
    username: "oma",
    display_name: "Oma",
    email: null,
    avatar_url: null,
    created_at: "",
    is_banned: false,
    last_login: null,
    guild_count: 0,
    guilds: [],
    may_approve: false,
    key_devices: [{ id: "d1", label: "Pixel 8", state: "active" }],
    passkeys: [],
    password_state: "active",
    totp_state: null,
    recovery_codes_left: 5,
    bootstrap_open: false,
    recovery_contact: null,
    open_request: { id: "r1", subject: { user_id: "oma", username: "oma", display_name: "Oma" }, kind: "self", purpose: "recovery", state: "waiting", created_at: "", ready_at: "2026-11-04T08:00:00Z", started_by: null, lost_devices: [], new_authenticator: null, cancel: { via: "step_up" } },
    open_sessions: { full: 1, probation: 2 },
    ...over,
  } as UserDetailsResponse;
}

beforeEach(() => {
  for (const f of Object.values(api)) f.mockReset();
  cancel.mockReset();
  stepUp.mockClear();
  adminStore.loadUserDetails.mockReset();
  toast.mockReset();
  adminStore.adminState.users = [];
  api.listAdminSecurityEvents.mockResolvedValue({ items: [], has_more: false, next_cursor: null });
});
afterEach(cleanup);

describe("UserSecurityPanel", () => {
  it("remove device now steps up for that authenticator", async () => {
    api.adminRemoveAuthenticator.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Remove device now: Pixel 8" }));
    await waitFor(() => expect(api.adminRemoveAuthenticator).toHaveBeenCalledWith("oma", "d1"));
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "admin.authenticator.remove", target: "d1", targetLabel: "Pixel 8" });
  });

  it("marks a removed device, reloads the details and says so (ADMIN-9)", async () => {
    api.adminRemoveAuthenticator.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Remove device now: Pixel 8" }));
    await waitFor(() => expect(adminStore.loadUserDetails).toHaveBeenCalledWith("oma"));
    expect(screen.getByText(/removed/)).toBeTruthy();
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({ type: "success", title: "Pixel 8 was removed" }));
  });

  it("shows a failed removal next to the device (ADMIN-9)", async () => {
    api.adminRemoveAuthenticator.mockRejectedValue(new TypeError("Failed to fetch"));
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Remove device now: Pixel 8" }));
    const alert = await screen.findByRole("alert");
    expect(alert.closest("li")?.textContent).toContain("Pixel 8");
  });

  it("immediate actions are hidden for another admin", () => {
    render(() => <UserSecurityPanel user={detail({ is_admin: true, totp_state: "active" })} meId="me" />);
    expect(screen.queryByRole("button", { name: "Remove device now: Pixel 8" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Mark the password as unsafe" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Mark the authenticator app as unsafe" })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "Recovery contact" })).toBeNull();
    // may_approve is an immediate action too (F6 rev 4): shown, not changeable.
    expect((screen.getByRole("switch", { name: "Phones may confirm sign-ins" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Only that admin or the host CLI can do this.")).toBeTruthy();
  });

  it("the admin's own account keeps its immediate actions", () => {
    render(() => <UserSecurityPanel user={detail({ id: "me", is_admin: true })} meId="me" />);
    expect(screen.getByRole("button", { name: "Remove device now: Pixel 8" })).toBeTruthy();
    expect(screen.queryByText("Only that admin or the host CLI can do this.")).toBeNull();
    // An admin account never gets a recovery contact (C28).
    expect(screen.queryByRole("combobox", { name: "Recovery contact" })).toBeNull();
  });

  it("may_approve toggle steps up with the user as target", async () => {
    api.adminSetMayApprove.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    expect(screen.getByText(/Lets this person's phones confirm sign-ins on other devices and add phones/)).toBeTruthy();
    fireEvent.click(screen.getByRole("switch", { name: "Phones may confirm sign-ins" }));
    await waitFor(() => expect(api.adminSetMayApprove).toHaveBeenCalledWith("oma", true));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "admin.may_approve", target: "oma" });
  });

  it("factor state change steps up", async () => {
    api.adminSetFactors.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Mark the password as unsafe" }));
    await waitFor(() => expect(api.adminSetFactors).toHaveBeenCalledWith("oma", { password: "compromised" }));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "admin.factor.set_state", target: "oma" });
    // An unsafe password leaves only by replacement (B.1): no "Allow again".
    expect(await screen.findByText("Oma must set a new password before it works again.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Allow the password again" })).toBeNull();
  });

  // Codex v1 integration check, major 4: a throttled factor is reset with
  // `active` (C27); "Mark as unsafe" stays an option, never the only one.
  it("a throttled factor is allowed again, not only marked unsafe", async () => {
    api.adminSetFactors.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail({ password_state: "throttled", totp_state: "throttled" })} meId="me" />);
    expect(screen.getByRole("button", { name: "Mark the password as unsafe" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Allow the authenticator app again" }));
    await waitFor(() => expect(api.adminSetFactors).toHaveBeenCalledWith("oma", { totp: "active" }));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "admin.factor.set_state", target: "oma" });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Allow the authenticator app again" })).toBeNull());
    expect(screen.getByRole("button", { name: "Mark the authenticator app as unsafe" })).toBeTruthy();
  });

  // The server refuses `active` for a compromised factor (C27, B.1 exit table).
  it("a compromised factor is never offered back", () => {
    render(() => <UserSecurityPanel user={detail({ password_state: "compromised", totp_state: "compromised" })} meId="me" />);
    expect(screen.queryByRole("button", { name: "Allow the password again" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Allow the authenticator app again" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Mark the password as unsafe" })).toBeNull();
    expect(screen.getByText("Oma must set a new password before it works again.")).toBeTruthy();
    expect(screen.getByText("Oma must set up the authenticator app again before it works again.")).toBeTruthy();
  });

  it("an admin names the recovery contact of a non-admin account (C28)", async () => {
    const person = (id: string, over: Record<string, unknown> = {}) => ({
      id,
      username: id,
      display_name: id[0].toUpperCase() + id.slice(1),
      email: null,
      avatar_url: null,
      created_at: "",
      is_banned: false,
      ...over,
    });
    adminStore.adminState.users = [
      person("paul"),
      person("oma"),
      person("me"),
      person("admin2", { is_admin: true }),
      person("kaiku", { is_system: true }),
      person("gone", { is_banned: true }),
    ];
    api.adminSetRecoveryContact.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    const select = screen.getByRole("combobox", { name: "Recovery contact" }) as HTMLSelectElement;
    const options = Array.from(select.querySelectorAll("option")).map((o) => o.textContent);
    expect(options).toEqual(["Choose a person", "Paul"]);
    fireEvent.change(select, { target: { value: "paul" } });
    fireEvent.click(screen.getByRole("button", { name: "Save recovery contact" }));
    await waitFor(() => expect(api.adminSetRecoveryContact).toHaveBeenCalledWith("oma", "paul"));
    expect(stepUp.mock.calls[0][0]).toEqual({ action: "admin.recovery_contact.set", target: "oma", targetLabel: "Oma" });
    expect(await screen.findByText("Paul")).toBeTruthy();
  });

  it("authenticator states are worded, never shown raw", () => {
    render(() => (
      <UserSecurityPanel
        user={detail({ key_devices: [{ id: "d1", label: "Pixel 8", state: "active" }, { id: "d2", label: "Old", state: "lapsed" }] })}
        meId="me"
      />
    ));
    expect(screen.getByText(/· Active/)).toBeTruthy();
    expect(screen.getByText(/· Unknown state/)).toBeTruthy();
    expect(screen.queryByText(/· active/)).toBeNull();
  });

  it("a cancel that lost against completion says so and reloads the account", async () => {
    cancel.mockRejectedValue(new HttpError(409, "x", "ACCOUNT_REQUEST_COMPLETED"));
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    await waitFor(() => expect(adminStore.loadUserDetails).toHaveBeenCalledWith("oma"));
    expect(toast.mock.calls[0][0]).toMatchObject({
      title: "Too late: the request already completed. If the new device is not theirs, remove it now.",
    });
  });

  it("set up a phone explains the phone-only rule", () => {
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    expect(
      screen.getByText("Phones are set up from the Kaiku app on your own phone: Admin → Users → Oma → Set up a phone."),
    ).toBeTruthy();
  });

  it("an open request can be cancelled with a step-up", async () => {
    cancel.mockResolvedValue({ id: "r1", state: "cancelled" });
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Cancel this request" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("r1", null, null));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "request.cancel", target: "r1" });
  });

  it("admin log uses the user filter", async () => {
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    await waitFor(() => expect(api.listAdminSecurityEvents).toHaveBeenCalledWith("oma", undefined, 50));
  });
});
