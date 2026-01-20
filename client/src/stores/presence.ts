/**
 * Presence Store
 *
 * Tracks user online status and presence information.
 */

import { createStore, produce } from "solid-js/store";
import type { Activity, UserPresence, UserStatus } from "@/lib/types";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

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
let activityUnlistener: UnlistenFn | null = null;

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

    // Listen for local activity changes from presence service
    activityUnlistener = await listen<Activity | null>("presence:activity_changed", async (_event) => {
      // Get WebSocket to send activity to server
      try {
        const wsModule = await import("./websocket");
        if (wsModule.isConnected()) {
          // The websocket will handle sending SetActivity to server
          // For now just emit to local - server sync handled in websocket.ts
        }
      } catch (e) {
        console.error("Failed to get WebSocket module:", e);
      }
    });
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
  if (activityUnlistener) {
    activityUnlistener();
    activityUnlistener = null;
  }
}

/**
 * Update a user's presence status with optional activity.
 */
export function updateUserPresence(
  userId: string,
  status: UserStatus,
  activity?: Activity | null
): void {
  setPresenceState(
    produce((state) => {
      state.users[userId] = {
        status,
        activity: activity !== undefined ? activity : state.users[userId]?.activity,
        lastSeen: status === "offline" ? new Date().toISOString() : undefined,
      };
    })
  );
}

/**
 * Update only the activity for a user (keeps status unchanged).
 */
export function updateUserActivity(userId: string, activity: Activity | null): void {
  setPresenceState(
    produce((state) => {
      if (state.users[userId]) {
        state.users[userId].activity = activity;
      } else {
        state.users[userId] = {
          status: "online",
          activity,
        };
      }
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
 * Get activity for a user.
 */
export function getUserActivity(userId: string): Activity | null | undefined {
  return presenceState.users[userId]?.activity;
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
