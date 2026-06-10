/**
 * Tests for the system tray unread badge total (desktop tray feature).
 */
import { describe, it, expect, vi } from "vitest";

vi.mock("@/stores/guilds", () => ({
  guildsState: { guildUnreadCounts: {} },
}));
vi.mock("@/stores/dms", () => ({
  dmsState: { dms: [] },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { computeTotalUnread } from "../trayBadge";

describe("computeTotalUnread", () => {
  it("returns 0 with no unreads anywhere", () => {
    expect(computeTotalUnread({}, [])).toBe(0);
  });

  it("sums guild unread counts", () => {
    expect(computeTotalUnread({ g1: 3, g2: 2 }, [])).toBe(5);
  });

  it("sums DM unread counts", () => {
    expect(
      computeTotalUnread({}, [{ unread_count: 1 }, { unread_count: 4 }]),
    ).toBe(5);
  });

  it("combines guild and DM unreads", () => {
    expect(
      computeTotalUnread({ g1: 2 }, [{ unread_count: 3 }, { unread_count: 0 }]),
    ).toBe(5);
  });

  it("tolerates missing counts", () => {
    expect(
      computeTotalUnread(
        { g1: undefined as unknown as number },
        [{ unread_count: undefined as unknown as number }],
      ),
    ).toBe(0);
  });
});
