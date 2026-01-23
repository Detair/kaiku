/**
 * Preferences Store
 *
 * Manages user preferences with cross-device sync via server and localStorage fallback.
 * Preferences include theme, sound settings, quiet hours, connection display, and
 * per-channel notification levels.
 */

import { createSignal } from "solid-js";
import type {
  UserPreferences,
  PreferencesResponse,
  StoredPreferences,
} from "@/lib/types";

// ============================================================================
// Constants
// ============================================================================

const STORAGE_KEY = "vc:preferences";
const DEBOUNCE_MS = 500;

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// ============================================================================
// Default Preferences
// ============================================================================

export const DEFAULT_PREFERENCES: UserPreferences = {
  theme: "focused-hybrid",
  sound: {
    enabled: true,
    volume: 80,
    soundType: "default",
    quietHours: {
      enabled: false,
      startTime: "22:00",
      endTime: "08:00",
    },
  },
  connection: {
    displayMode: "circle",
    showNotifications: true,
  },
  channelNotifications: {},
};

// ============================================================================
// Signals
// ============================================================================

const [preferences, setPreferences] =
  createSignal<UserPreferences>(DEFAULT_PREFERENCES);
const [lastUpdated, setLastUpdated] = createSignal<string>(
  new Date().toISOString()
);
const [isSyncing, setIsSyncing] = createSignal(false);

// ============================================================================
// Debounce Timer
// ============================================================================

let pushTimer: ReturnType<typeof setTimeout> | null = null;

// ============================================================================
// Migration Functions
// ============================================================================

/**
 * Migrate old localStorage keys to new unified format.
 * This handles users who have preferences stored in the old per-store keys.
 *
 * Old keys:
 * - "theme" - Theme selection
 * - "canis:sound:settings" - Sound settings (enabled, volume, selectedSound)
 * - "canis:sound:channels" - Per-channel notification levels
 * - "connection-settings" - Connection display settings
 */
function migrateOldPreferences(): Partial<UserPreferences> | null {
  if (typeof localStorage === "undefined") return null;

  const migrated: Partial<UserPreferences> = {};
  let hasMigration = false;

  // Migrate theme
  const oldTheme = localStorage.getItem("theme");
  if (oldTheme) {
    const validThemes = ["focused-hybrid", "solarized-dark", "solarized-light"];
    if (validThemes.includes(oldTheme)) {
      migrated.theme = oldTheme as UserPreferences["theme"];
      hasMigration = true;
    }
    localStorage.removeItem("theme");
    console.log("[Preferences] Migrated old theme key");
  }

  // Migrate sound settings
  const oldSound = localStorage.getItem("canis:sound:settings");
  if (oldSound) {
    try {
      const parsed = JSON.parse(oldSound);
      migrated.sound = {
        enabled: parsed.enabled ?? DEFAULT_PREFERENCES.sound.enabled,
        volume: parsed.volume ?? DEFAULT_PREFERENCES.sound.volume,
        // Old key was "selectedSound", new key is "soundType"
        soundType: parsed.selectedSound ?? DEFAULT_PREFERENCES.sound.soundType,
        quietHours: DEFAULT_PREFERENCES.sound.quietHours,
      };
      hasMigration = true;
      console.log("[Preferences] Migrated old sound settings key");
    } catch {
      // Invalid JSON, just remove the key
    }
    localStorage.removeItem("canis:sound:settings");
  }

  // Migrate per-channel notifications
  const oldChannelNotifs = localStorage.getItem("canis:sound:channels");
  if (oldChannelNotifs) {
    try {
      const parsed = JSON.parse(oldChannelNotifs);
      // Old format used "none", new format uses "muted"
      const converted: Record<string, "all" | "mentions" | "muted"> = {};
      for (const [channelId, level] of Object.entries(parsed)) {
        if (level === "none") {
          converted[channelId] = "muted";
        } else if (level === "all" || level === "mentions") {
          converted[channelId] = level as "all" | "mentions";
        }
      }
      migrated.channelNotifications = converted;
      hasMigration = true;
      console.log("[Preferences] Migrated old channel notifications key");
    } catch {
      // Invalid JSON, just remove the key
    }
    localStorage.removeItem("canis:sound:channels");
  }

  // Migrate connection settings
  const oldConnection = localStorage.getItem("connection-settings");
  if (oldConnection) {
    try {
      const parsed = JSON.parse(oldConnection);
      migrated.connection = {
        displayMode: parsed.displayMode ?? DEFAULT_PREFERENCES.connection.displayMode,
        showNotifications: parsed.showNotifications ?? DEFAULT_PREFERENCES.connection.showNotifications,
      };
      hasMigration = true;
      console.log("[Preferences] Migrated old connection settings key");
    } catch {
      // Invalid JSON, just remove the key
    }
    localStorage.removeItem("connection-settings");
  }

  return hasMigration ? migrated : null;
}

