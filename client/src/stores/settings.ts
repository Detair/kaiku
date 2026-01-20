/**
 * Connection Settings Store
 *
 * Manages user preferences for connection status display with localStorage persistence.
 */

import { createSignal } from "solid-js";

export interface ConnectionSettings {
  displayMode: "circle" | "number";
  showNotifications: boolean;
}

const STORAGE_KEY = "connection-settings";

const defaultSettings: ConnectionSettings = {
  displayMode: "circle",
  showNotifications: true,
};

function loadConnectionSettings(): ConnectionSettings {
  if (typeof localStorage === "undefined") return defaultSettings;
  const stored = localStorage.getItem(STORAGE_KEY);
  if (!stored) return defaultSettings;
  try {
    return { ...defaultSettings, ...JSON.parse(stored) };
  } catch {
    return defaultSettings;
  }
}

const [connectionSettings, setConnectionSettings] = createSignal(
  loadConnectionSettings()
);

export function getConnectionDisplayMode(): "circle" | "number" {
  return connectionSettings().displayMode;
}

export function setConnectionDisplayMode(mode: "circle" | "number"): void {
  const updated = { ...connectionSettings(), displayMode: mode };
  setConnectionSettings(updated);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}

export function getShowNotifications(): boolean {
  return connectionSettings().showNotifications;
}

export function setShowNotifications(show: boolean): void {
  const updated = { ...connectionSettings(), showNotifications: show };
  setConnectionSettings(updated);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(updated));
}

export { connectionSettings };
