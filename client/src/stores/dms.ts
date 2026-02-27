/**
 * DMs Store
 *
 * Manages DM channels state for Home view.
 */

import { createStore } from "solid-js/store";
import type { DMListItem, Message } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { subscribeChannel } from "@/stores/websocket";
import { showToast } from "@/components/ui/Toast";

interface DMsStoreState {
  dms: DMListItem[];
  selectedDMId: string | null;
  isShowingFriends: boolean;
  typingUsers: Record<string, string[]>;
  isLoading: boolean;
  error: string | null;
}

const [dmsState, setDmsState] = createStore<DMsStoreState>({
  dms: [],
  selectedDMId: null,
  isShowingFriends: true,
  typingUsers: {},
  isLoading: false,
  error: null,
});

/**
 * Load all DMs for the current user and subscribe to their channels
 */
export async function loadDMs(): Promise<void> {
  setDmsState({ isLoading: true, error: null });

  try {
    const dms = await tauri.getDMList();
    setDmsState({ dms, isLoading: false });

    // Wait for WebSocket to be fully connected before subscribing
    // Poll for connection status with timeout
    const maxWaitMs = 5000;
    const pollIntervalMs = 100;
    let waited = 0;

    while (waited < maxWaitMs) {
      const status = await tauri.wsStatus();
      if (status.type === "connected") {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
      waited += pollIntervalMs;
    }

    const finalStatus = await tauri.wsStatus();
    if (finalStatus.type !== "connected") {
      console.warn(
        "[DMs] WebSocket not connected after waiting, skipping subscriptions",
      );
      return;
    }

    // Subscribe to all DM channels for real-time events (messages, calls, etc.)
    for (const dm of dms) {
      try {
        await subscribeChannel(dm.id);
        console.log(`[DMs] Subscribed to channel ${dm.id}`);
      } catch (err) {
        console.warn(`Failed to subscribe to DM channel ${dm.id}:`, err);
      }
    }
  } catch (err) {
    console.error("Failed to load DMs:", err);
    setDmsState({
      error: err instanceof Error ? err.message : "Failed to load DMs",
      isLoading: false,
    });
  }
}

/**
 * Select a DM to view
 */
export function selectDM(channelId: string): void {
  setDmsState({
    selectedDMId: channelId,
    isShowingFriends: false,
  });
}

/**
 * Switch to Friends tab
 */
export function selectFriendsTab(): void {
  setDmsState({
    selectedDMId: null,
    isShowingFriends: true,
  });
}

/**
 * Update last message for a DM (from WebSocket)
 */
export function updateDMLastMessage(channelId: string, message: Message): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex === -1) return;

  setDmsState("dms", dmIndex, {
    last_message: {
      id: message.id,
      content: message.content,
      user_id: message.author.id,
      username: message.author.username,
      created_at: message.created_at,
    },
    unread_count: dmsState.dms[dmIndex].unread_count + 1,
  });

  // Re-sort DMs by last message time
  const sortedDMs = [...dmsState.dms].sort((a, b) => {
    const aTime = a.last_message?.created_at || a.created_at;
    const bTime = b.last_message?.created_at || b.created_at;
    return new Date(bTime).getTime() - new Date(aTime).getTime();
  });
  setDmsState({ dms: sortedDMs });
}

/**
 * Mark DM as read (called when viewing a DM)
 */
export async function markDMAsRead(channelId: string): Promise<void> {
  const dm = dmsState.dms.find((d) => d.id === channelId);
  if (!dm || dm.unread_count === 0) return;

  try {
    await tauri.markDMAsRead(channelId, dm.last_message?.id);

    const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
    if (dmIndex !== -1) {
      setDmsState("dms", dmIndex, "unread_count", 0);
    }
  } catch (err) {
    console.error("Failed to mark DM as read:", err);
    showToast({
      type: "error",
      title: "Failed to Mark DM as Read",
      message: "Could not update read status. Will retry on next message.",
      duration: 8000,
    });
  }
}

/**
 * Mark all DMs as read (optimistic update + API call).
 */
export async function markAllDMsAsRead(): Promise<void> {
  // Optimistic update: zero out all DM unread counts
  dmsState.dms.forEach((dm, idx) => {
    if (dm.unread_count > 0) {
      setDmsState("dms", idx, "unread_count", 0);
    }
  });

  try {
    await tauri.markAllDMsRead();
  } catch (err) {
    console.error("[DMs] Failed to mark all DMs as read:", err);
    showToast({
      type: "error",
      title: "Mark All Read Failed",
      message: "Could not mark all DMs as read. Please try again.",
      duration: 8000,
    });
  }
}

/**
 * Handle dm_read event from WebSocket (cross-device sync)
 */
export function handleDMReadEvent(channelId: string): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex !== -1) {
    setDmsState("dms", dmIndex, "unread_count", 0);
  }
}

/**
 * Update a DM channel's name (from WebSocket event).
 */
export function handleDMNameUpdated(channelId: string, name: string): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex !== -1) {
    setDmsState("dms", dmIndex, "name", name);
  }
}

/**
 * Update a DM channel's icon URL.
 */
export function updateDMIconUrl(channelId: string, iconUrl: string): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex !== -1) {
    setDmsState("dms", dmIndex, "icon_url", iconUrl);
  }
}

/**
 * Get the currently selected DM
 */
export function getSelectedDM(): DMListItem | null {
  if (!dmsState.selectedDMId) return null;
  return dmsState.dms.find((d) => d.id === dmsState.selectedDMId) || null;
}

/**
 * Get total unread count across all DMs
 */
export function getTotalUnreadCount(): number {
  return dmsState.dms.reduce((sum, dm) => sum + dm.unread_count, 0);
}

export { dmsState, setDmsState };