// ============================================================================
// localStorage Functions
// ============================================================================

/**
 * Load preferences from localStorage.
 */
function loadFromLocalStorage(): StoredPreferences | null {
  if (typeof localStorage === "undefined") return null;

  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      return JSON.parse(stored);
    }
  } catch (e) {
    console.error("[Preferences] Failed to load from localStorage:", e);
  }
  return null;
}

/**
 * Save preferences to localStorage.
 */
function saveToLocalStorage(prefs: UserPreferences, updatedAt: string): void {
  if (typeof localStorage === "undefined") return;

  try {
    const stored: StoredPreferences = { data: prefs, updated_at: updatedAt };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
  } catch (e) {
    console.error("[Preferences] Failed to save to localStorage:", e);
  }
}

// ============================================================================
// API Functions
// ============================================================================

/**
 * Fetch preferences from server.
 * Uses Tauri invoke when available, falls back to HTTP API.
 */
async function fetchPreferences(): Promise<PreferencesResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PreferencesResponse>("fetch_preferences");
  }

  // Browser mode - use HTTP API
  const { fetchApi } = await import("@/lib/tauri");
  return fetchApi<PreferencesResponse>("/api/me/preferences");
}

/**
 * Push preferences to server.
 * Uses Tauri invoke when available, falls back to HTTP API.
 */
async function pushPreferences(
  prefs: UserPreferences
): Promise<PreferencesResponse> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<PreferencesResponse>("update_preferences", {
      preferences: prefs,
    });
  }

  // Browser mode - use HTTP API
  const { fetchApi } = await import("@/lib/tauri");
  return fetchApi<PreferencesResponse>("/api/me/preferences", {
    method: "PUT",
    body: { preferences: prefs },
  });
}

// ============================================================================
// Sync Functions
// ============================================================================

/**
 * Initialize preferences on login.
 * Fetches from server, compares timestamps with localStorage, and syncs.
 */
export async function initPreferences(): Promise<void> {
  setIsSyncing(true);

  // Check for and apply migrations from old localStorage keys
  const migrated = migrateOldPreferences();
  if (migrated) {
    const current = loadFromLocalStorage()?.data ?? DEFAULT_PREFERENCES;
    const merged = { ...current, ...migrated };
    saveToLocalStorage(merged, new Date().toISOString());
    console.log("[Preferences] Applied migrations from old localStorage keys");
  }

  try {
    const local = loadFromLocalStorage();
    const server = await fetchPreferences();

    if (
      !server.preferences ||
      Object.keys(server.preferences).length === 0
    ) {
      // No server prefs, push local (or defaults)
      const toSync = local?.data ?? DEFAULT_PREFERENCES;
      const result = await pushPreferences(toSync);
      const merged = { ...DEFAULT_PREFERENCES, ...result.preferences };
      setPreferences(merged);
      setLastUpdated(result.updated_at);
      saveToLocalStorage(merged, result.updated_at);
      console.log("[Preferences] Pushed local preferences to server");
    } else if (
      !local ||
      new Date(server.updated_at) > new Date(local.updated_at)
    ) {
      // Server is newer, apply
      const merged = { ...DEFAULT_PREFERENCES, ...server.preferences };
      setPreferences(merged);
      setLastUpdated(server.updated_at);
      saveToLocalStorage(merged, server.updated_at);
      console.log("[Preferences] Applied server preferences");
    } else {
      // Local is newer (edited while offline), push
      const result = await pushPreferences(local.data);
      const merged = { ...DEFAULT_PREFERENCES, ...result.preferences };
      setPreferences(merged);
      setLastUpdated(result.updated_at);
      saveToLocalStorage(merged, result.updated_at);
      console.log("[Preferences] Pushed offline changes to server");
    }
  } catch (e) {
    console.error("[Preferences] Failed to init preferences:", e);
    // Fall back to local or defaults
    const local = loadFromLocalStorage();
    if (local) {
      setPreferences(local.data);
      setLastUpdated(local.updated_at);
      console.log("[Preferences] Using local preferences (offline fallback)");
    }
  } finally {
    setIsSyncing(false);
  }
}

