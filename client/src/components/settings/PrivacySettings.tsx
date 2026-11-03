/**
 * Privacy Settings
 *
 * Controls for activity sharing and other privacy-related preferences.
 */

import { Component, Show, createSignal, onMount } from "solid-js";
import { t } from "@/i18n";
import Toggle from "@/components/ui/Toggle";
import { isTauri } from "@/lib/tauri/common";

const PrivacySettings: Component = () => {
  const [activitySharingEnabled, setActivitySharingEnabled] =
    createSignal(true);
  const [isLoading, setIsLoading] = createSignal(true);

  onMount(async () => {
    if (!isTauri()) {
      setIsLoading(false);
      return;
    }

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const enabled = await invoke<boolean>("is_activity_sharing_enabled");
      setActivitySharingEnabled(enabled);
    } catch (e) {
      console.error("Failed to get activity sharing status:", e);
    } finally {
      setIsLoading(false);
    }
  });

  const handleActivitySharingChange = async (enabled: boolean) => {
    if (!isTauri()) {
      setActivitySharingEnabled(enabled);
      return;
    }

    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("set_activity_sharing_enabled", { enabled });
      setActivitySharingEnabled(enabled);
    } catch (e) {
      console.error("Failed to set activity sharing:", e);
      // Revert on error
      setActivitySharingEnabled(!enabled);
    }
  };

  return (
    <div class="space-y-6">
      <h3 class="text-lg font-semibold text-text-primary">Privacy</h3>

      {/* Activity detection is native-only (process scanning) */}
      <Show when={isTauri()} fallback={<p class="text-sm text-text-secondary">{t("settings.privacy.browserNone")}</p>}>
      <div class="bg-surface-base rounded-xl p-4">
        <h4 class="font-medium text-text-primary mb-4">Activity Status</h4>

        <div class="flex items-center justify-between py-2">
          <div class="flex-1 mr-4">
            <div class="font-medium text-text-primary">
              Display current activity
            </div>
            <div class="text-sm text-text-secondary mt-1">
              Show what you're playing or doing to friends
            </div>
          </div>
          <Toggle
            checked={activitySharingEnabled()}
            disabled={isLoading()}
            onChange={(next) => handleActivitySharingChange(next)}
            label="Display current activity"
          />
        </div>

        <p class="text-xs text-text-secondary mt-4 pt-4 border-t border-white/10">
          When disabled, your activity status will not be visible to others. You
          can still see the activity status of your friends.
        </p>
      </div>
      </Show>
    </div>
  );
};

export default PrivacySettings;
