/**
 * Friends Store
 *
 * Manages friend relationships and friend requests.
 */

import { createStore } from "solid-js/store";
import type { Friend } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";

/**
 * Friends store state
 */
interface FriendsStoreState {
  // All accepted friends
  friends: Friend[];
  // Pending friend requests (both sent and received)
  pendingRequests: Friend[];
  // Blocked users
  blocked: Friend[];
  // Loading states
  isLoading: boolean;
  isPendingLoading: boolean;
  isBlockedLoading: boolean;
  // Error state
  error: string | null;
}

// Create the store
const [friendsState, setFriendsState] = createStore<FriendsStoreState>({
  friends: [],
  pendingRequests: [],
  blocked: [],
  isLoading: false,
  isPendingLoading: false,
  isBlockedLoading: false,
  error: null,
});

/**
 * Load all friends (accepted)
 */
export async function loadFriends(): Promise<void> {
  setFriendsState({ isLoading: true, error: null });

  try {
    const friends = await tauri.getFriends();
    setFriendsState({ friends, isLoading: false });
  } catch (err) {
    console.error("Failed to load friends:", err);
    setFriendsState({
      error: err instanceof Error ? err.message : "Failed to load friends",
      isLoading: false,
    });
  }
}

/**
 * Load pending friend requests
 */
export async function loadPendingRequests(): Promise<void> {
  setFriendsState({ isPendingLoading: true });

  try {
    const pending = await tauri.getPendingFriends();
    setFriendsState({ pendingRequests: pending, isPendingLoading: false });
  } catch (err) {
    console.error("Failed to load pending requests:", err);
    setFriendsState({ isPendingLoading: false });
  }
}

/**
 * Load blocked users
 */
export async function loadBlocked(): Promise<void> {
  setFriendsState({ isBlockedLoading: true });

  try {
    const blocked = await tauri.getBlockedFriends();
    setFriendsState({ blocked, isBlockedLoading: false });
  } catch (err) {
    console.error("Failed to load blocked users:", err);
    setFriendsState({ isBlockedLoading: false });
  }
}

/**
 * Send a friend request by username
 */
export async function sendFriendRequest(username: string): Promise<void> {
  try {
    await tauri.sendFriendRequest(username);
    // Reload pending requests to show the new request
    await loadPendingRequests();
  } catch (err) {
    console.error("Failed to send friend request:", err);
    showToast({
      type: "error",
      title: "Friend Request Failed",
      message: "Could not send request. Please try again.",
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Accept a friend request
 */
export async function acceptFriendRequest(friendshipId: string): Promise<void> {
  try {
    await tauri.acceptFriendRequest(friendshipId);
    // Reload both friends and pending lists
    await Promise.all([loadFriends(), loadPendingRequests()]);
  } catch (err) {
    console.error("Failed to accept friend request:", err);
    showToast({
      type: "error",
      title: "Accept Failed",
      message: "Could not accept friend request. Please try again.",
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Reject a friend request
 */
export async function rejectFriendRequest(friendshipId: string): Promise<void> {
  try {
    await tauri.rejectFriendRequest(friendshipId);
    // Remove from pending list
    await loadPendingRequests();
  } catch (err) {
    console.error("Failed to reject friend request:", err);
    showToast({
      type: "error",
      title: "Reject Failed",
      message: "Could not reject friend request. Please try again.",
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Remove a friend
 */
export async function removeFriend(friendshipId: string): Promise<void> {
  try {
    await tauri.removeFriend(friendshipId);
    // Remove from friends list
    setFriendsState({
      friends: friendsState.friends.filter(
        (f) => f.friendship_id !== friendshipId,
      ),
    });
  } catch (err) {
    console.error("Failed to remove friend:", err);
    showToast({
      type: "error",
      title: "Remove Failed",
      message: "Could not remove friend. Please try again.",
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Block a user
 */
export async function blockUser(userId: string): Promise<void> {
  try {
    await tauri.blockUser(userId);
    // Reload friends, pending, and blocked lists
    await Promise.all([loadFriends(), loadPendingRequests(), loadBlocked()]);
  } catch (err) {
    console.error("Failed to block user:", err);
    showToast({
      type: "error",
      title: "Block Failed",
      message: "Could not block user. Please try again.",
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Unblock a user
 */
export async function unblockUser(userId: string): Promise<void> {
  try {
    await tauri.unblockUser(userId);
    // Remove from blocked list
    setFriendsState({
      blocked: friendsState.blocked.filter((f) => f.user_id !== userId),
    });
  } catch (err) {
    console.error("Failed to unblock user:", err);
    showToast({
      type: "error",
      title: "Unblock Failed",
      message: "Could not unblock user. Please try again.",
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Handle UserBlocked event from WebSocket
 */
export function handleUserBlocked(userId: string): void {
  // Remove from friends list if present
  setFriendsState({
    friends: friendsState.friends.filter((f) => f.user_id !== userId),
    pendingRequests: friendsState.pendingRequests.filter(
      (f) => f.user_id !== userId,
    ),
  });
  // Reload blocked list to show new entry
  loadBlocked();
}

/**
 * Handle UserUnblocked event from WebSocket
 */
export function handleUserUnblocked(userId: string): void {
  // Remove from blocked list
  setFriendsState({
    blocked: friendsState.blocked.filter((f) => f.user_id !== userId),
  });
}

/**
 * Get online friends
 */
export function getOnlineFriends(): Friend[] {
  return friendsState.friends.filter((f) => f.is_online);
}

// Export the store state
export { friendsState, setFriendsState };
