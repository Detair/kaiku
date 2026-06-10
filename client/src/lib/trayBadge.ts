/**
 * System tray unread badge (desktop only).
 *
 * Watches the guild and DM unread state and pushes the total to the native
 * tray via the `tray_set_unread` command. Mounted once from the app layout;
 * a no-op in the browser.
 */
import { createEffect } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { guildsState } from "@/stores/guilds";
import { dmsState } from "@/stores/dms";

const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

/**
 * Total unread across guilds and DMs. Pure and exported for tests.
 */
export function computeTotalUnread(
  guildUnreadCounts: Record<string, number>,
  dms: readonly { unread_count: number }[],
): number {
  const guildTotal = Object.values(guildUnreadCounts).reduce(
    (sum, count) => sum + (count ?? 0),
    0,
  );
  const dmTotal = dms.reduce((sum, dm) => sum + (dm.unread_count ?? 0), 0);
  return guildTotal + dmTotal;
}

/**
 * Start syncing the unread total to the tray badge. Must be called inside
 * a component context (uses createEffect).
 */
export function initTrayBadge(): void {
  if (!isTauri) return;

  createEffect(() => {
    const count = computeTotalUnread(
      guildsState.guildUnreadCounts,
      dmsState.dms,
    );
    invoke("tray_set_unread", { count }).catch((err) => {
      console.warn("[Tray] Failed to update unread badge:", err);
    });
  });
}
