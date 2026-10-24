/**
 * Desktop updater: one update check per app session, however often the
 * layout that calls `initUpdater()` remounts.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { check } = vi.hoisted(() => ({ check: vi.fn(async () => null) }));

vi.mock("@/lib/tauri/common", () => ({ isTauri: () => true }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

describe("initUpdater", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    check.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("initUpdater twice schedules one check", async () => {
    const { initUpdater } = await import("../updater");

    initUpdater();
    initUpdater();
    await vi.runAllTimersAsync();

    expect(check).toHaveBeenCalledTimes(1);
  });
});
