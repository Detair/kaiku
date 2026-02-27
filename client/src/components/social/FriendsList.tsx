/**
 * Friends List Component
 *
 * Displays friends with tabs for Online, All, Pending, and Blocked.
 */

import { Component, createSignal, For, onMount, Show } from "solid-js";
import { Users, Search, UserPlus, Ghost } from "lucide-solid";
import {
  friendsState,
  loadFriends,
  loadPendingRequests,
  loadBlocked,
  getOnlineFriends,
  acceptFriendRequest,
  rejectFriendRequest,
  removeFriend,
  unblockUser,
} from "@/stores/friends";
import { getUserActivity } from "@/stores/presence";
import type { Friend } from "@/lib/types";
import { ActivityIndicator } from "@/components/ui";
import { showToast } from "@/components/ui/Toast";
import AddFriend from "./AddFriend";

type FriendsTab = "online" | "all" | "pending" | "blocked";

const FriendsList: Component = () => {
  const [tab, setTab] = createSignal<FriendsTab>("online");
  const [showAddFriend, setShowAddFriend] = createSignal(false);
  const [filterQuery, setFilterQuery] = createSignal("");

  onMount(async () => {
    await Promise.all([loadFriends(), loadPendingRequests(), loadBlocked()]);
  });

  const filteredFriends = () => {
    let list: Friend[] = [];
    switch (tab()) {
      case "online":
        list = getOnlineFriends();
        break;
      case "all":
        list = friendsState.friends;
        break;
      case "pending":
        list = friendsState.pendingRequests;
        break;
      case "blocked":
        list = friendsState.blocked;
        break;
    }

    const query = filterQuery().toLowerCase();
    if (!query) return list;

    return list.filter(
      (f) =>
        f.display_name.toLowerCase().includes(query) ||
        f.username.toLowerCase().includes(query),
    );
  };

  const handleAccept = async (friendshipId: string) => {
    try {
      await acceptFriendRequest(friendshipId);
    } catch (err) {
      console.error("Failed to accept friend request:", err);
      showToast({
        type: "error",
        title: "Could not accept friend request. Please try again.",
        duration: 8000,
      });
    }
  };

  const handleReject = async (friendshipId: string) => {
    try {
      await rejectFriendRequest(friendshipId);
    } catch (err) {
      console.error("Failed to reject friend request:", err);
      showToast({
        type: "error",
        title: "Could not decline friend request. Please try again.",
        duration: 8000,
      });
    }
  };

  const handleRemove = async (friendshipId: string) => {
    try {
      if (confirm("Are you sure you want to remove this friend?")) {
        await removeFriend(friendshipId);
      }
    } catch (err) {
      console.error("Failed to remove friend:", err);
      showToast({
        type: "error",
        title: "Could not remove friend. Please try again.",
        duration: 8000,
      });
    }
  };

  const handleUnblock = async (userId: string) => {
    try {
      await unblockUser(userId);
    } catch (err) {
      console.error("Failed to unblock user:", err);
      showToast({
        type: "error",
        title: "Could not unblock user. Please try again.",
        duration: 8000,
      });
    }
  };

  return (
    <div class="flex-1 flex flex-col h-full">
      {/* Tab bar with Search */}
      <div class="flex items-center gap-4 px-4 py-3 border-b border-white/10">
        <div class="flex items-center gap-1 text-text-primary mr-2">
          <Users class="w-5 h-5" />
          <span class="font-bold">Friends</span>
        </div>

        <div class="h-6 w-px bg-white/10 mx-2" />

        <button
          onClick={() => setTab("online")}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "online",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "online",
          }}
        >
          Online
        </button>
        <button
          onClick={() => setTab("all")}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "all",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "all",
          }}
        >
          All
        </button>
        <button
          onClick={() => setTab("pending")}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "pending",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "pending",
          }}
        >
          Pending
          <Show when={friendsState.pendingRequests.length > 0}>
            <span class="ml-2 px-1.5 py-0.5 bg-accent-danger text-white text-[10px] rounded-full">
              {friendsState.pendingRequests.length}
            </span>
          </Show>
        </button>
        <button
          onClick={() => setTab("blocked")}
          class="px-3 py-1 rounded-lg font-medium transition-colors text-sm"
          classList={{
            "bg-white/10 text-text-primary": tab() === "blocked",
            "text-text-secondary hover:text-text-primary hover:bg-white/5":
              tab() !== "blocked",
          }}
        >
          Blocked
        </button>

        <div class="flex-1" />

        <Show when={tab() !== "pending" && tab() !== "blocked"}>
          <div class="relative w-48">
            <input
              type="text"
              placeholder="Search friends..."
              value={filterQuery()}
              onInput={(e) => setFilterQuery(e.currentTarget.value)}
              class="w-full pl-8 pr-3 py-1 bg-surface-base rounded-md text-sm text-text-input outline-none border border-white/5 focus:border-accent-primary/50 transition-colors"
            />
            <Search class="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-text-secondary" />
          </div>
        </Show>

        <button
          onClick={() => setShowAddFriend(true)}
          class="btn-primary py-1 px-3 text-sm flex items-center gap-2"
        >
          <UserPlus class="w-4 h-4" />
          Add Friend
        </button>
      </div>

      {/* Friend list */}
      <div class="flex-1 overflow-y-auto">
        <Show
          when={!friendsState.isLoading && filteredFriends().length > 0}
          fallback={
            <div class="flex flex-col items-center justify-center h-full text-text-secondary opacity-60">
              <Show
                when={!friendsState.isLoading}
                fallback={<div>Loading...</div>}
              >
                <div class="bg-surface-layer2 p-6 rounded-full mb-4">
                  <Ghost class="w-12 h-12" />
                </div>
                <div class="text-lg font-medium mb-1">
                  {tab() === "online"
                    ? "No one's online right now."
                    : tab() === "pending"
                      ? "There are no pending friend requests."
                      : tab() === "blocked"
                        ? "You haven't blocked anyone."
                        : "You don't have any friends yet."}
                </div>
                <Show when={tab() === "all" || tab() === "online"}>
                  <button
                    onClick={() => setShowAddFriend(true)}
                    class="text-accent-primary hover:underline text-sm mt-2"
                  >
                    Add someone to get started!
                  </button>
                </Show>
              </Show>
            </div>
          }
        >
          <div class="space-y-2 p-4">
            <div class="text-xs font-semibold text-text-secondary uppercase tracking-wide mb-2 px-2">
              {tab()} — {filteredFriends().length}
            </div>
            <For each={filteredFriends()}>
              {(friend) => (
                <FriendItem
                  friend={friend}
                  tab={tab()}
                  onAccept={handleAccept}
                  onReject={handleReject}
                  onRemove={handleRemove}
                  onUnblock={handleUnblock}
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
  onUnblock: (userId: string) => void;
}

const FriendItem: Component<FriendItemProps> = (props) => {
  return (
    <div class="flex items-center gap-3 p-3 rounded-lg hover:bg-white/5 transition-colors">
      {/* Avatar */}
      <div class="relative">
        <div class="w-10 h-10 rounded-full bg-accent-primary flex items-center justify-center font-semibold text-white">
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
        {/* Activity indicator */}
        <Show when={getUserActivity(props.friend.user_id)}>
          <ActivityIndicator
            activity={getUserActivity(props.friend.user_id)!}
            compact
          />
        </Show>
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
        <Show when={props.tab === "blocked"}>
          <button
            onClick={() => props.onUnblock(props.friend.user_id)}
            class="px-3 py-1.5 bg-white/10 text-text-primary rounded-lg text-sm font-medium hover:bg-white/20 transition-colors"
          >
            Unblock
          </button>
        </Show>
      </div>
    </div>
  );
};

export default FriendsList;
