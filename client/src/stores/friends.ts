/**
 * Friends Store
 *
 * Manages friend relationships and friend requests.
 */

import { createStore } from "solid-js/store";
import type { Friend } from "@/lib/types";
import * as tauri from "@/lib/tauri";

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
      friends: friendsState.friends.filter((f) => f.friendship_id !== friendshipId),
    });
  } catch (err) {
    console.error("Failed to remove friend:", err);
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
    throw err;
  }
}

/**
 * Get online friends
 */
export function getOnlineFriends(): Friend[] {
  return friendsState.friends.filter((f) => f.is_online);
}

// Export the store state
export { friendsState };
