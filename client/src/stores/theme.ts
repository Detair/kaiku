/**
 * Theme Store
 *
 * Manages theme state with persistence via unified preferences store.
 * Theme data is synced across devices through the preferences system.
 */

import { batch, createEffect, createSignal } from "solid-js";
import { preferences, updatePreference } from "./preferences";
import type { FontSize, ThemeName } from "@/lib/types";

export type { ThemeName, FontSize };

export type ThemeFamily = "standard" | "pixel";

export interface ThemeDefinition {
  id: ThemeName;
  name: string;
  description: string;
  isDark: boolean;
  family: ThemeFamily;
  /** Low-contrast themes: still applied when stored, but not offered in pickers. */
  hidden?: boolean;
  preview: {
    surface: string;
    accent: string;
    text: string;
  };
}

// ============================================================================
// Available Themes
// ============================================================================

/**
 * Available Themes
 *
 * Central registry of all themes. This is the SINGLE SOURCE OF TRUTH
 * for theme definitions. ThemeName type and validation derive from this array.
 *
 * To add a new theme:
 * 1. Add your theme ID string to the THEME_NAMES array in src/lib/types.ts
 * 2. Add a ThemeDefinition entry below
 * 3. Add matching CSS color variables in src/styles/themes.css
 * 4. For pixel themes: set family: "pixel" to inherit structural overrides
 *    from themes-pixel.css (fonts, borders, dithering, etc.)
 *
 * Theme families: Themes with the same family share structural CSS.
 * The pixel-* family uses [data-theme^="pixel-"] selector.
 * Standard themes use the default structural tokens.
 */
export const availableThemes: ThemeDefinition[] = [
  {
    id: "focused-hybrid",
    name: "Focused Hybrid",
    description: "Modern dark theme with high contrast",
    isDark: true,
    family: "standard",
    preview: { surface: "#242933", accent: "#88C0D0", text: "#ECEFF4" },
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    description: "Precision colors for machines and people",
    isDark: true,
    family: "standard",
    hidden: true,
    preview: { surface: "#002b36", accent: "#4fa6ea", text: "#acb9b9" },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    description: "Warm light theme for daytime use",
    isDark: false,
    family: "standard",
    preview: { surface: "#fdf6e3", accent: "#1d689e", text: "#2b434a" },
  },
  {
    id: "pixel-cozy",
    name: "Pixel Cozy",
    description: "Cozy 8-bit RPG aesthetic with warm earth tones",
    isDark: true,
    family: "pixel",
    preview: { surface: "#2C2418", accent: "#7BAE7F", text: "#E8D8C4" },
  },
];

/** Themes offered in pickers (hidden ones excluded). */
export const selectableThemes: ThemeDefinition[] = availableThemes.filter(
  (t) => !t.hidden,
);

/**
 * Themes for a picker: the selectable ones, plus the current theme when it is
 * hidden so a stored choice stays visible and keeps working.
 */
export function pickerThemes(): ThemeDefinition[] {
  const current = availableThemes.find((t) => t.id === storedTheme());
  return current?.hidden ? [...selectableThemes, current] : selectableThemes;
}

/** Theme IDs derived from availableThemes for validation use. */
export const THEME_IDS = availableThemes.map((t) => t.id);

// ============================================================================
// Derived Signals
// ============================================================================

/** The light theme "Same as system" switches to by day. */
export const SYSTEM_LIGHT_THEME: ThemeName = "solarized-light";
/** The dark theme used when the stored choice is a light one. */
export const SYSTEM_DARK_FALLBACK: ThemeName = "focused-hybrid";

/**
 * Which theme is shown. Pure, so public/theme-preload.js (which has to repeat
 * this before the app loads, to avoid a flash) can be tested against it.
 *
 * - Not following the system: the stored theme.
 * - Following the system: by day the light theme; at night the stored theme,
 *   unless that one is light, then the dark fallback.
 */
