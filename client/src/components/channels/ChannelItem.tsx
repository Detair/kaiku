/**
 * ChannelItem - Individual Channel in the Sidebar
 *
 * Displays a single text or voice channel with:
 * - Appropriate icon (# for text, 🔊 for voice)
 * - Selection state highlighting
 * - Voice connection indicator
 * - Settings button on hover (for users with manage permission)
 * - Smooth hover transitions
 */

import { Component, Show, createSignal, createMemo } from "solid-js";
import {
  Hash,
  Volume2,
  Settings,
  BellOff,
  Star,
  CheckCheck,
  Bell,
  StarOff,
  Copy,
} from "lucide-solid";
import type { ChannelWithUnread } from "@/lib/types";
import { isInChannel, getParticipants, voiceState } from "@/stores/voice";
import { authState } from "@/stores/auth";
import { isChannelMuted, setChannelNotificationLevel } from "@/stores/sound";
import { markChannelAsRead } from "@/stores/channels";
import { isFavorited, toggleFavorite } from "@/stores/favorites";
import {
  showContextMenu,
  type ContextMenuEntry,
} from "@/components/ui/ContextMenu";

interface ChannelItemProps {
  channel: ChannelWithUnread;
  isSelected: boolean;
  onClick: () => void;
  /** Callback when settings button is clicked (only shown if provided) */
  onSettings?: () => void;
  /** Guild info for favorites feature */
  guildId?: string;
  guildName?: string;
  guildIcon?: string | null;
}

