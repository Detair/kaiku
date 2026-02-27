/**
 * Privacy Settings
 *
 * Controls for activity sharing and other privacy-related preferences.
 */

import { Component, createSignal, onMount } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

const PrivacySettings: Component = () => {
  const [activitySharingEnabled, setActivitySharingEnabled] =
    createSignal(true);
  const [isLoading, setIsLoading] = createSignal(true);

  onMount(async () => {
    try {
      const enabled = await invoke<boolean>("is_activity_sharing_enabled");
      setActivitySharingEnabled(enabled);
    } catch (e) {
      console.error("Failed to get activity sharing status:", e);
    } finally {
      setIsLoading(false);
    }
  });

  const handleActivitySharingChange = async (enabled: boolean) => {
    try {
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

      {/* Activity Status Section */}
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
          <label class="relative inline-flex items-center cursor-pointer">
            <input
              type="checkbox"
              checked={activitySharingEnabled()}
              disabled={isLoading()}
              onChange={(e) =>
                handleActivitySharingChange(e.currentTarget.checked)
              }
              class="sr-only peer"
            />
            <div class="w-11 h-6 bg-white/10 rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-accent-primary peer-disabled:opacity-50 peer-disabled:cursor-not-allowed" />
          </label>
        </div>

        <p class="text-xs text-text-secondary mt-4 pt-4 border-t border-white/10">
          When disabled, your activity status will not be visible to others. You
          can still see the activity status of your friends.
        </p>
      </div>
    </div>
  );
};

export default PrivacySettings;
