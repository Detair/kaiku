/**
 * ADMIN-1: ban and unban report success only when the server did it; on a
 * failure the dialog stays open and says why. ADMIN-7: the bulk ban names
 * everyone, the selection never covers hidden rows, partial failures show.
 * ADMIN-18: a failed load is not "no users", the search field keeps the filter.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  adminListUsers: vi.fn(),
  adminBanUser: vi.fn(),
  adminUnbanUser: vi.fn(),
  adminBulkBanUsers: vi.fn(),
  adminGetUserDetails: vi.fn(),
  checkAdminStatus: vi.fn(),
}));
vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { HttpError: common.HttpError, ...api };
});
const toast = vi.hoisted(() => vi.fn());
vi.mock("@/components/ui/Toast", () => ({ showToast: toast }));
vi.mock("../UserSecurityPanel", () => ({ default: () => null }));
vi.mock("../AdminRequestsPanel", () => ({ default: () => null }));
vi.mock("../CreateAccountModal", () => ({ default: () => null }));
vi.mock("@/stores/auth", () => ({ currentUser: () => ({ id: "me" }) }));

import UsersPanel from "../UsersPanel";
import { adminState, resetAdminState, selectUser, checkAdminStatus, setUsersSearch } from "@/stores/admin";
import { HttpError } from "@/lib/tauri/common";

const user = (id: string, name: string, banned = false) => ({
  id,
  username: id,
  display_name: name,
  email: null,
  avatar_url: null,
  created_at: "2026-01-01T00:00:00Z",
  is_banned: banned,
});

function page(items: ReturnType<typeof user>[], total = items.length) {
  return { items, total, limit: 20, offset: 0 };
}

beforeEach(async () => {
  for (const f of Object.values(api)) f.mockReset();
  toast.mockReset();
  resetAdminState();
  api.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: true, elevation_expires_at: new Date(Date.now() + 600000).toISOString() });
  await checkAdminStatus();
  api.adminGetUserDetails.mockResolvedValue(null);
});
afterEach(cleanup);

describe("UsersPanel ban (ADMIN-1)", () => {
  it("keeps the dialog open and says admin mode ended when the server refuses", async () => {
    api.adminListUsers.mockResolvedValue(page([user("oma", "Oma")]));
    api.adminBanUser.mockRejectedValue(new HttpError(403, "x", "ELEVATION_REQUIRED"));
    render(() => <UsersPanel />);
    await screen.findByText("Oma");
    selectUser("oma");
    fireEvent.click(await screen.findByRole("button", { name: "Ban" }));
    fireEvent.input(screen.getByRole("textbox", { name: /Reason/ }), { target: { value: "spam" } });
    const dialog = screen.getByTestId("ban-dialog");
    fireEvent.click(Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Ban")!);
    expect((await screen.findByRole("alert")).textContent).toContain("Admin mode has ended");
    expect(screen.getByTestId("ban-dialog")).toBeTruthy();
    expect(toast).not.toHaveBeenCalled();
    expect(adminState.users[0].is_banned).toBe(false);
  });

  it("reports success only after the server banned the person", async () => {
    api.adminListUsers.mockResolvedValue(page([user("oma", "Oma")]));
    api.adminBanUser.mockResolvedValue({ banned: true });
    render(() => <UsersPanel />);
    await screen.findByText("Oma");
    selectUser("oma");
    fireEvent.click(await screen.findByRole("button", { name: "Ban" }));
    fireEvent.input(screen.getByRole("textbox", { name: /Reason/ }), { target: { value: "spam" } });
    const dialog = screen.getByTestId("ban-dialog");
    fireEvent.click(Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Ban")!);
    await waitFor(() => expect(toast).toHaveBeenCalled());
    expect(toast.mock.calls[0][0]).toMatchObject({ type: "success", title: "Oma is banned" });
    expect(screen.queryByTestId("ban-dialog")).toBeNull();
  });

  it("lifting a ban asks first and says why it failed", async () => {
    api.adminListUsers.mockResolvedValue(page([user("oma", "Oma", true)]));
    api.adminUnbanUser.mockResolvedValue({ banned: true });
    render(() => <UsersPanel />);
    await screen.findByText("Oma");
    selectUser("oma");
    fireEvent.click(await screen.findByRole("button", { name: "Lift ban" }));
    const dialog = screen.getByTestId("unban-dialog");
    fireEvent.click(Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === "Lift ban")!);
    expect((await screen.findByRole("alert")).textContent).toContain("That did not happen");
    expect(toast).not.toHaveBeenCalled();
  });
});

describe("UsersPanel bulk ban (ADMIN-7)", () => {
  it("names everyone, and lists who could not be banned", async () => {
    api.adminListUsers.mockResolvedValue(page([user("a", "Anna"), user("b", "Ben")]));
    api.adminBulkBanUsers.mockResolvedValue({ banned_count: 1, already_banned: 0, failed: [{ id: "b", reason: "admin" }] });
    render(() => <UsersPanel />);
    await screen.findByText("Anna");
    fireEvent.click(screen.getByRole("button", { name: "Select Anna" }));
    fireEvent.click(screen.getByRole("button", { name: "Select Ben" }));
    fireEvent.click(screen.getByRole("button", { name: "Ban selected" }));
    const names = screen.getByTestId("bulk-ban-names").textContent ?? "";
    expect(names).toContain("Anna");
    expect(names).toContain("Ben");
    fireEvent.input(screen.getByRole("textbox", { name: /Reason/ }), { target: { value: "spam" } });
    fireEvent.click(screen.getByRole("button", { name: "Ban 2 users" }));
    expect((await screen.findByTestId("bulk-ban-failures")).textContent).toContain("Ben");
    expect(adminState.selectedUserIds.has("b")).toBe(true);
    expect(adminState.selectedUserIds.has("a")).toBe(false);
  });

  it("clears the selection on another page", async () => {
    api.adminListUsers.mockResolvedValue(page([user("a", "Anna")], 40));
    render(() => <UsersPanel />);
    await screen.findByText("Anna");
    fireEvent.click(screen.getByRole("button", { name: "Select Anna" }));
    expect(adminState.selectedUserIds.size).toBe(1);
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(adminState.selectedUserIds.size).toBe(0));
  });
});

describe("UsersPanel loading (ADMIN-18)", () => {
  it("shows a load failure with a retry instead of 'no users'", async () => {
    api.adminListUsers.mockRejectedValueOnce(new HttpError(403, "x", "FORBIDDEN"));
    render(() => <UsersPanel />);
    expect((await screen.findByRole("alert")).textContent).toContain("Could not load the users");
    expect(screen.queryByText("No users yet.")).toBeNull();
    api.adminListUsers.mockResolvedValue(page([user("a", "Anna")]));
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByText("Anna");
  });

  it("keeps the active search in the field when the panel opens again", async () => {
    setUsersSearch("an");
    api.adminListUsers.mockResolvedValue(page([user("a", "Anna")]));
    render(() => <UsersPanel />);
    expect((screen.getByRole("searchbox", { name: "Search users" }) as HTMLInputElement).value).toBe("an");
  });
});
