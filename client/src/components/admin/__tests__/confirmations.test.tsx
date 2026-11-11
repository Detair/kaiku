/**
 * ADMIN-13: one confirmation dialog instead of confirm() and 3-second
 * "Confirm?" buttons. ADMIN-17: "Registration: Open" asks first.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({
  adminGetAuthSettings: vi.fn(),
  adminUpdateAuthSettings: vi.fn(),
  adminListOidcProviders: vi.fn(),
  adminCreateOidcProvider: vi.fn(),
  adminUpdateOidcProvider: vi.fn(),
  adminDeleteOidcProvider: vi.fn(),
}));
vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { HttpError: common.HttpError, ...api };
});
vi.mock("@/stores/admin", () => ({ adminState: { isElevated: true }, checkAdminStatus: vi.fn() }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@/components/pages/MarkdownPreview", () => ({ default: () => null }));

import AdminSettings from "../AdminSettings";
import PageView from "@/components/pages/PageView";

beforeEach(() => {
  vi.clearAllMocks();
  api.adminGetAuthSettings.mockResolvedValue({ auth_methods: { local: true, oidc: false }, registration_policy: "invite_only" });
  api.adminListOidcProviders.mockResolvedValue([]);
  api.adminUpdateAuthSettings.mockResolvedValue({ auth_methods: { local: true, oidc: false }, registration_policy: "open" });
});
afterEach(cleanup);

describe("AdminSettings registration (ADMIN-17)", () => {
  it("asks before opening registration to everyone", async () => {
    render(() => <AdminSettings />);
    const open = await screen.findByRole("button", { name: /^Open/ });
    fireEvent.click(open);
    expect(api.adminUpdateAuthSettings).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog").textContent).toContain("Let anyone make an account?");
    fireEvent.click(screen.getByRole("button", { name: "Yes, open for everyone" }));
    await waitFor(() =>
      expect(api.adminUpdateAuthSettings).toHaveBeenCalledWith(expect.objectContaining({ registration_policy: "open" })),
    );
  });
});

describe("PageView delete (ADMIN-13)", () => {
  it("asks in a dialog that names the page, not with confirm()", async () => {
    const confirmSpy = vi.spyOn(window, "confirm");
    const onDelete = vi.fn().mockResolvedValue(undefined);
    render(() => (
      <PageView
        page={{ id: "p1", title: "Hausregeln", slug: "hausregeln", content: "", requires_acceptance: false, updated_at: "2026-01-01T00:00:00Z", created_at: "2026-01-01T00:00:00Z" } as never}
        canEdit
        onDelete={onDelete}
      />
    ));
    fireEvent.click(screen.getByRole("button", { name: /Delete/ }));
    expect(confirmSpy).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog").textContent).toContain("Hausregeln");
    fireEvent.click(screen.getByRole("button", { name: "Delete page" }));
    await waitFor(() => expect(onDelete).toHaveBeenCalled());
  });
});
