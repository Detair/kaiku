import { describe, it, expect, vi } from "vitest";

const status = vi.hoisted(() => vi.fn());
vi.mock("@/lib/tauri", () => ({ checkAdminStatus: () => status() }));

import { adminState, confirmElevation } from "../admin";

describe("confirmElevation", () => {
  it("does not change state for a cancelled attempt", async () => {
    status.mockResolvedValue({ is_admin: true, is_elevated: true, elevation_expires_at: null });
    const ac = new AbortController();
    const p = confirmElevation(ac.signal);
    ac.abort();
    await expect(p).resolves.toBe(false);
    expect(adminState.isElevated).toBe(false);
  });

  it("applies the server answer otherwise and throws on errors", async () => {
    status.mockResolvedValue({ is_admin: true, is_elevated: true, elevation_expires_at: null });
    await expect(confirmElevation()).resolves.toBe(true);
    expect(adminState.isElevated).toBe(true);
    status.mockRejectedValue(new Error("down"));
    await expect(confirmElevation()).rejects.toThrow("down");
  });
});
