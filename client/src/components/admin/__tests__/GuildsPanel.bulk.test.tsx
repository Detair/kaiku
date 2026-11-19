/**
 * W1 / Codex Major 2: the server bulk suspend sends exactly the servers the
 * dialog named. Selecting on page 1, then on page 2, then suspending must
 * not hit the page-1 servers nobody can see any more.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  adminListGuilds: vi.fn(),
  adminBulkSuspendGuilds: vi.fn(),
  adminGetGuildDetails: vi.fn(),
  checkAdminStatus: vi.fn(),
}));
vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { HttpError: common.HttpError, ...api };
});
const toast = vi.hoisted(() => vi.fn());
vi.mock("@/components/ui/Toast", () => ({ showToast: toast }));

import GuildsPanel from "../GuildsPanel";
import { adminState, resetAdminState, checkAdminStatus, toggleGuildSelection, loadGuilds } from "@/stores/admin";

const g = (id: string, name: string) => ({
  id, name, owner_id: "o", owner_username: "o", icon_url: null, member_count: 1,
  created_at: "2026-01-01T00:00:00Z", suspended_at: null,
});
const page = (items: ReturnType<typeof g>[]) => ({ items, total: 40, limit: 20, offset: 0 });

beforeEach(async () => {
  for (const f of Object.values(api)) f.mockReset();
  toast.mockReset();
  resetAdminState();
  api.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: true, elevation_expires_at: new Date(Date.now() + 600000).toISOString() });
  api.adminGetGuildDetails.mockResolvedValue(null);
  await checkAdminStatus();
});
afterEach(cleanup);

describe("GuildsPanel bulk suspend", () => {
  it("select on page 1, select on page 2, suspend: only the named servers are sent", async () => {
    api.adminListGuilds.mockImplementation(async (_l: number, offset: number) =>
      offset === 0 ? page([g("a", "Alpha"), g("b", "Beta")]) : page([g("c", "Gamma")]),
    );
    api.adminBulkSuspendGuilds.mockResolvedValue({ suspended_count: 1, already_suspended: 0, failed: [] });
    render(() => <GuildsPanel />);
    await screen.findByText("Alpha");
    toggleGuildSelection("a");
    toggleGuildSelection("b");
    await loadGuilds(2);
    await screen.findByText("Gamma");
    toggleGuildSelection("c");
    fireEvent.click(screen.getByRole("button", { name: /Suspend selected/ }));
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("Gamma");
    expect(dialog.textContent).not.toContain("Alpha");
    fireEvent.input(dialog.querySelector("textarea")!, { target: { value: "spam" } });
    fireEvent.click(screen.getByRole("button", { name: "Suspend 1 server" }));
    await waitFor(() => expect(api.adminBulkSuspendGuilds).toHaveBeenCalled());
    expect(api.adminBulkSuspendGuilds.mock.calls[0][0]).toEqual(["c"]);
  });

  it("names the servers that could not be suspended", async () => {
    api.adminListGuilds.mockResolvedValue(page([g("a", "Alpha"), g("b", "Beta")]));
    api.adminBulkSuspendGuilds.mockResolvedValue({ suspended_count: 1, already_suspended: 0, failed: [{ id: "b", reason: "x" }] });
    render(() => <GuildsPanel />);
    await screen.findByText("Alpha");
    toggleGuildSelection("a");
    toggleGuildSelection("b");
    fireEvent.click(screen.getByRole("button", { name: /Suspend selected/ }));
    const dialog = screen.getByRole("dialog");
    fireEvent.input(dialog.querySelector("textarea")!, { target: { value: "spam" } });
    fireEvent.click(screen.getByRole("button", { name: "Suspend 2 servers" }));
    await waitFor(() => expect(toast).toHaveBeenCalled());
    expect(toast.mock.calls[0][0].message).toContain("Beta");
    expect(adminState.selectedGuildIds.has("b")).toBe(true);
  });
});
