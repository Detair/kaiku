/**
 * User settings, preferences, and UI state.
 */

import type { AppSettings, UiState } from "../types";
import { isTauri } from "./common";

/**
 * Tester override for the server address (env `KAIKU_SERVER_URL`, native only).
 * Returns null in the browser or when the variable is unset.
 */
export async function getDefaultServerUrl(): Promise<string | null> {
  if (!isTauri()) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<string | null>("get_default_server_url");
}

// ============================================================================
// Browser-mode persistence (localStorage)
// ============================================================================

/** localStorage key for browser-mode app settings (same shape as native). */
export const BROWSER_SETTINGS_KEY = "kaiku:app-settings";

export function defaultSettings(): AppSettings {
  return {
    audio: {
      input_device: null,
      output_device: null,
      input_volume: 100,
      output_volume: 100,
      noise_suppression: true,
      echo_cancellation: true,
    },
    voice: {
      push_to_talk: false,
      push_to_talk_key: null,
      push_to_talk_release_delay: 200,
      push_to_mute: false,
      push_to_mute_key: null,
      push_to_mute_release_delay: 200,
      voice_activity_detection: true,
      vad_threshold: 0.5,
    },
    theme: "dark",
    notifications_enabled: true,
  };
}

/**
 * Copy only the keys of `defaults` from `stored`, keeping a stored value
 * only when its type matches the default's (null defaults accept string or
 * null). A stale, partial or corrupt value can't break startup.
 */
function mergeKnown<T extends object>(defaults: T, stored: unknown): T {
  const out = { ...defaults } as Record<string, unknown>;
  if (!stored || typeof stored !== "object") return out as T;
  const src = stored as Record<string, unknown>;
  for (const [key, def] of Object.entries(defaults)) {
    const val = src[key];
    if (val === undefined) continue;
    if (def === null) {
      if (val === null || typeof val === "string") out[key] = val;
    } else if (typeof val === typeof def && !(typeof val === "number" && !Number.isFinite(val))) {
      out[key] = val;
    }
  }
  return out as T;
}

function loadBrowserSettings(): AppSettings {
  const defaults = defaultSettings();
  try {
    const raw = localStorage.getItem(BROWSER_SETTINGS_KEY);
    if (!raw) return defaults;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return defaults;
    const p = parsed as Record<string, unknown>;
    const merged = mergeKnown(defaults, p);
    merged.audio = mergeKnown(defaults.audio, p.audio);
    merged.voice = mergeKnown(defaults.voice, p.voice);
    if (merged.theme !== "dark" && merged.theme !== "light") merged.theme = defaults.theme;
    return merged;
  } catch {
    // Unreadable storage (private mode) or corrupt JSON: use defaults.
    return defaults;
  }
}

function saveBrowserSettings(settings: AppSettings): void {
  try {
    localStorage.setItem(BROWSER_SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Storage unavailable or full: settings stay in-memory for this session.
  }
}

export async function getSettings(): Promise<AppSettings> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_settings");
  }

  // Browser mode - stored settings merged over the defaults
  return loadBrowserSettings();
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_settings", { settings });
  }
  saveBrowserSettings(settings);
}

export async function getUiState(): Promise<UiState> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("get_ui_state");
  }
  return { category_collapse: {} };
}

export async function updateCategoryCollapse(
  categoryId: string,
  collapsed: boolean,
): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("update_category_collapse", { categoryId, collapsed });
  }
}
