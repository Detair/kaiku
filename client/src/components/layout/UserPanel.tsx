/**
 * UserPanel - User Info at Bottom of Sidebar
 *
 * Shows current user's avatar, name, username, and settings button.
 * Fixed to the bottom of the sidebar with mt-auto.
 *
 * Voice controls are in VoiceIsland (appears when connected to voice).
 */

import { Component, Show, createSignal } from "solid-js";
import { Settings } from "lucide-solid";
import { authState } from "@/stores/auth";
import Avatar from "@/components/ui/Avatar";
import { SettingsModal } from "@/components/settings";

const UserPanel: Component = () => {
  const user = () => authState.user;
  const [showSettings, setShowSettings] = createSignal(false);

  return (
    <>
      <div class="mt-auto p-3 bg-surface-base/50 border-t border-white/5">
        <div class="flex items-center gap-3">
          {/* User info */}
          <Show when={user()}>
            <div class="flex items-center gap-2.5 flex-1 min-w-0">
              <Avatar
                src={user()!.avatar_url}
                alt={user()!.display_name}
                size="sm"
                status={user()!.status}
                showStatus
              />
              <div class="flex-1 min-w-0">
                <div class="text-sm font-semibold text-text-primary truncate">
                  {user()!.display_name}
                </div>
                <div class="text-xs text-text-secondary truncate">
                  @{user()!.username}
                </div>
              </div>
            </div>
          </Show>

          {/* Action buttons */}
          <button
            class="p-1.5 text-text-secondary hover:text-accent-primary hover:bg-white/10 rounded-lg transition-all duration-200"
            title="User Settings"
            onClick={() => setShowSettings(true)}
          >
            <Settings class="w-4 h-4" />
          </button>
        </div>
      </div>

      {/* Settings Modal */}
      <Show when={showSettings()}>
        <SettingsModal onClose={() => setShowSettings(false)} />
      </Show>
    </>
  );
};

export default UserPanel;
