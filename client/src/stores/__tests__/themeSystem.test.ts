/**
 * "Same as my device", the light theme and the text size (A11Y-10, A11Y-5).
 * Includes a parity check of public/theme-preload.js, which repeats the theme
 * rules before the app loads so nothing flashes.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const prefs = vi.hoisted(() => ({ current: {} as Record<string, unknown> }));

vi.mock("../preferences", () => ({
  preferences: () => prefs.current,
  updatePreference: vi.fn((key: string, value: unknown) => {
    prefs.current = { ...prefs.current, [key]: value };
  }),
}));

import {
  availableThemes,
  selectableThemes,
  pickerThemes,
  resolveTheme,
  theme,
  storedTheme,
  followSystemTheme,
  fontSize,
  setTheme,
  setFollowSystemTheme,
  setFontSize,
  applyInitialTheme,
  FONT_SIZE_PERCENT,
  THEME_IDS,
} from "../theme";
import { updatePreference } from "../preferences";

beforeEach(() => {
  prefs.current = { theme: "focused-hybrid" };
  vi.mocked(updatePreference).mockClear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-font-size");
});

describe("light theme is selectable (A11Y-10)", () => {
  it("offers solarized-light in the picker", () => {
    expect(selectableThemes.map((t) => t.id)).toContain("solarized-light");
    expect(availableThemes.find((t) => t.id === "solarized-light")?.isDark).toBe(false);
  });

  it("keeps the default for people who never chose", () => {
    prefs.current = { theme: "focused-hybrid" };
    expect(theme()).toBe("focused-hybrid");
    expect(followSystemTheme()).toBe(false);
    expect(fontSize()).toBe("default");
  });

  it("still shows a hidden stored theme in the picker", () => {
    prefs.current = { theme: "solarized-dark" };
    expect(pickerThemes().map((t) => t.id)).toContain("solarized-dark");
  });
});

describe("resolveTheme", () => {
  it("returns the stored theme when not following the system", () => {
    for (const id of THEME_IDS) {
      expect(resolveTheme(id, false, true)).toBe(id);
      expect(resolveTheme(id, false, false)).toBe(id);
    }
  });

  it("uses the light theme by day", () => {
    for (const id of THEME_IDS) expect(resolveTheme(id, true, false)).toBe("solarized-light");
  });

  it("uses the stored dark theme at night", () => {
    expect(resolveTheme("pixel-cozy", true, true)).toBe("pixel-cozy");
    expect(resolveTheme("solarized-dark", true, true)).toBe("solarized-dark");
    expect(resolveTheme("focused-hybrid", true, true)).toBe("focused-hybrid");
  });

  it("falls back to focused-hybrid at night when the stored theme is light", () => {
    expect(resolveTheme("solarized-light", true, true)).toBe("focused-hybrid");
  });
});

describe("store behaviour", () => {
  it("follows the system when asked and keeps the stored theme", () => {
    setFollowSystemTheme(true);
    expect(updatePreference).toHaveBeenCalledWith("theme_follow_system", true);
    expect(followSystemTheme()).toBe(true);
    expect(storedTheme()).toBe("focused-hybrid");
  });

  it("picking a theme by hand ends 'same as device'", () => {
    prefs.current = { theme: "focused-hybrid", theme_follow_system: true };
    setTheme("solarized-light");
    expect(updatePreference).toHaveBeenCalledWith("theme_follow_system", false);
    expect(updatePreference).toHaveBeenCalledWith("theme", "solarized-light");
  });

  it("stores the text size and ignores unknown values", () => {
    setFontSize("large");
    expect(fontSize()).toBe("large");
    prefs.current = { theme: "focused-hybrid", font_size: "gigantic" };
    expect(fontSize()).toBe("default");
  });

  it("writes data-theme and data-font-size on the root", () => {
    prefs.current = { theme: "solarized-light", font_size: "xlarge" };
    applyInitialTheme();
    expect(document.documentElement.getAttribute("data-theme")).toBe("solarized-light");
    expect(document.documentElement.getAttribute("data-font-size")).toBe("xlarge");
  });

  it("has three text sizes: 100, 115 and 130 percent", () => {
    expect(FONT_SIZE_PERCENT).toEqual({ default: 100, large: 115, xlarge: 130 });
  });
});

describe("public/theme-preload.js matches resolveTheme (no flash)", () => {
  const src = readFileSync(join(__dirname, "..", "..", "..", "public", "theme-preload.js"), "utf8");

  function runPreload(data: Record<string, unknown> | null, systemDark: boolean) {
    const attrs: Record<string, string> = {};
    const doc = {
      documentElement: { setAttribute: (k: string, v: string) => (attrs[k] = v) },
      createElement: () => ({ setAttribute() {} }) as Record<string, unknown>,
      head: { appendChild() {} },
    };
    const ls = {
      getItem: (k: string) =>
        k === "vc:preferences" && data ? JSON.stringify({ data, updated_at: "x" }) : null,
    };
    const win = { matchMedia: () => ({ matches: systemDark }) };
    new Function("localStorage", "document", "window", src)(ls, doc, win);
    return attrs;
  }

  const cases = THEME_IDS.flatMap((id) =>
    [true, false].flatMap((follow) =>
      [true, false].map((dark) => [id, follow, dark] as const),
    ),
  );

  it.each(cases)("%s, follow=%s, system dark=%s", (id, follow, dark) => {
    const attrs = runPreload({ theme: id, theme_follow_system: follow }, dark);
    expect(attrs["data-theme"]).toBe(resolveTheme(id, follow, dark));
  });

  it("sets the text size before the app loads", () => {
    expect(runPreload({ theme: "focused-hybrid", font_size: "large" }, true)["data-font-size"]).toBe("large");
    expect(runPreload({ theme: "focused-hybrid", font_size: "xlarge" }, true)["data-font-size"]).toBe("xlarge");
    expect(runPreload({ theme: "focused-hybrid" }, true)["data-font-size"]).toBeUndefined();
  });

  it("uses the default theme when nothing is stored", () => {
    expect(runPreload(null, false)["data-theme"]).toBe("focused-hybrid");
  });

  it("never throws on broken stored data", () => {
    const doc = { documentElement: { setAttribute() {} }, createElement: () => ({ setAttribute() {} }), head: { appendChild() {} } };
    const ls = { getItem: () => "{not json" };
    expect(() => new Function("localStorage", "document", "window", src)(ls, doc, {})).not.toThrow();
  });
});
