/**
 * Desktop auto-update (Tauri updater plugin).
 *
 * Checks GitHub Releases for a newer signed build shortly after startup.
 * When one exists it downloads in the background and offers a Restart
 * action; declining leaves the update to apply on the next launch.
 * No-op in the browser and in dev builds (the updater plugin only serves
 * release builds).
 */
import { showToast } from "@/components/ui/Toast";

const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

/** Delay before the startup check so it never competes with app boot. */
const STARTUP_CHECK_DELAY_MS = 10_000;

export function initUpdater(): void {
  if (!isTauri) return;

  window.setTimeout(() => {
    void checkForUpdate();
  }, STARTUP_CHECK_DELAY_MS);
}

async function checkForUpdate(): Promise<void> {
  try {
    // Dynamic imports keep the updater plugin out of the browser bundle
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) return;

    console.info(`[Updater] Update available: ${update.version}`);
    showToast({
      type: "info",
      title: `Update available: Kaiku ${update.version}`,
      message: "Downloading in the background…",
      duration: 5000,
      id: "app-update",
    });

    await update.downloadAndInstall();

    const { relaunch } = await import("@tauri-apps/plugin-process");
    showToast({
      type: "success",
      title: `Kaiku ${update.version} is ready`,
      message: "Restart to apply the update.",
      duration: 0,
      id: "app-update",
      action: {
        label: "Restart now",
        onClick: () => {
          void relaunch();
        },
      },
    });
  } catch (err) {
    // Network failures and dev builds (no updater) are expected; log only.
    console.warn("[Updater] Update check failed:", err);
  }
}
