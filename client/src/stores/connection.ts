/**
 * Connection Settings Store
 *
 * Manages user preferences for connection status display through the unified preferences store.
 * Connection settings are synced across devices through the preferences system.
 */

import { preferences, updateNestedPreference } from "./preferences";

// ============================================================================
// Types
// ============================================================================

export interface ConnectionSettings {
  displayMode: "circle" | "number";
  showNotifications: boolean;
}

// ============================================================================
// Derived Signals
// ============================================================================

/**
 * Get connection settings from preferences.
 */
export const connectionSettings = (): ConnectionSettings => {
  const connection = preferences().connection;
  return {
    displayMode: connection.displayMode,
    showNotifications: connection.showNotifications,
  };
};

// ============================================================================
// Connection Settings Functions
// ============================================================================

export function getConnectionDisplayMode(): "circle" | "number" {
  return preferences().connection.displayMode;
}

export function setConnectionDisplayMode(mode: "circle" | "number"): void {
  updateNestedPreference("connection", "displayMode", mode);
}

export function getShowNotifications(): boolean {
  return preferences().connection.showNotifications;
}

export function setShowNotifications(show: boolean): void {
  updateNestedPreference("connection", "showNotifications", show);
}