const ChannelItem: Component<ChannelItemProps> = (props) => {
  const isVoice = () => props.channel.channel_type === "voice";
  const isConnected = () => isInChannel(props.channel.id);
  // Also show as "active" when connecting to this channel
  const isConnecting = () =>
    voiceState.state === "connecting" &&
    voiceState.channelId === props.channel.id;
  const isActive = () => isConnected() || isConnecting();
  const [showTooltip, setShowTooltip] = createSignal(false);

  // Get participants for voice channels (remote participants only, exclude current user)
  const participants = () => {
    if (!isVoice()) return [];
    const currentUserId = authState.user?.id;
    return getParticipants().filter((p) => {
      // Only show participants from connected channel, excluding current user
      return isConnected() && p.user_id !== currentUserId;
    });
  };

  // Total count includes local user when connected
  const participantCount = () => {
    const remoteCount = participants().length;
    // Add 1 for local user when connected to this channel
    return isActive() ? remoteCount + 1 : remoteCount;
  };

  // Check if anyone is speaking in this channel (including local user)
  // Use createMemo for proper reactivity tracking
  const hasSpeakingParticipants = createMemo(() => {
    // Only check speaking when connected (not just connecting)
    if (!isConnected()) return false;
    // Access voiceState.speaking explicitly for reactivity
    const localSpeaking = voiceState.speaking;
    const remoteSpeaking = participants().some((p) => p.speaking);
    return localSpeaking || remoteSpeaking;
  });

  const handleContextMenu = (e: MouseEvent) => {
    const ch = props.channel;
    const muted = isChannelMuted(ch.id);
    const favorited = props.guildId ? isFavorited(ch.id) : false;

    const items: ContextMenuEntry[] = [];

    // Mark as Read (only for text channels with unreads)
    if (ch.channel_type === "text" && ch.unread_count > 0) {
      items.push({
        label: "Mark as Read",
        icon: CheckCheck,
        action: () => {
          markChannelAsRead(ch.id);
        },
      });
    }

    // Mute/Unmute
    items.push({
      label: muted ? "Unmute Channel" : "Mute Channel",
      icon: muted ? Bell : BellOff,
      action: () => {
        setChannelNotificationLevel(ch.id, muted ? "all" : "none");
      },
    });

    // Favorites
    if (props.guildId && props.guildName) {
      items.push({
        label: favorited ? "Remove from Favorites" : "Add to Favorites",
        icon: favorited ? StarOff : Star,
        action: () => {
          toggleFavorite(
            ch.id,
            props.guildId!,
            props.guildName!,
            props.guildIcon ?? null,
            ch.name,
            ch.channel_type as "text" | "voice",
          );
        },
      });
    }

    items.push({ separator: true });

    // Edit Channel (if settings callback exists = user has permission)
    if (props.onSettings) {
      items.push({
        label: "Edit Channel",
        icon: Settings,
        action: () => props.onSettings!(),
      });
    }

    // Copy ID
    items.push({
      label: "Copy Channel ID",
      icon: Copy,
      action: () => navigator.clipboard.writeText(ch.id),
    });

    showContextMenu(e, items);
  };

  return (
    <div class="relative" onContextMenu={handleContextMenu}>
      <div
        role="button"
        tabIndex={0}
        class="w-full flex items-center gap-2 px-2 py-1.5 rounded-xl text-sm transition-all duration-200 group cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-accent-primary/50"
        classList={{
          // Voice connected/connecting state (green glow)
          "bg-accent-primary/10 text-accent-primary border border-accent-primary/30":
            isActive(),
          // Pulsing border while connecting
          "animate-pulse": isConnecting(),
          // Text channel selected state
          "bg-surface-highlight text-text-primary font-medium":
            !isActive() && props.isSelected,
          // Default state
          "text-text-secondary hover:text-text-primary hover:bg-white/5":
            !isActive() && !props.isSelected,
        }}
        onClick={props.onClick}
        onKeyDown={(e) => {
          if (
            e.target === e.currentTarget &&
            (e.key === "Enter" || e.key === " ")
          ) {
            e.preventDefault();
            props.onClick();
          }
        }}
        onMouseEnter={() => setShowTooltip(true)}
        onMouseLeave={() => setShowTooltip(false)}
        title={
          isConnected()
            ? "Click to disconnect"
            : isVoice()
              ? "Click to join voice"
              : undefined
        }
      >
        {/* Channel Icon */}
        <Show
          when={isVoice()}
          fallback={
            <Hash class="w-4 h-4 shrink-0 transition-transform duration-200 group-hover:scale-110" />
          }
        >
          <span
            class="shrink-0 transition-all duration-200"
            classList={{
              "animate-pulse text-accent-primary": hasSpeakingParticipants(),
              "text-current group-hover:scale-110": !hasSpeakingParticipants(),
            }}
          >
            <Volume2 class="w-4 h-4" />
          </span>
        </Show>

        {/* Channel Name */}
        <span
          class="truncate transition-all duration-200"
          classList={{
            "font-semibold": isConnected(),
          }}
        >
          {props.channel.name}
        </span>

        {/* Unread badge for text channels */}
        <Show
          when={
            props.channel.channel_type === "text" &&
            props.channel.unread_count > 0
          }
        >
          <span class="ml-auto flex-shrink-0 min-w-5 h-5 px-1.5 bg-accent-primary text-white text-xs font-bold rounded-full flex items-center justify-center">
            {props.channel.unread_count > 99
              ? "99+"
              : props.channel.unread_count}
          </span>
        </Show>

        {/* Muted indicator */}
        <Show when={isChannelMuted(props.channel.id)}>
          <span title="Notifications muted" class="shrink-0">
            <BellOff class="w-3.5 h-3.5 text-text-muted" />
          </span>
        </Show>

        {/* Favorite star - shown on hover or when favorited */}
        <Show when={props.guildId && props.guildName}>
          <button
            class="p-0.5 rounded transition-all duration-200 shrink-0"
            classList={{
              "text-yellow-400": isFavorited(props.channel.id),
              "text-text-secondary hover:text-yellow-400 opacity-0 group-hover:opacity-100":
                !isFavorited(props.channel.id),
            }}
            onClick={(e) => {
              e.stopPropagation();
              toggleFavorite(
                props.channel.id,
                props.guildId!,
                props.guildName!,
                props.guildIcon ?? null,
                props.channel.name,
                props.channel.channel_type as "text" | "voice",
              );
            }}
            title={
              isFavorited(props.channel.id)
                ? "Remove from favorites"
                : "Add to favorites"
            }
          >
            <Star
              class="w-3.5 h-3.5"
              fill={isFavorited(props.channel.id) ? "currentColor" : "none"}
            />
          </button>
        </Show>

        {/* Participant count for voice channels - always show when there are participants */}
        <Show when={isVoice() && participantCount() > 0}>
          <div class="ml-auto flex items-center gap-1.5">
            <span class="text-xs text-text-secondary font-medium">
              {participantCount()}
            </span>
            {/* Speaking indicator dot when connected */}
            <Show when={isConnected()}>
              <div
                class="w-2 h-2 bg-accent-primary rounded-full"
                classList={{
                  "animate-pulse": hasSpeakingParticipants(),
                }}
              />
            </Show>
          </div>
        </Show>

        {/* Settings button - shown on hover when onSettings provided */}
        <Show when={props.onSettings}>
          <button
            class="p-1 text-text-secondary hover:text-text-primary hover:bg-white/10 rounded opacity-0 group-hover:opacity-100 transition-all duration-200"
            classList={{
              "ml-auto": !isVoice() || participantCount() === 0,
            }}
            onClick={(e) => {
              e.stopPropagation();
              props.onSettings?.();
            }}
            title="Channel Settings"
          >
            <Settings class="w-3.5 h-3.5" />
          </button>
        </Show>
      </div>

      {/* Tooltip showing participants (on hover for voice channels) */}
      <Show when={isVoice() && showTooltip() && participantCount() > 0}>
        <div class="absolute left-full ml-2 top-0 z-50 px-3 py-2 bg-surface-base border border-white/10 rounded-lg shadow-xl min-w-[150px]">
          <p class="text-xs text-text-secondary mb-1">
            {participantCount()}{" "}
            {participantCount() === 1 ? "member" : "members"}
          </p>
          <div class="space-y-0.5">
            {participants()
              .slice(0, 5)
              .map((participant) => (
                <p class="text-sm text-text-primary truncate">
                  {participant.user_id.slice(0, 8)}...
                </p>
              ))}
            <Show when={participantCount() > 5}>
              <p class="text-xs text-text-secondary italic">
                +{participantCount() - 5} more
              </p>
            </Show>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default ChannelItem;
