/**
 * Presence Store
 *
 * Tracks user online status and presence information.
 * Includes idle detection to automatically set user status to 'idle' after inactivity.
 */

import { createStore, produce } from "solid-js/store";
import type { Activity, UserPresence, UserStatus } from "@/lib/types";
import {
  startIdleDetection,
  stopIdleDetection,
  setIdleTimeout,
} from "@/lib/idleDetector";
import { updateStatus } from "@/lib/tauri";
import { preferences } from "./preferences";
import { currentUser } from "./auth";

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
    activityUnlistener = await listen<Activity | null>("presence:activity_changed", async (event) => {
      // Send activity to server via WebSocket command
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("ws_send_activity", { activity: event.payload });
      } catch (e) {
        console.error("Failed to send activity to server:", e);
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

// ============================================================================
// Idle Detection Integration
// ============================================================================

// Track the user's status before going idle (to restore on activity)
let previousStatus: UserStatus = "online";

// Track if user manually set idle or dnd (don't auto-restore in that case)
let wasManuallySetIdle = false;

/**
 * Set the current user's presence status.
 * Updates both server and local state.
 */
export async function setMyStatus(status: UserStatus): Promise<void> {
  const user = currentUser();
  if (!user) return;

  try {
    await updateStatus(status);
    updateUserPresence(user.id, status);
  } catch (e) {
    console.error("[Presence] Failed to update status:", e);
  }
}

/**
 * Get the current user's status.
 */
export function getMyStatus(): UserStatus {
  const user = currentUser();
  if (!user) return "offline";
  return getUserStatus(user.id);
}

/**
 * Initialize idle detection.
 * Automatically sets user to 'idle' after configured timeout of inactivity.
 * Restores previous status when user becomes active again.
 */
export function initIdleDetection(): void {
  const timeout = preferences().display?.idleTimeoutMinutes ?? 5;

  startIdleDetection((isIdle) => {
    const currentStatus = getMyStatus();

    if (isIdle && currentStatus === "online") {
      // User went idle while online - save status and switch to idle
      previousStatus = "online";
      wasManuallySetIdle = false;
      setMyStatus("idle");
    } else if (!isIdle && currentStatus === "idle" && !wasManuallySetIdle) {
      // User became active while auto-idle - restore previous status
      setMyStatus(previousStatus);
    }
  }, timeout);
}

/**
 * Stop idle detection and clean up.
 */
export function stopIdleDetectionCleanup(): void {
  stopIdleDetection();
}

/**
 * Update the idle timeout from preferences.
 * Called when user changes the idle timeout setting.
 */
export function updateIdleTimeout(minutes: number): void {
  setIdleTimeout(minutes);
}

/**
 * Mark that the user manually set their status.
 * Prevents auto-restore when user becomes active.
 * Call this when user explicitly changes their status via UI.
 */
export function markManualStatusChange(status: UserStatus): void {
  if (status === "idle" || status === "dnd" || status === "invisible") {
    wasManuallySetIdle = true;
  } else if (status === "online") {
    // User manually set online, clear the manual flag
    wasManuallySetIdle = false;
    previousStatus = "online";
  }
}

/**
 * Apply a partial patch to a user's data.
 * Updates presence state and auth store's current user if applicable.
 */
export function patchUser(userId: string, diff: Record<string, unknown>): void {
  // Update presence state if there are presence-related fields
  if ("status" in diff || "activity" in diff) {
    setPresenceState(
      produce((state) => {
        if (state.users[userId]) {
          if ("status" in diff) {
            state.users[userId].status = diff.status as UserStatus;
          }
          if ("activity" in diff) {
            state.users[userId].activity = diff.activity as Activity | null | undefined;
          }
        }
      })
    );
  }

  // Update current user in auth store if this is the current user
  const user = currentUser();
  if (user && user.id === userId) {
    // Import updateUser dynamically to avoid circular deps
    import("./auth").then(({ updateUser }) => {
      // Filter to only valid User fields
      const validFields: (keyof import("@/lib/types").User)[] = [
        "username", "display_name", "avatar_url", "email", "mfa_enabled"
      ];
      const updates: Partial<import("@/lib/types").User> = {};
      for (const field of validFields) {
        if (field in diff) {
          (updates as Record<string, unknown>)[field] = diff[field];
        }
      }
      if (Object.keys(updates).length > 0) {
        updateUser(updates);
      }
    });
  }
}

// Export the store for reading
export { presenceState };
