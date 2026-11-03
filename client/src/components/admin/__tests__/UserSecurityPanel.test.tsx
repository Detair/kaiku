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

import UserSecurityPanel from "../UserSecurityPanel";
import type { UserDetailsResponse } from "@/lib/types";

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

  it("immediate actions are hidden for another admin", () => {
    render(() => <UserSecurityPanel user={detail({ is_admin: true })} meId="me" />);
    expect(screen.queryByRole("button", { name: "Remove device now: Pixel 8" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Mark as unsafe" })).toBeNull();
    expect(screen.getByText("Only that admin or the host CLI can do this.")).toBeTruthy();
  });

  it("may_approve toggle steps up with the user as target", async () => {
    api.adminSetMayApprove.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    expect(screen.getByText("Lets this person's phones confirm sign-ins on other devices and add phones")).toBeTruthy();
    fireEvent.click(screen.getByRole("switch", { name: "Phones may confirm sign-ins" }));
    await waitFor(() => expect(api.adminSetMayApprove).toHaveBeenCalledWith("oma", true));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "admin.may_approve", target: "oma" });
  });

  it("factor state change steps up", async () => {
    api.adminSetFactors.mockResolvedValue(undefined);
    render(() => <UserSecurityPanel user={detail()} meId="me" />);
    fireEvent.click(screen.getByRole("button", { name: "Mark as unsafe" }));
    await waitFor(() => expect(api.adminSetFactors).toHaveBeenCalledWith("oma", { password: "compromised" }));
    expect(stepUp.mock.calls[0][0]).toMatchObject({ action: "admin.factor.set_state", target: "oma" });
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