export function resolveTheme(
  stored: ThemeName,
  followSystem: boolean,
  systemPrefersDark: boolean,
): ThemeName {
  if (!followSystem) return stored;
  if (!systemPrefersDark) return SYSTEM_LIGHT_THEME;
  const isDark = availableThemes.find((t) => t.id === stored)?.isDark ?? true;
  return isDark ? stored : SYSTEM_DARK_FALLBACK;
}

/** Root font-size per step, as a percentage of the browser's own size. */
export const FONT_SIZE_PERCENT: Record<FontSize, number> = {
  default: 100,
  large: 115,
  xlarge: 130,
};

const darkQuery =
  typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)")
    : null;

const [systemPrefersDark, setSystemPrefersDark] = createSignal(
  darkQuery ? darkQuery.matches : true,
);
darkQuery?.addEventListener?.("change", (e) => setSystemPrefersDark(e.matches));

/**
 * The theme the user picked (what the picker marks as selected).
 */
export const storedTheme = () => preferences().theme;

/** True when the theme follows the system light/dark setting. */
export const followSystemTheme = () => preferences().theme_follow_system === true;

/**
 * The theme that is actually shown (what `data-theme` says). Differs from
 * `storedTheme()` only while "Same as system" is on.
 */
export const theme = (): ThemeName =>
  resolveTheme(storedTheme(), followSystemTheme(), systemPrefersDark());

/** Current text size step. */
export const fontSize = (): FontSize => {
  const f = preferences().font_size;
  return f === "large" || f === "xlarge" ? f : "default";
};

function applyToDocument(): void {
  const root = document.documentElement;
  root.setAttribute("data-theme", theme());
  if (fontSize() === "default") root.removeAttribute("data-font-size");
  else root.setAttribute("data-font-size", fontSize());
}

export function applyInitialTheme(): void {
  if (typeof document === "undefined") {
    return;
  }
  if (!storedTheme()) {
    return;
  }
  applyToDocument();
}

// ============================================================================
// Theme Application Effect
// ============================================================================

/**
 * Apply theme and text size to the document whenever they change.
 * This effect runs automatically when preferences are initialized or updated.
 */
createEffect(() => {
  applyToDocument();
});

// ============================================================================
// Theme Functions
// ============================================================================

/**
 * Set the current theme.
 * Updates through the unified preferences store which handles sync.
 * When switching between theme families, applies a brief fade transition.
 */
export function setTheme(newTheme: ThemeName): void {
  const currentDef = getCurrentTheme();
  const newDef = availableThemes.find((t) => t.id === newTheme);
  // Picking a theme by hand ends "Same as system". Both values change in the
  // same step: ending it first would show the stored theme for a moment.
  const apply = () =>
    batch(() => {
      if (followSystemTheme()) updatePreference("theme_follow_system", false);
      updatePreference("theme", newTheme);
    });

  // Cross-family switch: fade out, swap theme, fade in
  if (currentDef && newDef && currentDef.family !== newDef.family) {
    document.documentElement.classList.add("theme-family-transition");
    setTimeout(() => {
      apply();
      requestAnimationFrame(() => {
        document.documentElement.classList.remove("theme-family-transition");
      });
    }, 150);
  } else {
    // Same family: instant switch
    apply();
  }
}

/**
 * Turn "Same as system" on or off. Turning it on keeps the stored theme as the
 * dark choice; the light side is always SYSTEM_LIGHT_THEME.
 */
export function setFollowSystemTheme(on: boolean): void {
  if (followSystemTheme() === on) return;
  updatePreference("theme_follow_system", on);
}

/** Set the text size step. */
export function setFontSize(size: FontSize): void {
  updatePreference("font_size", size);
}

/**
 * Get the current theme definition.
 */
export function getCurrentTheme(): ThemeDefinition | undefined {
  return availableThemes.find((t) => t.id === theme());
}

/**
 * Get the family of a theme by ID.
 */
export function getThemeFamily(themeId: ThemeName): ThemeFamily {
  return availableThemes.find((t) => t.id === themeId)?.family ?? "standard";
}

/**
 * Check if current theme is dark.
 */
export function isDarkTheme(): boolean {
  return getCurrentTheme()?.isDark ?? true;
}

