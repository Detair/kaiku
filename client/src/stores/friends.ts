/**
 * Friends Store
 *
 * Manages friend relationships and friend requests.
 */

import { createStore } from "solid-js/store";
import type { Friend } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { showToast } from "@/components/ui/Toast";
import { t } from "@/i18n";

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
      error: t("home.friends.loadFailed"),
      isLoading: false,
    });
  }
}

// Incoming requests already seen, so a reload only announces new ones.
// null until the first load: requests waiting at startup show as a count,
// not as a burst of notices.
let knownIncomingIds: Set<string> | null = null;

/** Start over: the next load counts as the first (another account signs in). */
export function forgetSeenRequests(): void {
  knownIncomingIds = null;
}

function announceNewIncoming(pending: Friend[]): void {
  const incoming = pending.filter((f) => f.direction === "incoming");
  const previous = knownIncomingIds;
  knownIncomingIds = new Set(incoming.map((f) => f.friendship_id));
  if (!previous) return;
  for (const request of incoming) {
    if (previous.has(request.friendship_id)) continue;
    showToast({
      type: "info",
      title: t("home.friends.toast.requestTitle"),
      message: t("home.friends.toast.requestMessage", { name: request.display_name }),
      duration: 8000,
      id: `friend-request-${request.friendship_id}`,
      action: {
        label: t("home.friends.toast.requestShow"),
        onClick: () => {
          void Promise.all([import("./guilds"), import("./dms")]).then(
            ([guilds, dms]) => {
              if (guilds.guildsState.activeGuildId !== null) void guilds.selectHome();
              dms.selectFriendsTab();
            },
          );
        },
      },
    });
  }
}

/**
 * Load pending friend requests. A request that arrived since the last load
 * shows a notice (NAV-4): the server only says "reload", so the store spots
 * what is new.
 */
export async function loadPendingRequests(): Promise<void> {
  setFriendsState({ isPendingLoading: true });

  try {
    const pending = await tauri.getPendingFriends();
    setFriendsState({ pendingRequests: pending, isPendingLoading: false });
    announceNewIncoming(pending);
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
      title: t("home.friends.toast.sendFailedTitle"),
      message: t("home.friends.toast.sendFailed"),
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Accept a friend request
 */
export async function acceptFriendRequest(friendshipId: string): Promise<void> {
  const pendingSnapshot = friendsState.pendingRequests;
  const acceptedRequest = pendingSnapshot.find(
    (request) => request.friendship_id === friendshipId,
  );

  if (acceptedRequest) {
    setFriendsState({
      pendingRequests: pendingSnapshot.filter(
        (request) => request.friendship_id !== friendshipId,
      ),
    });
  }

  try {
    await tauri.acceptFriendRequest(friendshipId);

    showToast({
      type: "success",
      title: t("home.friends.toast.acceptedTitle"),
      message: acceptedRequest
        ? t("home.friends.toast.accepted", { name: acceptedRequest.display_name })
        : t("home.friends.toast.acceptedNoName"),
      duration: 3000,
    });

    const [friends, pendingRequests] = await Promise.all([
      tauri.getFriends(),
      tauri.getPendingFriends(),
    ]);

    setFriendsState({ friends, pendingRequests });
  } catch (err) {
    if (acceptedRequest) {
      setFriendsState({ pendingRequests: pendingSnapshot });
    }

    console.error("Failed to accept friend request:", err);
    showToast({
      type: "error",
      title: t("home.friends.toast.acceptFailedTitle"),
      message: t("home.friends.toast.acceptFailed"),
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
      title: t("home.friends.toast.rejectFailedTitle"),
      message: t("home.friends.toast.rejectFailed"),
      duration: 8000,
    });
    throw err;
  }
}

/**
 * Cancel an outgoing friend request (uses reject endpoint)
 */
export async function cancelFriendRequest(friendshipId: string): Promise<void> {
  try {
    await tauri.rejectFriendRequest(friendshipId);
    await loadPendingRequests();
  } catch (err) {
    console.error("Failed to cancel friend request:", err);
    showToast({
      type: "error",
      title: t("home.friends.toast.cancelFailedTitle"),
      message: t("home.friends.toast.cancelFailed"),
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
      title: t("home.friends.toast.removeFailedTitle"),
      message: t("home.friends.toast.removeFailed"),
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
      title: t("home.friends.toast.blockFailedTitle"),
      message: t("home.friends.toast.blockFailed"),
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
      title: t("home.friends.toast.unblockFailedTitle"),
      message: t("home.friends.toast.unblockFailed"),
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
 * Friend requests waiting for my answer (shown as a count on Home).
 */
export function getIncomingRequestCount(): number {
  return friendsState.pendingRequests.filter((f) => f.direction === "incoming").length;
}

/**
 * Get online friends
 */
export function getOnlineFriends(): Friend[] {
  return friendsState.friends.filter((f) => f.is_online);
}

// Export the store state
export { friendsState, setFriendsState };
