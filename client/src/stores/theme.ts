/**
 * Theme Store - Manages application theme state
 *
 * Supports runtime theme switching with CSS variables via data-theme attribute.
 * Persists theme preference to AppSettings via Tauri API with localStorage fallback.
 */

import { createStore } from "solid-js/store";
import { getSettings, updateSettings } from "@/lib/tauri";

export type ThemeName = "focused-hybrid" | "solarized-dark" | "solarized-light";

/**
 * Theme definition with metadata
 */
export interface ThemeDefinition {
  /** Unique theme identifier */
  id: ThemeName;
  /** Display name */
  name: string;
  /** Theme description */
  description: string;
  /** Whether theme is dark or light */
  isDark: boolean;
}

/**
 * Theme store state
 */
interface ThemeState {
  /** Currently active theme */
  currentTheme: ThemeName;
  /** Available themes */
  availableThemes: ThemeDefinition[];
  /** Whether theme has been initialized */
  isInitialized: boolean;
}

const [themeState, setThemeState] = createStore<ThemeState>({
  currentTheme: "focused-hybrid",
  availableThemes: [
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
      description: "Warm light theme based on solar wavelengths",
      isDark: false,
    },
  ],
  isInitialized: false,
});

/**
 * Initialize theme from persisted settings or system preference
 */
export async function initTheme(): Promise<void> {
  try {
    const settings = await getSettings();
    // Map AppSettings.theme ("dark" | "light") to specific theme
    const theme = settings.theme === "dark" ? "focused-hybrid" : "solarized-light";
    await setTheme(theme);
  } catch {
    // Fallback to localStorage (browser mode)
    const saved = localStorage.getItem("theme") as ThemeName | null;
    await setTheme(saved || "focused-hybrid");
  }

  setThemeState({ isInitialized: true });
}

/**
 * Change theme and persist to settings
 */
export async function setTheme(theme: ThemeName): Promise<void> {
  // Update store
  setThemeState({ currentTheme: theme });

  // Apply to DOM immediately
  document.documentElement.setAttribute("data-theme", theme);

  // Persist to settings
  try {
    const settings = await getSettings();
    const themeDefinition = themeState.availableThemes.find((t) => t.id === theme);
    const isDark = themeDefinition?.isDark ?? true;

    await updateSettings({
      ...settings,
      theme: isDark ? "dark" : "light",
    });
  } catch {
    // Browser mode fallback
    localStorage.setItem("theme", theme);
  }
}

export { themeState };
