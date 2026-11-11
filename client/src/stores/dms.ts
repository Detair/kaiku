/**
 * DMs Store
 *
 * Manages DM channels state for Home view.
 */

import { createStore } from "solid-js/store";
import type { DMListItem, Message } from "@/lib/types";
import * as tauri from "@/lib/tauri";
import { subscribeChannel, waitForConnection } from "@/stores/websocket";
import { showToast } from "@/components/ui/Toast";
import { currentUser } from "@/stores/auth";
import { t } from "@/i18n";
import { guildsState } from "@/stores/guilds";
import { errorText } from "@/i18n/errors";

interface DMsStoreState {
  dms: DMListItem[];
  selectedDMId: string | null;
  isShowingFriends: boolean;
  typingUsers: Record<string, string[]>;
  isLoading: boolean;
  error: string | null;
  /**
   * Counts every pick of a conversation or the friends page, also of the one
   * already open, so the phone drawer can close on it (NAV-12, K2).
   */
  navigationCount: number;
}

const initialState = (): DMsStoreState => ({
  dms: [],
  selectedDMId: null,
  isShowingFriends: true,
  typingUsers: {},
  isLoading: false,
  error: null,
  navigationCount: 0,
});

const [dmsState, setDmsState] = createStore<DMsStoreState>(initialState());

/** True while the window is visible and focused: someone can read it. */
export function isWindowActive(): boolean {
  return (
    typeof document !== "undefined" &&
    document.visibilityState === "visible" &&
    document.hasFocus()
  );
}

// Account the store's data belongs to (W3).
let accountId: string | null = null;

/**
 * Drop everything from another account. Call before loading, with the
 * signed-in user's id: after sign-out and sign-in in the same tab the store
 * would otherwise still hold the previous account's conversations and counts.
 * Returns true when it reset.
 */
export function bindDMsToAccount(userId: string | null): boolean {
  if (accountId === userId) return false;
  accountId = userId;
  setDmsState(initialState());
  readNotSaved.clear();
  return true;
}

/**
 * Load all DMs for the current user and subscribe to their channels
 */
