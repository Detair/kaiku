/**
 * DMItem Component
 *
 * Displays a single DM in the sidebar list.
 */

import { Component, Show } from "solid-js";
import { Phone } from "lucide-solid";
import type { DMListItem } from "@/lib/types";
import { dmsState, selectDM } from "@/stores/dms";
import { hasActiveCallInChannel, callState } from "@/stores/call";
import { isUserOnline } from "@/stores/presence";
import { currentUser } from "@/stores/auth";

interface DMItemProps {
  dm: DMListItem;
}

const DMItem: Component<DMItemProps> = (props) => {
  const isSelected = () => dmsState.selectedDMId === props.dm.id;

  // Filter out the current user from participants
  const otherParticipants = () => {
    const me = currentUser();
    return props.dm.participants.filter((p) => p.user_id !== me?.id);
  };

  const isGroupDM = () => otherParticipants().length > 1;

  // Get the other participant(s) for display
  const displayName = () => {
    // For group DMs, use the channel name (custom or auto-generated)
    if (isGroupDM()) {
      return props.dm.name;
    }
    // For 1:1 DMs, show the other participant's display name
    const others = otherParticipants();
    if (others.length === 0) {
      return props.dm.participants[0]?.display_name ?? "Unknown";
    }
    return others[0].display_name;
  };

  // Get online status for 1:1 DMs
  const isOnline = () => {
    if (isGroupDM()) return false;
    const otherUser = otherParticipants()[0];
    if (!otherUser) return false;
    return isUserOnline(otherUser.user_id);
  };

  // Check if there's an active call in this DM
  const hasActiveCall = () => hasActiveCallInChannel(props.dm.id);

  // Check if this is an incoming call (ringing)
  const isIncomingCall = () => {
    const current = callState.currentCall;
    return (
      current.status === "incoming_ringing" &&
      "channelId" in current &&
      current.channelId === props.dm.id
    );
  };

  const formatTimestamp = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return "now";
    if (diffMins < 60) return `${diffMins}m`;
    if (diffHours < 24) return `${diffHours}h`;
    if (diffDays < 7) return `${diffDays}d`;
    return date.toLocaleDateString();
  };

  const lastMessagePreview = () => {
    const msg = props.dm.last_message;
    if (!msg) return "No messages yet";

    const prefix = isGroupDM() ? `${msg.username}: ` : "";
    const content =
      msg.content.length > 30
        ? msg.content.substring(0, 30) + "..."
        : msg.content;
    return prefix + content;
  };

  return (
    <div class="relative group">
      {/* Active Pill */}
      <div
        class="absolute -left-2 top-1/2 -translate-y-1/2 w-1 bg-white rounded-r-full transition-all duration-200"
        style={{ height: isSelected() ? "20px" : "0px" }}
      />
      <button
        onClick={() => selectDM(props.dm.id)}
        class="w-full flex items-start gap-3 p-2 rounded-lg transition-colors text-left"
        classList={{
          "bg-white/10": isSelected(),
          "hover:bg-white/5": !isSelected(),
        }}
      >
        {/* Avatar */}
        <div class="relative flex-shrink-0">
          <Show
            when={props.dm.icon_url}
            fallback={
              <Show
                when={isGroupDM()}
                fallback={
                  <div class="w-10 h-10 rounded-full bg-accent-primary flex items-center justify-center">
                    <span class="text-sm font-semibold text-white">
                      {otherParticipants()[0]
                        ?.display_name?.charAt(0)
                        .toUpperCase() || "?"}
                    </span>
                  </div>
                }
              >
                <div class="w-10 h-10 rounded-full bg-surface-layer2 flex items-center justify-center">
                  <svg
                    class="w-5 h-5 text-text-secondary"
                    fill="currentColor"
                    viewBox="0 0 20 20"
                  >
                    <path d="M13 6a3 3 0 11-6 0 3 3 0 016 0zM18 8a2 2 0 11-4 0 2 2 0 014 0zM14 15a4 4 0 00-8 0v3h8v-3zM6 8a2 2 0 11-4 0 2 2 0 014 0zM16 18v-3a5.972 5.972 0 00-.75-2.906A3.005 3.005 0 0119 15v3h-3zM4.75 12.094A5.973 5.973 0 004 15v3H1v-3a3 3 0 013.75-2.906z" />
                  </svg>
                </div>
              </Show>
            }
          >
            <div class="w-10 h-10 rounded-full overflow-hidden">
              <img
                src={props.dm.icon_url!}
                alt="DM avatar"
                class="w-full h-full object-cover"
              />
            </div>
          </Show>

          {/* Online indicator for 1:1 DMs */}
          <Show when={!isGroupDM() && isOnline()}>
            <div class="absolute bottom-0 right-0 w-3 h-3 bg-green-500 border-2 border-surface-base rounded-full" />
          </Show>

          {/* Call indicator */}
          <Show when={hasActiveCall()}>
            <div
              class="absolute -bottom-0.5 -right-0.5 w-4 h-4 rounded-full flex items-center justify-center"
              classList={{
                "bg-green-500 animate-pulse": isIncomingCall(),
                "bg-green-500": !isIncomingCall(),
              }}
              title={isIncomingCall() ? "Incoming call" : "Active call"}
            >
              <Phone class="w-2.5 h-2.5 text-white" />
            </div>
          </Show>
        </div>

        {/* Content */}
        <div class="flex-1 min-w-0">
          <div class="flex items-center justify-between gap-2">
            <span class="font-medium text-text-primary truncate">
              {displayName()}
            </span>
            <Show when={props.dm.last_message}>
              <span class="text-xs text-text-secondary flex-shrink-0">
                {formatTimestamp(props.dm.last_message!.created_at)}
              </span>
            </Show>
          </div>

          <div class="flex items-center gap-2">
            <span class="text-sm text-text-secondary truncate flex-1">
              {lastMessagePreview()}
            </span>

            {/* Unread badge */}
            <Show when={props.dm.unread_count > 0}>
              <span class="flex-shrink-0 min-w-5 h-5 px-1.5 bg-accent-primary text-white text-xs font-bold rounded-full flex items-center justify-center">
                {props.dm.unread_count > 99 ? "99+" : props.dm.unread_count}
              </span>
            </Show>
          </div>
        </div>
      </button>
    </div>
  );
};

export default DMItem;
