import { describe, expect, it, vi, beforeEach } from "vitest";

const showToast = vi.fn();
vi.mock("@/components/ui/Toast", () => ({ showToast: (...a: unknown[]) => showToast(...a) }));
vi.mock("@/lib/tauri/common", () => ({ isTauri: vi.fn(() => false) }));

import { compareVersions, isBelowMinimum, checkMinClientVersion, resetMinVersionNotice } from "../minClientVersion";

describe("semver precedence", () => {
  it.each([
    ["1.0.0", "1.0.0", 0],
    ["1.0.0-rc.6", "1.0.0", -1],
    ["1.0.0-rc.5", "1.0.0-rc.6", -1],
    ["1.0.0-rc.10", "1.0.0-rc.9", 1],
    ["1.0.0-alpha", "1.0.0-alpha.1", -1],
    ["1.0.0-alpha.1", "1.0.0-alpha.beta", -1],
    ["1.0.0-beta", "1.0.0-alpha.beta", 1],
    ["1.0.0-beta.11", "1.0.0-beta.2", 1],
    ["1.0.0-rc.1", "1.0.0-beta.11", 1],
    ["1.2.0", "1.10.0", -1],
    ["2.0.0", "1.99.99", 1],
    ["1.0.0+build.5", "1.0.0", 0],
    ["v1.0.1", "1.0.0", 1],
  ])("%s vs %s → %i", (a, b, expected) => {
    expect(Math.sign(compareVersions(a, b) ?? NaN)).toBe(expected);
  });

  it("an unparsable version never counts as below", () => {
    expect(compareVersions("dev", "1.0.0")).toBeNull();
    expect(isBelowMinimum("dev", "1.0.0")).toBe(false);
    expect(isBelowMinimum("1.0.0-rc.5", "nonsense")).toBe(false);
  });
});

describe("checkMinClientVersion", () => {
  beforeEach(() => {
    showToast.mockClear();
    resetMinVersionNotice();
  });

  it("missing field (older server) or null minimum: no hint", () => {
    checkMinClientVersion(undefined, "1.0.0-rc.5");
    checkMinClientVersion({ web: null, desktop: null, android: null }, "1.0.0-rc.5");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("below the web minimum: one calm hint with a reload action, not repeated", () => {
    checkMinClientVersion({ web: "1.0.0", desktop: null, android: null }, "1.0.0-rc.5");
    checkMinClientVersion({ web: "1.0.0", desktop: null, android: null }, "1.0.0-rc.5");
    expect(showToast).toHaveBeenCalledTimes(1);
    const toast = showToast.mock.calls[0][0];
    expect(toast.type).toBe("info");
    expect(toast.duration).toBe(0);
    expect(toast.action.label).toBe("Reload");
  });

  it("at or above the minimum: no hint", () => {
    checkMinClientVersion({ web: "1.0.0-rc.5", desktop: null, android: null }, "1.0.0-rc.5");
    expect(showToast).not.toHaveBeenCalled();
  });

  it("desktop uses its own field and points to the updater", async () => {
    const common = await import("@/lib/tauri/common");
    vi.mocked(common.isTauri).mockReturnValue(true);
    checkMinClientVersion({ web: null, desktop: "1.0.0", android: null }, "1.0.0-rc.5");
    expect(showToast.mock.calls[0][0].action.label).toBe("Check for updates");
    vi.mocked(common.isTauri).mockReturnValue(false);
  });
});
