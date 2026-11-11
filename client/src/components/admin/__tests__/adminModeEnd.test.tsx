/**
 * K3: admin mode shows "off" as soon as it ran out, and a server refusal
 * with ELEVATION_REQUIRED switches the display to "off" as well.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@solidjs/testing-library";

const api = vi.hoisted(() => ({ checkAdminStatus: vi.fn() }));
vi.mock("@/lib/tauri", async () => {
  const common = await vi.importActual<typeof import("@/lib/tauri/common")>("@/lib/tauri/common");
  return { HttpError: common.HttpError, ...api };
});

import ConfirmActionDialog from "../ConfirmActionDialog";
import { adminState, checkAdminStatus, resetAdminState, getElevationTimeRemaining, expireElevationIfDue } from "@/stores/admin";
import { HttpError } from "@/lib/tauri/common";

beforeEach(() => resetAdminState());
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("admin mode end", () => {
  it("a refusal with ELEVATION_REQUIRED turns the display off", async () => {
    api.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: true, elevation_expires_at: new Date(Date.now() + 600000).toISOString() });
    await checkAdminStatus();
    expect(adminState.isElevated).toBe(true);
    render(() => (
      <ConfirmActionDialog
        title="Ban"
        message="x"
        confirmLabel="Ban"
        onConfirm={() => Promise.reject(new HttpError(403, "x", "ELEVATION_REQUIRED"))}
        onDone={() => {}}
        onCancel={() => {}}
      />
    ));
    fireEvent.click(screen.getByRole("button", { name: "Ban" }));
    await screen.findByRole("alert");
    await waitFor(() => expect(adminState.isElevated).toBe(false));
    expect(adminState.elevationExpired).toBe(true);
  });

  it("expires right when the time is up, not seconds later", async () => {
    api.checkAdminStatus.mockResolvedValue({ is_admin: true, is_elevated: true, elevation_expires_at: new Date(Date.now() - 1).toISOString() });
    await checkAdminStatus();
    expect(getElevationTimeRemaining()).toBe("0:00");
    expireElevationIfDue();
    expect(adminState.isElevated).toBe(false);
    expect(adminState.elevationExpired).toBe(true);
  });
});
