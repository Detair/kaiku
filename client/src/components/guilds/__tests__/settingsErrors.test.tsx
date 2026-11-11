/**
 * ADMIN-6: failed writes in the server settings say so, and saving filters
 * says it worked. ADMIN-13/-24: kicking names the person and asks first.
 */
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";
import { HttpError } from "@/lib/tauri/common";

const m = vi.hoisted(() => ({
  toast: vi.fn(),
  kickMember: vi.fn(),
  assignMemberRole: vi.fn(),
  removeMemberRole: vi.fn(),
  updateFilterConfigs: vi.fn(),
}));

vi.mock("@/components/ui/Toast", () => ({ showToast: m.toast }));
vi.mock("@/stores/auth", () => ({ authState: { user: { id: "me" } } }));
vi.mock("@/stores/guilds", () => ({
  isGuildOwner: (_g: string, u: string) => u === "me",
  kickMember: m.kickMember,
  getGuildMembers: () => [{ user_id: "oma", username: "oma", display_name: "Oma" }],
}));
vi.mock("@/stores/permissions", () => ({
  getGuildRoles: () => [{ id: "r1", name: "Family", position: 1, is_default: false, color: null }],
  getMemberRoleIds: () => [],
  assignMemberRole: m.assignMemberRole,
  removeMemberRole: m.removeMemberRole,
  canManageRole: () => true,
  memberHasPermission: () => true,
  canModerateMember: () => true,
}));
vi.mock("@/lib/api/filters", () => ({
  listFilterConfigs: vi.fn().mockResolvedValue([]),
  updateFilterConfigs: m.updateFilterConfigs,
  listCustomPatterns: vi.fn().mockResolvedValue([]),
  createCustomPattern: vi.fn(),
  deleteCustomPattern: vi.fn(),
  listModerationLog: vi.fn().mockResolvedValue({ items: [], total: 0 }),
  testFilter: vi.fn(),
}));

import MemberRoleDropdown from "../MemberRoleDropdown";
import SafetyTab from "../SafetyTab";

beforeEach(() => vi.clearAllMocks());
afterEach(cleanup);

describe("MemberRoleDropdown", () => {
  it("names the person when a role is given", async () => {
    m.assignMemberRole.mockResolvedValue(undefined);
    render(() => <MemberRoleDropdown guildId="g1" userId="oma" />);
    fireEvent.click(screen.getByTestId("member-role-dropdown-trigger"));
    fireEvent.change(screen.getByTestId("member-role-checkbox"));
    await waitFor(() => expect(m.toast).toHaveBeenCalled());
    expect(m.toast.mock.calls[0][0]).toMatchObject({ type: "success", title: "Oma now has the role Family" });
  });

  it("says why giving a role failed", async () => {
    m.assignMemberRole.mockRejectedValue(new HttpError(403, "x", "FORBIDDEN"));
    render(() => <MemberRoleDropdown guildId="g1" userId="oma" />);
    fireEvent.click(screen.getByTestId("member-role-dropdown-trigger"));
    fireEvent.change(screen.getByTestId("member-role-checkbox"));
    await waitFor(() => expect(m.toast).toHaveBeenCalled());
    expect(m.toast.mock.calls[0][0]).toMatchObject({ type: "error", title: "Could not give the role" });
  });

  it("asks before removing someone, names them, and keeps the reason on failure", async () => {
    m.kickMember.mockRejectedValue(new HttpError(500, "x", "INTERNAL_ERROR"));
    render(() => <MemberRoleDropdown guildId="g1" userId="oma" />);
    fireEvent.click(screen.getByTestId("member-role-dropdown-trigger"));
    fireEvent.click(screen.getByRole("button", { name: "Remove from server" }));
    expect(m.kickMember).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog").textContent).toContain("Remove Oma from the server?");
    const buttons = screen.getAllByRole("button", { name: "Remove from server" });
    fireEvent.click(buttons[buttons.length - 1]);
    expect((await screen.findByRole("alert")).textContent).toContain("Something went wrong on the server");
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
});

describe("SafetyTab", () => {
  it("says the filter settings were saved, or why not", async () => {
    m.updateFilterConfigs.mockResolvedValueOnce([]);
    render(() => <SafetyTab guildId="g1" />);
    const first = (await screen.findAllByRole("button", { name: /^Filter / }))[0];
    fireEvent.click(first!);
    fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(m.toast).toHaveBeenCalledWith(expect.objectContaining({ type: "success", title: "Filter settings saved" })));

    m.updateFilterConfigs.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    fireEvent.click(await screen.findByRole("button", { name: "Save changes" }));
    await waitFor(() =>
      expect(m.toast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "error", title: "Could not save the filter settings", message: "Can't reach the server. Check your connection." }),
      ),
    );
  });
});
