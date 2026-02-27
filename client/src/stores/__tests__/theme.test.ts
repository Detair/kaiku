import { describe, it, expect } from "vitest";
import {
  availableThemes,
  THEME_IDS,
  getCurrentTheme,
  getThemeFamily,
  type ThemeFamily,
} from "../theme";
import { THEME_NAMES } from "@/lib/types";

describe("theme store", () => {
  describe("theme definitions", () => {
    it("should have at least one theme defined", () => {
      expect(availableThemes.length).toBeGreaterThan(0);
    });

    it("should have matching THEME_IDS and THEME_NAMES arrays", () => {
      // Every theme ID should be in THEME_NAMES
      for (const id of THEME_IDS) {
        expect(THEME_NAMES).toContain(id);
      }
      // Every THEME_NAME should be in THEME_IDS
      for (const name of THEME_NAMES) {
        expect(THEME_IDS).toContain(name);
      }
    });

    it("should have unique theme IDs", () => {
      const ids = availableThemes.map((t) => t.id);
      const uniqueIds = new Set(ids);
      expect(uniqueIds.size).toBe(ids.length);
    });
  });

  describe("theme completeness", () => {
    it.each(availableThemes)(
      "theme '$name' should have all required fields",
      (theme) => {
        expect(theme.id).toBeDefined();
        expect(typeof theme.id).toBe("string");
        expect(theme.id.length).toBeGreaterThan(0);

        expect(theme.name).toBeDefined();
        expect(typeof theme.name).toBe("string");

        expect(theme.description).toBeDefined();
        expect(typeof theme.description).toBe("string");

        expect(typeof theme.isDark).toBe("boolean");
      },
    );

    it.each(availableThemes)(
      "theme '$name' should have valid family",
      (theme) => {
        const validFamilies: ThemeFamily[] = ["standard", "pixel"];
        expect(validFamilies).toContain(theme.family);
      },
    );

    it.each(availableThemes)(
      "theme '$name' should have complete preview colors",
      (theme) => {
        expect(theme.preview).toBeDefined();
        expect(theme.preview.surface).toBeDefined();
        expect(theme.preview.accent).toBeDefined();
        expect(theme.preview.text).toBeDefined();

        // Verify colors are valid hex format
        const hexPattern = /^#[0-9A-Fa-f]{6}$/;
        expect(theme.preview.surface).toMatch(hexPattern);
        expect(theme.preview.accent).toMatch(hexPattern);
        expect(theme.preview.text).toMatch(hexPattern);
      },
    );
  });

  describe("theme families", () => {
    it("should return correct family for standard themes", () => {
      expect(getThemeFamily("focused-hybrid")).toBe("standard");
      expect(getThemeFamily("solarized-dark")).toBe("standard");
      expect(getThemeFamily("solarized-light")).toBe("standard");
    });

    it("should return correct family for pixel themes", () => {
      expect(getThemeFamily("pixel-cozy")).toBe("pixel");
    });

    it("should return 'standard' for unknown theme IDs", () => {
      // Cast to bypass TypeScript check for invalid theme
      expect(getThemeFamily("nonexistent" as any)).toBe("standard");
    });
  });

  describe("pixel theme naming convention", () => {
    it("pixel themes should have IDs starting with 'pixel-'", () => {
      const pixelThemes = availableThemes.filter((t) => t.family === "pixel");
      for (const theme of pixelThemes) {
        expect(theme.id).toMatch(/^pixel-/);
      }
    });

    it("standard themes should not have IDs starting with 'pixel-'", () => {
      const standardThemes = availableThemes.filter(
        (t) => t.family === "standard",
      );
      for (const theme of standardThemes) {
        expect(theme.id).not.toMatch(/^pixel-/);
      }
    });
  });

  describe("getCurrentTheme", () => {
    it("should return theme definition for current theme", () => {
      const current = getCurrentTheme();
      expect(current).toBeDefined();
      expect(availableThemes).toContain(current);
    });
  });
});
