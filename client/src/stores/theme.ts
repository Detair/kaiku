/**
 * Theme Store
 *
 * Manages theme state with persistence via settings or localStorage fallback.
 */

import { createStore } from "solid-js/store";
import * as tauri from "@/lib/tauri";

export type ThemeName = "focused-hybrid" | "solarized-dark" | "solarized-light";

export interface ThemeDefinition {
  id: ThemeName;
  name: string;
  description: string;
  isDark: boolean;
}

interface ThemeState {
  currentTheme: ThemeName;
  availableThemes: ThemeDefinition[];
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
      description: "Warm light theme for daytime use",
      isDark: false,
    },
  ],
  isInitialized: false,
});

/**
 * Apply theme to document.
 */
function applyTheme(theme: ThemeName): void {
  document.documentElement.setAttribute("data-theme", theme);
}

/**
 * Initialize theme from settings or localStorage.
 */
export async function initTheme(): Promise<void> {
  if (themeState.isInitialized) return;

  try {
    // Try to get theme from app settings
    const settings = await tauri.getSettings();
    // Map dark/light to specific theme, or use stored preference
    const storedTheme = localStorage.getItem("theme") as ThemeName | null;
    
    let theme: ThemeName;
    if (storedTheme && themeState.availableThemes.some(t => t.id === storedTheme)) {
      // Use specific stored theme preference
      theme = storedTheme;
    } else {
      // Fall back to dark/light preference from settings
      theme = settings.theme === "light" ? "solarized-light" : "focused-hybrid";
    }
    
    setThemeState({ currentTheme: theme, isInitialized: true });
    applyTheme(theme);
  } catch {
    // Fallback to localStorage only
    const saved = localStorage.getItem("theme") as ThemeName | null;
    const theme = saved && themeState.availableThemes.some(t => t.id === saved)
      ? saved
      : "focused-hybrid";
    
    setThemeState({ currentTheme: theme, isInitialized: true });
    applyTheme(theme);
  }
}

/**
 * Set and persist the current theme.
 */
export async function setTheme(theme: ThemeName): Promise<void> {
  setThemeState({ currentTheme: theme });
  applyTheme(theme);
  
  // Persist to localStorage for specific theme preference
  localStorage.setItem("theme", theme);
  
  // Also update settings dark/light preference
  try {
    const settings = await tauri.getSettings();
    const isDark = themeState.availableThemes.find(t => t.id === theme)?.isDark ?? true;
    await tauri.updateSettings({ ...settings, theme: isDark ? "dark" : "light" });
  } catch {
    // Settings update failed, localStorage is the fallback
    console.warn("[Theme] Failed to persist theme to settings");
  }
}

/**
 * Get the current theme definition.
 */
export function getCurrentTheme(): ThemeDefinition | undefined {
  return themeState.availableThemes.find(t => t.id === themeState.currentTheme);
}

/**
 * Check if current theme is dark.
 */
export function isDarkTheme(): boolean {
  return getCurrentTheme()?.isDark ?? true;
}

export { themeState };
