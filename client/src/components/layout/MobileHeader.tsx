/**
 * MobileHeader - Top Navigation Bar for Mobile
 *
 * Displays a hamburger menu button to open the navigation drawer,
 * plus the current guild and channel names for orientation.
 * Only rendered on mobile viewports.
 */

import { Component, Show } from "solid-js";
import { Menu } from "lucide-solid";
import { getActiveGuild } from "@/stores/guilds";
import { selectedChannel } from "@/stores/channels";

interface MobileHeaderProps {
  onMenuClick: () => void;
}

const MobileHeader: Component<MobileHeaderProps> = (props) => {
  const activeGuild = () => getActiveGuild();
  const channel = () => selectedChannel();

  return (
    <header class="h-[44px] flex items-center px-3 gap-3 bg-surface-layer1 border-b border-border-default shrink-0">
      <button
        class="p-2 rounded-lg hover:bg-white/10 transition-colors"
        onClick={props.onMenuClick}
        aria-label="Open navigation"
      >
        <Menu class="w-5 h-5 text-text-primary" />
      </button>
      <div class="flex-1 min-w-0 flex items-center gap-2 text-sm">
        <Show when={activeGuild()}>
          <span class="text-text-secondary truncate">
            {activeGuild()!.name}
          </span>
        </Show>
        <Show when={activeGuild() && channel()}>
          <span class="text-text-muted">/</span>
        </Show>
        <Show when={channel()}>
          <span class="text-text-primary font-medium truncate">
            {channel()!.name}
          </span>
        </Show>
        <Show when={!activeGuild() && !channel()}>
          <span class="text-text-primary font-medium">Kaiku</span>
        </Show>
      </div>
    </header>
  );
};

export default MobileHeader;
