/**
 * Theme Store
 *
 * Manages theme state with persistence via unified preferences store.
 * Theme data is synced across devices through the preferences system.
 */

import { createEffect } from "solid-js";
import { preferences, updatePreference } from "./preferences";

export type ThemeName = "focused-hybrid" | "solarized-dark" | "solarized-light";

export interface ThemeDefinition {
  id: ThemeName;
  name: string;
  description: string;
  isDark: boolean;
}

// ============================================================================
// Constants
// ============================================================================

export const availableThemes: ThemeDefinition[] = [
  {
    id: "focused-hybrid",
    name: "Focused Hybrid",
    description: "Modern dark theme with high contrast",
    isDark: true,
  },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    description: "Precision colors for machines and people",
    isDark: true,
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    description: "Warm light theme for daytime use",
    isDark: false,
  },
];

// ============================================================================
// Derived Signals
// ============================================================================

/**
 * Get the current theme name from preferences.
 */
export const theme = () => preferences().theme;

// ============================================================================
// Theme Application Effect
// ============================================================================

/**
 * Apply theme to document whenever it changes.
 * This effect runs automatically when preferences are initialized or updated.
 */
createEffect(() => {
  const currentTheme = theme();
  document.documentElement.setAttribute("data-theme", currentTheme);
});

// ============================================================================
// Theme Functions
// ============================================================================

/**
 * Set the current theme.
 * Updates through the unified preferences store which handles sync.
 */
export function setTheme(newTheme: ThemeName): void {
  updatePreference("theme", newTheme);
}

/**
 * Get the current theme definition.
 */
export function getCurrentTheme(): ThemeDefinition | undefined {
  return availableThemes.find((t) => t.id === theme());
}

/**
 * Check if current theme is dark.
 */
export function isDarkTheme(): boolean {
  return getCurrentTheme()?.isDark ?? true;
}

/**
 * @deprecated Use initPreferences() from preferences store instead.
 * This is kept for backwards compatibility during migration.
 */
export async function initTheme(): Promise<void> {
  // Theme is now initialized through initPreferences()
  // The createEffect above will apply the theme when preferences load
  console.log(
    "[Theme] initTheme() called - theme is now managed by preferences store"
  );
}

// ============================================================================
// Legacy Compatibility
// ============================================================================

/**
 * @deprecated Access theme() directly or use preferences().theme
 * This object is kept for backwards compatibility.
 */
export const themeState = {
  get currentTheme() {
    return theme();
  },
  availableThemes,
  get isInitialized() {
    // Always true since preferences handle initialization
    return true;
  },
};
