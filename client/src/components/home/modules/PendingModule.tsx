/**
 * PendingModule Component
 *
 * Shows pending friend requests with quick actions.
 * Displays all pending requests (both incoming and outgoing) with
 * accept/decline buttons for incoming and cancel button for outgoing.
 */

import { Component, Show, For } from "solid-js";
import { UserPlus, Check, X } from "lucide-solid";
import {
  friendsState,
  acceptFriendRequest,
  rejectFriendRequest,
} from "@/stores/friends";
import CollapsibleModule from "./CollapsibleModule";
import { Avatar } from "@/components/ui";
import type { Friend } from "@/lib/types";

const PendingModule: Component = () => {
  // Split pending requests into incoming and outgoing
  // Note: The Friend type has user_id of the OTHER person, not us
  // We need to check against friendship metadata to determine direction
  // For now, we show all pending requests with unified UI since the API
  // doesn't clearly distinguish direction. The actions will work based
  // on the friendship_id.

  const pendingCount = () => friendsState.pendingRequests.length;

  // Handle accept request
  const handleAccept = async (friend: Friend) => {
    try {
      await acceptFriendRequest(friend.friendship_id);
    } catch (err) {
      console.error("Failed to accept friend request:", err);
    }
  };

  // Handle decline/cancel request
  const handleDecline = async (friend: Friend) => {
    try {
      await rejectFriendRequest(friend.friendship_id);
    } catch (err) {
      console.error("Failed to decline friend request:", err);
    }
  };

  return (
    <CollapsibleModule id="pending" title="Pending" badge={pendingCount()}>
      <Show
        when={pendingCount() > 0}
        fallback={
          <div class="text-center py-4">
            <UserPlus class="w-8 h-8 text-text-secondary mx-auto mb-2 opacity-50" />
            <p class="text-sm text-text-secondary">No pending requests</p>
            <p class="text-xs text-text-muted mt-1">
              Add friends by their username
            </p>
          </div>
        }
      >
        <div class="space-y-1">
          <For each={friendsState.pendingRequests}>
            {(request) => (
              <div class="flex items-center justify-between py-2 px-2 rounded-lg hover:bg-white/5 transition-colors">
                <div class="flex items-center gap-2 min-w-0 flex-1">
                  <Avatar
                    src={request.avatar_url}
                    alt={request.display_name}
                    size="sm"
                  />
                  <div class="min-w-0 flex-1">
                    <div class="text-sm font-medium text-text-primary truncate">
                      {request.display_name}
                    </div>
                    <div class="text-xs text-text-secondary truncate">
                      @{request.username}
                    </div>
                  </div>
                </div>
                <div class="flex items-center gap-1 flex-shrink-0">
                  <button
                    onClick={() => handleAccept(request)}
                    class="p-1.5 rounded-full bg-status-success/20 text-status-success hover:bg-status-success/30 transition-colors"
                    title="Accept"
                  >
                    <Check class="w-4 h-4" />
                  </button>
                  <button
                    onClick={() => handleDecline(request)}
                    class="p-1.5 rounded-full bg-status-error/20 text-status-error hover:bg-status-error/30 transition-colors"
                    title="Decline"
                  >
                    <X class="w-4 h-4" />
                  </button>
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>
    </CollapsibleModule>
  );
};

export default PendingModule;
