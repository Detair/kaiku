/**
 * FavoritesSection - Expandable Favorites in ServerRail
 *
 * Displays user's favorited channels grouped by guild.
 * Features:
 * - Expandable/collapsible section
 * - Guild headers with icons
 * - Channel items with navigation
 * - Visual feedback for active channel
 */

import { Component, For, Show, createSignal } from "solid-js";
import { Star, ChevronDown, ChevronRight, Hash, Volume2 } from "lucide-solid";
import { favoritesByGuild, isLoading } from "@/stores/favorites";
import { selectGuild } from "@/stores/guilds";
import { selectChannel, channelsState } from "@/stores/channels";

const FavoritesSection: Component = () => {
  const [isExpanded, setIsExpanded] = createSignal(true);

  const handleChannelClick = (guildId: string, channelId: string) => {
    // Navigate to guild and select channel
    selectGuild(guildId);
    selectChannel(channelId);
  };

  const isActiveChannel = (channelId: string) => {
    return channelsState.selectedChannelId === channelId;
  };

  return (
    <Show when={favoritesByGuild().length > 0}>
      <div class="w-full">
        {/* Header */}
        <button
          class="w-full flex items-center gap-2 px-3 py-2 text-xs font-semibold text-text-secondary hover:text-text-primary transition-colors"
          onClick={() => setIsExpanded((prev) => !prev)}
        >
          <Star class="w-3.5 h-3.5 text-yellow-400" />
          <span>Favorites</span>
          <span class="ml-auto">
            <Show
              when={isExpanded()}
              fallback={<ChevronRight class="w-3.5 h-3.5" />}
            >
              <ChevronDown class="w-3.5 h-3.5" />
            </Show>
          </span>
        </button>

        {/* Content */}
        <Show when={isExpanded()}>
          <div class="px-2 pb-2 space-y-2">
            <Show when={isLoading()}>
              <div class="px-2 py-1 text-xs text-text-muted">Loading...</div>
            </Show>

            <For each={favoritesByGuild()}>
              {(group) => (
                <div class="space-y-0.5">
                  {/* Guild Header */}
                  <div class="flex items-center gap-2 px-2 py-1">
                    <Show
                      when={group.guild.icon}
                      fallback={
                        <div class="w-4 h-4 rounded bg-surface-layer2 flex items-center justify-center text-[8px] font-semibold text-text-secondary">
                          {group.guild.name.slice(0, 2).toUpperCase()}
                        </div>
                      }
                    >
                      <img
                        src={group.guild.icon!}
                        alt={group.guild.name}
                        class="w-4 h-4 rounded object-cover"
                      />
                    </Show>
                    <span class="text-xs font-medium text-text-secondary truncate">
                      {group.guild.name}
                    </span>
                  </div>

                  {/* Channels */}
                  <For each={group.channels}>
                    {(channel) => (
                      <button
                        class="w-full flex items-center gap-2 px-2 py-1 rounded text-xs transition-colors"
                        classList={{
                          "bg-surface-highlight text-text-primary":
                            isActiveChannel(channel.channel_id),
                          "text-text-secondary hover:text-text-primary hover:bg-white/5":
                            !isActiveChannel(channel.channel_id),
                        }}
                        onClick={() =>
                          handleChannelClick(
                            channel.guild_id,
                            channel.channel_id,
                          )
                        }
                      >
                        <Show
                          when={channel.channel_type === "voice"}
                          fallback={<Hash class="w-3.5 h-3.5 shrink-0" />}
                        >
                          <Volume2 class="w-3.5 h-3.5 shrink-0" />
                        </Show>
                        <span class="truncate">{channel.channel_name}</span>
                      </button>
                    )}
                  </For>
                </div>
              )}
            </For>
          </div>
        </Show>
      </div>
    </Show>
  );
};

export default FavoritesSection;