/**
 * Update a single preference value.
 * Updates signal and localStorage immediately, then debounces push to server.
 */
export function updatePreference<K extends keyof UserPreferences>(
  key: K,
  value: UserPreferences[K]
): void {
  const updated = { ...preferences(), [key]: value };
  const now = new Date().toISOString();

  setPreferences(updated);
  setLastUpdated(now);
  saveToLocalStorage(updated, now);

  // Debounced push to server
  if (pushTimer) clearTimeout(pushTimer);
  pushTimer = setTimeout(async () => {
    try {
      await pushPreferences(updated);
      console.log("[Preferences] Synced to server");
    } catch (e) {
      console.error("[Preferences] Failed to push preferences:", e);
    }
  }, DEBOUNCE_MS);
}

/**
 * Update a nested preference value.
 * For example: updateNestedPreference("sound", "volume", 50)
 */
export function updateNestedPreference<
  K extends keyof UserPreferences,
  NK extends keyof UserPreferences[K]
>(key: K, nestedKey: NK, value: UserPreferences[K][NK]): void {
  const current = preferences();
  const currentNested = current[key];

  if (typeof currentNested === "object" && currentNested !== null) {
    const updatedNested = { ...currentNested, [nestedKey]: value };
    updatePreference(key, updatedNested as UserPreferences[K]);
  }
}

/**
 * Handle WebSocket preferences_updated event from another device.
 * Compares timestamps and updates if server is newer.
 */
export function handlePreferencesUpdated(event: {
  preferences: Partial<UserPreferences>;
  updated_at: string;
}): void {
  const local = loadFromLocalStorage();

  // Only update if server version is newer than our local version
  if (!local || new Date(event.updated_at) > new Date(local.updated_at)) {
    const merged = { ...DEFAULT_PREFERENCES, ...event.preferences };
    setPreferences(merged);
    setLastUpdated(event.updated_at);
    saveToLocalStorage(merged, event.updated_at);
    console.log("[Preferences] Applied update from another device");
  } else {
    console.log("[Preferences] Ignored older update from server");
  }
}

/**
 * Get a specific channel's notification level.
 */
export function getChannelNotificationLevel(
  channelId: string
): "all" | "mentions" | "muted" {
  return preferences().channelNotifications[channelId] ?? "mentions";
}

/**
 * Set a specific channel's notification level.
 */
export function setChannelNotificationLevel(
  channelId: string,
  level: "all" | "mentions" | "muted"
): void {
  const current = preferences();
  const updatedNotifications = {
    ...current.channelNotifications,
    [channelId]: level,
  };
  updatePreference("channelNotifications", updatedNotifications);
}

/**
 * Check if currently in quiet hours.
 */
export function isInQuietHours(): boolean {
  const sound = preferences().sound;
  if (!sound.quietHours.enabled) return false;

  const now = new Date();
  const currentTime = `${now.getHours().toString().padStart(2, "0")}:${now
    .getMinutes()
    .toString()
    .padStart(2, "0")}`;

  const { startTime, endTime } = sound.quietHours;

  // Handle overnight quiet hours (e.g., 22:00 - 08:00)
  if (startTime > endTime) {
    return currentTime >= startTime || currentTime < endTime;
  }

  // Handle same-day quiet hours (e.g., 09:00 - 17:00)
  return currentTime >= startTime && currentTime < endTime;
}

// ============================================================================
// Exports
// ============================================================================

export { preferences, lastUpdated, isSyncing };
