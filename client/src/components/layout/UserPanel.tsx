import { Component, Show } from "solid-js";
import { Settings, Mic, Headphones } from "lucide-solid";
import { authState, logout } from "@/stores/auth";
import Avatar from "@/components/ui/Avatar";

const UserPanel: Component = () => {
  const user = () => authState.user;

  const handleLogout = async () => {
    await logout();
  };

  return (
    <div class="p-2 bg-background-primary/50">
      <div class="flex items-center gap-2">
        {/* User info */}
        <Show when={user()}>
          <div class="flex items-center gap-2 flex-1 min-w-0">
            <Avatar
              src={user()!.avatar_url}
              alt={user()!.display_name}
              size="sm"
              status={user()!.status}
              showStatus
            />
            <div class="flex-1 min-w-0">
              <div class="text-sm font-medium text-text-primary truncate">
                {user()!.display_name}
              </div>
              <div class="text-xs text-text-muted truncate">
                @{user()!.username}
              </div>
            </div>
          </div>
        </Show>

        {/* Action buttons */}
        <div class="flex items-center gap-0.5">
          <button
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-background-tertiary rounded transition-colors"
            title="Mute"
          >
            <Mic class="w-4 h-4" />
          </button>
          <button
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-background-tertiary rounded transition-colors"
            title="Deafen"
          >
            <Headphones class="w-4 h-4" />
          </button>
          <button
            class="p-1.5 text-text-secondary hover:text-text-primary hover:bg-background-tertiary rounded transition-colors"
            title="Settings"
            onClick={() => {
              // TODO: Open settings modal
              console.log("Settings clicked");
            }}
          >
            <Settings class="w-4 h-4" />
          </button>
        </div>
      </div>
    </div>
  );
};

export default UserPanel;
