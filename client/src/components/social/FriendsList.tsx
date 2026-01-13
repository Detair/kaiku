/**
 * Friends List Component
 *
 * Displays friends with tabs for Online, All, Pending, and Blocked.
 */

import { Component, createSignal, For, onMount, Show } from "solid-js";
import {
  friendsState,
  loadFriends,
  loadPendingRequests,
  loadBlocked,
  getOnlineFriends,
  acceptFriendRequest,
  rejectFriendRequest,
  removeFriend,
} from "@/stores/friends";
import type { Friend } from "@/lib/types";
import AddFriend from "./AddFriend";

type FriendsTab = "online" | "all" | "pending" | "blocked";

const FriendsList: Component = () => {
  const [tab, setTab] = createSignal<FriendsTab>("online");
  const [showAddFriend, setShowAddFriend] = createSignal(false);

  onMount(async () => {
    await Promise.all([loadFriends(), loadPendingRequests(), loadBlocked()]);
  });

  const filteredFriends = () => {
    switch (tab()) {
      case "online":
        return getOnlineFriends();
      case "all":
        return friendsState.friends;
      case "pending":
        return friendsState.pendingRequests;
      case "blocked":
        return friendsState.blocked;
    }
  };

  const handleAccept = async (friendshipId: string) => {
    try {
      await acceptFriendRequest(friendshipId);
    } catch (err) {
      console.error("Failed to accept friend request:", err);
    }
  };

  const handleReject = async (friendshipId: string) => {
    try {
      await rejectFriendRequest(friendshipId);
    } catch (err) {
      console.error("Failed to reject friend request:", err);
    }
  };

  const handleRemove = async (friendshipId: string) => {
    try {
      if (confirm("Are you sure you want to remove this friend?")) {
        await removeFriend(friendshipId);
      }
    } catch (err) {
      console.error("Failed to remove friend:", err);
    }
  };

  return (
    <div class="flex-1 flex flex-col h-full">
      {/* Tab bar */}
      <div class="flex items-center gap-4 px-4 py-3 border-b border-white/10">
        <button
          onClick={() => setTab("online")}
          class="px-3 py-1.5 rounded-lg font-medium transition-colors"
          classList={{
            "bg-accent-primary text-surface-base": tab() === "online",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "online",
          }}
        >
          Online ({getOnlineFriends().length})
        </button>
        <button
          onClick={() => setTab("all")}
          class="px-3 py-1.5 rounded-lg font-medium transition-colors"
          classList={{
            "bg-accent-primary text-surface-base": tab() === "all",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "all",
          }}
        >
          All Friends ({friendsState.friends.length})
        </button>
        <button
          onClick={() => setTab("pending")}
          class="px-3 py-1.5 rounded-lg font-medium transition-colors"
          classList={{
            "bg-accent-primary text-surface-base": tab() === "pending",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "pending",
          }}
        >
          Pending ({friendsState.pendingRequests.length})
        </button>
        <button
          onClick={() => setTab("blocked")}
          class="px-3 py-1.5 rounded-lg font-medium transition-colors"
          classList={{
            "bg-accent-primary text-surface-base": tab() === "blocked",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "blocked",
          }}
        >
          Blocked ({friendsState.blocked.length})
        </button>
        <button
          onClick={() => setShowAddFriend(true)}
          class="ml-auto px-4 py-1.5 bg-accent-primary text-surface-base rounded-lg font-medium hover:opacity-90 transition-opacity"
        >
          Add Friend
        </button>
      </div>

      {/* Friend list */}
      <div class="flex-1 overflow-y-auto">
        <Show
          when={!friendsState.isLoading && filteredFriends().length > 0}
          fallback={
            <div class="flex items-center justify-center h-64 text-text-secondary">
              <Show
                when={!friendsState.isLoading}
                fallback={<div>Loading...</div>}
              >
                <div>No {tab()} friends</div>
              </Show>
            </div>
          }
        >
          <div class="space-y-2 p-4">
            <For each={filteredFriends()}>
              {(friend) => (
                <FriendItem
                  friend={friend}
                  tab={tab()}
                  onAccept={handleAccept}
                  onReject={handleReject}
                  onRemove={handleRemove}
                />
              )}
            </For>
          </div>
        </Show>
      </div>

      {/* Add Friend Modal */}
      <Show when={showAddFriend()}>
        <AddFriend onClose={() => setShowAddFriend(false)} />
      </Show>
    </div>
  );
};

interface FriendItemProps {
  friend: Friend;
  tab: FriendsTab;
  onAccept: (friendshipId: string) => void;
  onReject: (friendshipId: string) => void;
  onRemove: (friendshipId: string) => void;
}

const FriendItem: Component<FriendItemProps> = (props) => {
  return (
    <div class="flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors">
      {/* Avatar */}
      <div class="relative">
        <div class="w-10 h-10 rounded-full bg-accent-primary flex items-center justify-center font-semibold text-surface-base">
          {props.friend.display_name.charAt(0).toUpperCase()}
        </div>
        <Show when={props.friend.is_online && props.tab !== "blocked"}>
          <div class="absolute bottom-0 right-0 w-3 h-3 bg-green-500 border-2 border-surface-base rounded-full" />
        </Show>
      </div>

      {/* Info */}
      <div class="flex-1 min-w-0">
        <div class="font-semibold text-text-primary truncate">
          {props.friend.display_name}
        </div>
        <div class="text-sm text-text-secondary truncate">
          @{props.friend.username}
          <Show when={props.friend.status_message}>
            {" "}
            - {props.friend.status_message}
          </Show>
        </div>
      </div>

      {/* Actions */}
      <div class="flex gap-2">
        <Show when={props.tab === "pending"}>
          <button
            onClick={() => props.onAccept(props.friend.friendship_id)}
            class="px-3 py-1.5 bg-green-600 text-white rounded-lg text-sm font-medium hover:bg-green-700 transition-colors"
          >
            Accept
          </button>
          <button
            onClick={() => props.onReject(props.friend.friendship_id)}
            class="px-3 py-1.5 bg-red-600 text-white rounded-lg text-sm font-medium hover:bg-red-700 transition-colors"
          >
            Reject
          </button>
        </Show>
        <Show when={props.tab === "all" || props.tab === "online"}>
          <button
            onClick={() => props.onRemove(props.friend.friendship_id)}
            class="px-3 py-1.5 bg-red-600 text-white rounded-lg text-sm font-medium hover:bg-red-700 transition-colors"
          >
            Remove
          </button>
        </Show>
      </div>
    </div>
  );
};

export default FriendsList;
