/**
 * Presence Store
 *
 * Tracks user online status and presence information.
 */

import { createStore, produce } from "solid-js/store";
import type { UserStatus } from "@/lib/types";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

// Presence info for a user
interface UserPresence {
  status: UserStatus;
  lastSeen?: string;
}

interface PresenceState {
  // Map of user_id -> presence info
  users: Record<string, UserPresence>;
}

// Create the store
const [presenceState, setPresenceState] = createStore<PresenceState>({
  users: {},
});

// Event listener cleanup
let unlistener: UnlistenFn | null = null;

/**
 * Initialize presence event listeners.
 */
export async function initPresence(): Promise<void> {
  // Clean up existing listener
  if (unlistener) {
    unlistener();
  }

  // Only set up Tauri listeners in Tauri mode
  // In browser mode, presence updates are handled by the websocket store
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    unlistener = await listen<{ user_id: string; status: UserStatus }>(
      "ws:presence_update",
      (event) => {
        const { user_id, status } = event.payload;
        updateUserPresence(user_id, status);
      }
    );
  }
}

/**
 * Cleanup presence listeners.
 */
export function cleanupPresence(): void {
  if (unlistener) {
    unlistener();
    unlistener = null;
  }
}

/**
 * Update a user's presence status.
 */
export function updateUserPresence(userId: string, status: UserStatus): void {
  setPresenceState(
    produce((state) => {
      state.users[userId] = {
        status,
        lastSeen: status === "offline" ? new Date().toISOString() : undefined,
      };
    })
  );
}

/**
 * Set initial presence for multiple users.
 */
export function setInitialPresence(users: Array<{ id: string; status: UserStatus }>): void {
  setPresenceState(
    produce((state) => {
      for (const user of users) {
        state.users[user.id] = { status: user.status };
      }
    })
  );
}

/**
 * Get a user's presence status.
 */
export function getUserStatus(userId: string): UserStatus {
  return presenceState.users[userId]?.status ?? "offline";
}

/**
 * Get a user's presence info.
 */
export function getUserPresence(userId: string): UserPresence | undefined {
  return presenceState.users[userId];
}

/**
 * Check if a user is online.
 */
export function isUserOnline(userId: string): boolean {
  const status = getUserStatus(userId);
  return status !== "offline";
}

/**
 * Clear all presence data.
 */
export function clearPresence(): void {
  setPresenceState({ users: {} });
}

// Export the store for reading
export { presenceState };
