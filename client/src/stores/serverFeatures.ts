/**
 * Server Features Store
 *
 * Optional features (forum, announcement channels, scheduled events,
 * membership screening) are turned on per server (contract C22). The desktop
 * app hides every entry point of a feature the server has turned off.
 */

import { createSignal } from "solid-js";
import { fetchServerSettings, getServerUrl } from "@/lib/tauri";
import type { Channel, ServerFeatures } from "@/lib/types";

export type OptionalFeatureKey =
  | "forum_channels"
  | "announcement_channels"
  | "scheduled_events"
  | "membership_screening";

// null until loaded. A settings response without `features` (older server)
// means every feature is on.
const [features, setFeatures] = createSignal<ServerFeatures | "all" | null>(null);

const RETRY_DELAYS_MS = [2000, 5000, 15000];
let generation = 0;
let retryTimer: ReturnType<typeof setTimeout> | null = null;

async function attempt(gen: number, retry: number): Promise<void> {
  try {
    const settings = await fetchServerSettings(getServerUrl());
    if (gen !== generation) return; // reset while in flight
    setFeatures(settings.features ?? "all");
  } catch (err) {
    if (gen !== generation) return;
    if (retry < RETRY_DELAYS_MS.length) {
      console.debug("[ServerFeatures] Settings load failed, retrying:", err);
      retryTimer = setTimeout(() => {
        retryTimer = null;
        void attempt(gen, retry + 1);
      }, RETRY_DELAYS_MS[retry]);
    } else {
      console.warn("[ServerFeatures] Could not load server settings:", err);
    }
  }
}

/**
 * Load the feature flags from the server settings. Never throws; a failed
 * load is retried with backoff, and called again on every WebSocket `ready`.
 */
export async function loadServerFeatures(): Promise<void> {
  if (retryTimer) clearTimeout(retryTimer);
  retryTimer = null;
  await attempt(generation, 0);
}

/** False until loaded; true for every key when the server sends no `features`. */
export function isFeatureEnabled(key: OptionalFeatureKey): boolean {
  const f = features();
  if (f === null) return false;
  if (f === "all") return true;
  return f[key] === true;
}

/**
 * True only when the flags are loaded and the server explicitly turned the
 * feature off. While unknown (not yet loaded) nothing is hidden by this, so a
 * slow or failed load does not drop content on a server that has it on.
 */
export function isFeatureOff(key: OptionalFeatureKey): boolean {
  return features() !== null && !isFeatureEnabled(key);
}

/** Forget the loaded flags (sign-out, server switch, tests). */
export function resetServerFeatures(): void {
  generation++;
  if (retryTimer) clearTimeout(retryTimer);
  retryTimer = null;
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

/** A forum/announcement channel whose feature the server turned off. */
export function isHiddenChannel(channel: Pick<Channel, "channel_type">): boolean {
  if (channel.channel_type === "forum") return isFeatureOff("forum_channels");
  if (channel.channel_type === "announcement") return isFeatureOff("announcement_channels");
  return false;
}