export async function loadDMs(): Promise<void> {
  setDmsState({ isLoading: true, error: null });

  try {
    const dms = await tauri.getDMList();
    setDmsState({ dms, isLoading: false });

    // Wait for WebSocket to be fully connected before subscribing (event-driven)
    const connected = await waitForConnection();
    if (!connected) {
      console.warn(
        "[DMs] WebSocket not connected after waiting, skipping subscriptions",
      );
      return;
    }

    // Subscribe to all DM channels in parallel for real-time events
    await Promise.all(
      dms.map((dm) =>
        subscribeChannel(dm.id)
          .then(() => console.log(`[DMs] Subscribed to channel ${dm.id}`))
          .catch((err) =>
            console.warn(`Failed to subscribe to DM channel ${dm.id}:`, err),
          ),
      ),
    );
  } catch (err) {
    console.error("Failed to load DMs:", err);
    setDmsState({
      error: errorText(err, "dm.list.loadFailed"),
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
    navigationCount: dmsState.navigationCount + 1,
  });
}

/**
 * Open the conversation with these people: reuse an existing 1:1 DM, else
 * create one (the server allows DMs with anyone who has not blocked you), then
 * switch to Home and show it. Throws when the server refuses; callers show
 * the reason.
 */
export async function openDirectMessage(userIds: string[]): Promise<string> {
  let channelId: string | undefined;
  if (userIds.length === 1) {
    const me = currentUser()?.id;
    channelId = dmsState.dms.find((dm) => {
      const others = dm.participants.filter((p) => p.user_id !== me);
      return others.length === 1 && others[0].user_id === userIds[0];
    })?.id;
  }
  if (!channelId) {
    const dm = await tauri.createDM(userIds);
    channelId = dm.channel?.id ?? (dm as { id?: string }).id;
    if (!channelId) throw new Error("DM creation response missing channel id");
    await loadDMs();
  }
  const { guildsState, selectHome } = await import("./guilds");
  if (guildsState.activeGuildId !== null) void selectHome();
  selectDM(channelId);
  return channelId;
}

/**
 * Switch to Friends tab
 */
export function selectFriendsTab(): void {
  setDmsState({
    selectedDMId: null,
    isShowingFriends: true,
    navigationCount: dmsState.navigationCount + 1,
  });
}

/**
 * Check if DM has unread messages via ID comparison.
 */
export function isDMUnread(channelId: string): boolean {
  const dm = dmsState.dms.find((d) => d.id === channelId);
  if (!dm) return false;
  return dm.last_message_id != null &&
    dm.last_message_id !== dm.last_read_message_id;
}

/**
 * Get last_read_message_id for a DM.
 */
export function getDMLastReadMessageId(channelId: string): string | null {
  const dm = dmsState.dms.find((d) => d.id === channelId);
  return dm?.last_read_message_id ?? null;
}

/**
 * Update last message for a DM (from WebSocket)
 */
export function updateDMLastMessage(channelId: string, message: Message): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex === -1) return;

  const isOwnMessage = message.author.id === currentUser()?.id;
  // Read only while it is on screen: Home is open and shows this very
  // conversation. In a server the last opened DM stays "selected" but hidden
  // (W2), so its messages must count as unread.
  // And the window must be in front (R1): in a background tab nobody reads,
  // so title and Home count must show it. On return, DMConversation marks
  // it read.
  const isSelectedDM =
    isWindowActive() &&
    guildsState.activeGuildId === null &&
    dmsState.selectedDMId === channelId &&
    !dmsState.isShowingFriends;
  const shouldIncrementUnread = !isOwnMessage && !isSelectedDM;

  setDmsState("dms", dmIndex, {
    last_message: {
      id: message.id,
      content: message.content,
      user_id: message.author.id,
      username: message.author.username,
      created_at: message.created_at,
    },
    last_message_id: message.id,
    unread_count: shouldIncrementUnread
      ? dmsState.dms[dmIndex].unread_count + 1
      : dmsState.dms[dmIndex].unread_count,
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
export async function markDMAsRead(channelId: string, lastMessageId: string): Promise<void> {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex !== -1) {
    // Optimistic update
    setDmsState("dms", dmIndex, "unread_count", 0);
    setDmsState("dms", dmIndex, "last_read_message_id", lastMessageId);

    // Shown as read at once; the server copy is tracked apart, so a failed
    // save is tried again at the next chance (open, focus, next message).
    readSaving.set(channelId, lastMessageId);
    try {
      await tauri.markDMAsRead(channelId, lastMessageId);
      readNotSaved.delete(channelId);
    } catch (err) {
      console.error("[DMs] Failed to mark DM as read:", err);
      readNotSaved.add(channelId);
    } finally {
      if (readSaving.get(channelId) === lastMessageId) readSaving.delete(channelId);
    }
  }
}

// Conversations shown as read here whose read cursor the server has not
// stored (the request failed), and the cursor of a request on the way. Not
// reactive: a failure must not re-run effects (no retry storm).
const readNotSaved = new Set<string>();
const readSaving = new Map<string, string>();

/**
 * The conversation is on screen and the window is active: move the read
 * cursor to its newest message (N1). Uses the DM route (`POST /api/dm/{id}/read`);
 * the channel route refuses conversations without a server. Does nothing when
 * the cursor is already there, so callers may run it on every change.
 */
export function readVisibleDM(channelId: string, newestMessageId: string | null): void {
  const dm = dmsState.dms.find((d) => d.id === channelId);
  if (!dm) return;
  const target = newestMessageId ?? dm.last_message_id ?? null;
  if (!target) return;
  // The same cursor is already on its way; a newer one goes out (the server
  // only moves forward).
  if (readSaving.get(channelId) === target) return;
  const shownRead = dm.last_read_message_id === target && dm.unread_count === 0;
  if (shownRead && !readNotSaved.has(channelId)) return;
  void markDMAsRead(channelId, target);
}

/**
 * Mark all DMs as read (optimistic update + API call).
 */
export async function markAllDMsAsRead(): Promise<void> {
  // Optimistic update: zero out all DM unread counts and sync read cursors
  dmsState.dms.forEach((dm, idx) => {
    if (dm.unread_count > 0) {
      setDmsState("dms", idx, "unread_count", 0);
      if (dm.last_message_id) {
        setDmsState("dms", idx, "last_read_message_id", dm.last_message_id);
      }
    }
  });

  try {
    await tauri.markAllDMsRead();
  } catch (err) {
    console.error("[DMs] Failed to mark all DMs as read:", err);
    showToast({
      type: "error",
      title: t("home.unread.toast.markReadFailedTitle"),
      message: t("dm.list.markAllReadFailed"),
      duration: 8000,
    });
  }
}

/**
 * Handle dm_read event from WebSocket (cross-device sync)
 */
export function handleDMReadEvent(channelId: string, lastReadMessageId?: string): void {
  const dmIndex = dmsState.dms.findIndex((d) => d.id === channelId);
  if (dmIndex !== -1) {
    setDmsState("dms", dmIndex, "unread_count", 0);
    if (lastReadMessageId) {
      setDmsState("dms", dmIndex, "last_read_message_id", lastReadMessageId);
    } else {
      // None = "read to latest"
      const lastMsgId = dmsState.dms[dmIndex].last_message_id;
      if (lastMsgId) {
        setDmsState("dms", dmIndex, "last_read_message_id", lastMsgId);
      }
    }
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
 * A DM with this user was just created (C3 `dm_created`). Adds it to the top
 * of the list once; subscribing records it so reconnects re-subscribe it (the
 * server's auto-subscribe only covers the socket open at creation time).
 */
export function handleDmCreated(channel: DMListItem): void {
  if (!dmsState.dms.some((d) => d.id === channel.id)) {
    setDmsState("dms", (dms) => [channel, ...dms]);
  }
  // Never rejects: subscribeChannel logs its own failures.
  void subscribeChannel(channel.id);
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
