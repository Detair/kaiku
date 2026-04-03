/**
 * PendingModule Component
 *
 * Shows pending friend requests with quick actions.
 * Displays all pending requests (both incoming and outgoing) with
 * accept/decline buttons for incoming and cancel button for outgoing.
 */

import { Component, Show, For } from "solid-js";
import { UserPlus } from "lucide-solid";
import {
  friendsState,
  acceptFriendRequest,
  rejectFriendRequest,
  cancelFriendRequest,
} from "@/stores/friends";
import CollapsibleModule from "./CollapsibleModule";
import { Avatar } from "@/components/ui";
import type { Friend } from "@/lib/types";

const PendingModule: Component = () => {
  const pendingCount = () => friendsState.pendingRequests.length;

  // Handle accept request
  const handleAccept = async (friend: Friend) => {
    try {
      await acceptFriendRequest(friend.friendship_id);
    } catch (err) {
      console.error("Failed to accept friend request:", err);
    }
  };

  const handleDecline = async (friend: Friend) => {
    try {
      await rejectFriendRequest(friend.friendship_id);
    } catch (err) {
      console.error("Failed to decline friend request:", err);
    }
  };

  const handleCancel = async (friend: Friend) => {
    try {
      await cancelFriendRequest(friend.friendship_id);
    } catch (err) {
      console.error("Failed to cancel friend request:", err);
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
                  <Show
                    when={request.direction === "incoming"}
                    fallback={
                      <button
                        onClick={() => handleCancel(request)}
                        class="px-2.5 py-1 rounded-lg text-xs font-medium bg-white/5 text-text-secondary hover:bg-status-error/20 hover:text-text-primary transition-colors"
                        title="Cancel request"
                      >
                        Cancel
                      </button>
                    }
                  >
                    <button
                      onClick={() => handleAccept(request)}
                      class="p-1 rounded-full bg-white/5 hover:bg-status-success/20 transition-colors flex items-center justify-center w-8 h-8"
                      title="Accept"
                    >
                      <div style={{ "background-image": "var(--icon-accept)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-80 hover:opacity-100" />
                    </button>
                    <button
                      onClick={() => handleDecline(request)}
                      class="p-1 rounded-full bg-white/5 hover:bg-status-error/20 transition-colors flex items-center justify-center w-8 h-8"
                      title="Decline"
                    >
                      <div style={{ "background-image": "var(--icon-decline)" }} class="w-5 h-5 bg-contain bg-center bg-no-repeat opacity-80 hover:opacity-100" />
                    </button>
                  </Show>
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
