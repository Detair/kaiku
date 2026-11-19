/**
 * ADMIN-3/-11/-12: admin mode has one name and says how long it lasts before
 * it is turned on; the badge opens the confirmation directly; no shortcut
 * that does not exist; running out is announced. ADMIN-10: Platform pages
 * shows its panel.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  checkAdminStatus: vi.fn(),
  getAdminStats: vi.fn(),
  adminDeElevate: vi.fn(),
}));
vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { HttpError: common.HttpError, ...api };
});
vi.mock("@solidjs/router", () => ({ useNavigate: () => vi.fn() }));
const elevateOpened = vi.hoisted(() => vi.fn());
vi.mock("@/components/admin/ElevateDialog", () => ({
  default: () => {
    elevateOpened();
    return <div data-testid="elevate-dialog" />;
  },
}));
vi.mock("@/components/admin", async () => {
  const actual = await vi.importActual<typeof import("@/components/admin/AdminSidebar")>("@/components/admin/AdminSidebar");
  const stub = (id: string) => () => <div data-testid={id} />;
  return {
    AdminSidebar: actual.default,
    UsersPanel: stub("users-panel"),
    GuildsPanel: stub("guilds-panel"),
    AuditLogPanel: stub("audit-panel"),
    ReportsPanel: stub("reports-panel"),
    AdminSettings: stub("settings-panel"),
    CommandCenterPanel: stub("cc-panel"),
    PlatformPagesPanel: stub("platform-pages-panel"),
  };
});

import AdminDashboard from "../AdminDashboard";
import { resetAdminState } from "@/stores/admin";

beforeEach(() => {
  resetAdminState();
  for (const f of Object.values(api)) f.mockReset();
  elevateOpened.mockReset();
  api.getAdminStats.mockResolvedValue({ user_count: 3, guild_count: 1, banned_count: 0 });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("AdminDashboard admin mode", () => {
  it("says how long admin mode lasts and opens the confirmation in one click", async () => {
    api.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: false, elevation_expires_at: null });
    render(() => <AdminDashboard />);
    const on = await screen.findByTestId("admin-mode-on");
    expect(on.textContent).toBe("Turn on admin mode (15 min)");
    await waitFor(() => expect(document.body.textContent).toContain("stays on for 15 minutes"));
    expect(document.body.textContent).not.toContain("Ctrl+Shift+A");
    expect(document.body.textContent).not.toMatch(/elevat/i);
    fireEvent.click(on);
    expect(screen.getByTestId("elevate-dialog")).toBeTruthy();
    expect(elevateOpened).toHaveBeenCalledTimes(1);
  });

  it("shows time left, can be turned off, and announces when it ran out", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.checkAdminStatus.mockResolvedValue({
      is_admin: true,
      is_elevated: true,
      elevation_expires_at: new Date(Date.now() + 3000).toISOString(),
    });
    render(() => <AdminDashboard />);
    expect((await screen.findByTestId("admin-mode-status")).textContent).toMatch(/Admin mode on · 0:0\d left/);
    expect(screen.getByRole("button", { name: "Turn off admin mode" })).toBeTruthy();
    await vi.advanceTimersByTimeAsync(5000);
    await waitFor(() => expect(screen.getByTestId("admin-mode-expired").textContent).toContain("Admin mode has ended"));
  });

  it("opens the platform pages panel", async () => {
    api.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: false, elevation_expires_at: null });
    render(() => <AdminDashboard />);
    fireEvent.click(await screen.findByTestId("admin-tab-platform-pages"));
    expect(screen.getByTestId("platform-pages-panel")).toBeTruthy();
  });
});
