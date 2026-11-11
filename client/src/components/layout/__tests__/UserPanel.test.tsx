// NAV-9, KONTO-7: signing out asks first; Cancel keeps the user signed in.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";

const mocks = vi.hoisted(() => ({ logout: vi.fn() }));

vi.mock("@/stores/auth", () => ({
  authState: {
    user: { id: "me", username: "oma", display_name: "Oma", avatar_url: null, status: "online" },
  },
  logout: mocks.logout,
}));
vi.mock("@/stores/admin", () => ({
  adminState: { isAdmin: false, isElevated: false },
  checkAdminStatus: vi.fn(),
}));
vi.mock("@/stores/presence", () => ({
  getUserPresence: () => undefined,
  setMyCustomStatus: vi.fn(),
}));

import UserPanel from "../UserPanel";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("UserPanel sign out", () => {
  it("asks before signing out and does nothing on Cancel", async () => {
    render(() => <UserPanel />);
    fireEvent.click(screen.getByTestId("logout-button"));
    expect(mocks.logout).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("Sign out of Kaiku?");
    await vi.waitFor(() =>
      expect(document.activeElement?.textContent).toBe("Cancel"),
    );
    fireEvent.click(screen.getByText("Cancel"));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(mocks.logout).not.toHaveBeenCalled();
  });

  it("signs out after confirming", () => {
    render(() => <UserPanel />);
    fireEvent.click(screen.getByTestId("logout-button"));
    fireEvent.click(screen.getByTestId("logout-confirm"));
    expect(mocks.logout).toHaveBeenCalledTimes(1);
  });
});
