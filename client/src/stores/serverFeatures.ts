/**
 * Server Features Store
 *
 * Optional features (forum, announcement channels, scheduled events,
 * membership screening) are turned on per server (contract C22). The desktop
 * app hides every entry point of a feature the server has turned off.
 */

import { createSignal } from "solid-js";
import { fetchServerSettings, getServerUrl } from "@/lib/tauri";
import type { ServerFeatures } from "@/lib/types";

export type OptionalFeatureKey =
  | "forum_channels"
  | "announcement_channels"
  | "scheduled_events"
  | "membership_screening";

// null until loaded. A settings response without `features` (older server)
// means every feature is on.
const [features, setFeatures] = createSignal<ServerFeatures | "all" | null>(null);

/** Load the feature flags from the server settings. Never throws. */
export async function loadServerFeatures(): Promise<void> {
  try {
    const settings = await fetchServerSettings(getServerUrl());
    setFeatures(settings.features ?? "all");
  } catch (err) {
    // Stay hidden (not loaded) rather than offering features that may be off.
    console.warn("[ServerFeatures] Could not load server settings:", err);
  }
}

/** False until loaded; true for every key when the server sends no `features`. */
export function isFeatureEnabled(key: OptionalFeatureKey): boolean {
  const f = features();
  if (f === null) return false;
  if (f === "all") return true;
  return f[key] === true;
}

/** Forget the loaded flags (sign-out, server switch, tests). */
export function resetServerFeatures(): void {
  setFeatures(null);
}

/** Feature that gates a permission bit, if any. */
const PERMISSION_FEATURE: Partial<Record<string, OptionalFeatureKey>> = {
  MANAGE_POSTS: "forum_channels",
  SEND_ANNOUNCEMENTS: "announcement_channels",
  MANAGE_EVENTS: "scheduled_events",
};

/** Whether a permission (by key) should be offered in role/override editors. */
export function isPermissionVisible(key: string): boolean {
  const feature = PERMISSION_FEATURE[key];
  return feature === undefined || isFeatureEnabled(feature);
}
