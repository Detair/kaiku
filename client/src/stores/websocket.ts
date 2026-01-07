/**
 * WebSocket Store
 *
 * Manages WebSocket connection and routes events to appropriate stores.
 */

import { createStore } from "solid-js/store";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import * as tauri from "@/lib/tauri";
import type { Message, ServerEvent, UserStatus } from "@/lib/types";
import { addMessage, updateMessage, removeMessage } from "./messages";

// Connection state
type ConnectionState = "disconnected" | "connecting" | "connected" | "reconnecting";

interface WebSocketState {
  status: ConnectionState;
  reconnectAttempt: number;
  subscribedChannels: Set<string>;
  error: string | null;
}

// Typing state per channel
interface TypingState {
  // Map of channel_id -> Set of user_ids currently typing
  byChannel: Record<string, Set<string>>;
}

// Create stores
const [wsState, setWsState] = createStore<WebSocketState>({
  status: "disconnected",
  reconnectAttempt: 0,
  subscribedChannels: new Set(),
  error: null,
});

const [typingState, setTypingState] = createStore<TypingState>({
  byChannel: {},
});

// Event listeners
let unlisteners: UnlistenFn[] = [];

// Typing debounce timers
const typingTimers: Record<string, NodeJS.Timeout> = {};
const TYPING_TIMEOUT = 5000; // 5 seconds

/**
 * Initialize WebSocket event listeners.
 * Call this once when the app starts (after auth).
 */
export async function initWebSocket(): Promise<void> {
  // Clean up existing listeners
  await cleanupWebSocket();

  // Connection status events
  unlisteners.push(
    await listen("ws:connecting", () => {
      setWsState({ status: "connecting", error: null });
    })
  );

  unlisteners.push(
    await listen("ws:connected", () => {
      setWsState({ status: "connected", reconnectAttempt: 0, error: null });
    })
  );

  unlisteners.push(
    await listen("ws:disconnected", () => {
      setWsState({ status: "disconnected" });
    })
  );

  unlisteners.push(
    await listen<number>("ws:reconnecting", (event) => {
      setWsState({ status: "reconnecting", reconnectAttempt: event.payload });
    })
  );

  // Message events
  unlisteners.push(
    await listen<{ channel_id: string; message: Message }>("ws:message_new", (event) => {
      addMessage(event.payload.message);
    })
  );

  unlisteners.push(
    await listen<{ channel_id: string; message_id: string; content: string; edited_at: string }>(
      "ws:message_edit",
      (event) => {
        const { channel_id, message_id, content, edited_at } = event.payload;
        // We'd need the full message, but for now just update what we can
        // This is a simplified version - in production, you might refetch the message
        console.log("Message edited:", message_id, content);
      }
    )
  );

  unlisteners.push(
    await listen<{ channel_id: string; message_id: string }>("ws:message_delete", (event) => {
      removeMessage(event.payload.channel_id, event.payload.message_id);
    })
  );

  // Typing events
  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>("ws:typing_start", (event) => {
      const { channel_id, user_id } = event.payload;
      addTypingUser(channel_id, user_id);
    })
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>("ws:typing_stop", (event) => {
      const { channel_id, user_id } = event.payload;
      removeTypingUser(channel_id, user_id);
    })
  );

  // Presence events (will be handled by presence store)
  unlisteners.push(
    await listen<{ user_id: string; status: UserStatus }>("ws:presence_update", (event) => {
      // This will be handled by the presence store
      console.log("Presence update:", event.payload.user_id, event.payload.status);
    })
  );

  // Error events
  unlisteners.push(
    await listen<{ code: string; message: string }>("ws:error", (event) => {
      console.error("WebSocket error:", event.payload);
      setWsState({ error: event.payload.message });
    })
  );
}

/**
 * Cleanup WebSocket listeners.
 */
export async function cleanupWebSocket(): Promise<void> {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];

  // Clear typing timers
  for (const timer of Object.values(typingTimers)) {
    clearTimeout(timer);
  }
}

/**
 * Connect to the WebSocket server.
 */
export async function connect(): Promise<void> {
  try {
    setWsState({ status: "connecting", error: null });
    await tauri.wsConnect();
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setWsState({ status: "disconnected", error });
    throw err;
  }
}

/**
 * Disconnect from the WebSocket server.
 */
export async function disconnect(): Promise<void> {
  try {
    await tauri.wsDisconnect();
    setWsState({ status: "disconnected", subscribedChannels: new Set() });
  } catch (err) {
    console.error("Failed to disconnect:", err);
  }
}

/**
 * Subscribe to a channel.
 */
export async function subscribeChannel(channelId: string): Promise<void> {
  if (wsState.subscribedChannels.has(channelId)) return;

  try {
    await tauri.wsSubscribe(channelId);
    setWsState("subscribedChannels", (prev) => {
      const next = new Set(prev);
      next.add(channelId);
      return next;
    });
  } catch (err) {
    console.error("Failed to subscribe to channel:", err);
  }
}

/**
 * Unsubscribe from a channel.
 */
export async function unsubscribeChannel(channelId: string): Promise<void> {
  if (!wsState.subscribedChannels.has(channelId)) return;

  try {
    await tauri.wsUnsubscribe(channelId);
    setWsState("subscribedChannels", (prev) => {
      const next = new Set(prev);
      next.delete(channelId);
      return next;
    });
  } catch (err) {
    console.error("Failed to unsubscribe from channel:", err);
  }
}

/**
 * Send typing indicator (debounced).
 */
let lastTypingSent = 0;
export async function sendTyping(channelId: string): Promise<void> {
  const now = Date.now();
  // Only send typing every 3 seconds
  if (now - lastTypingSent < 3000) return;

  try {
    await tauri.wsTyping(channelId);
    lastTypingSent = now;
  } catch (err) {
    console.error("Failed to send typing:", err);
  }
}

/**
 * Stop typing indicator.
 */
export async function stopTyping(channelId: string): Promise<void> {
  try {
    await tauri.wsStopTyping(channelId);
  } catch (err) {
    console.error("Failed to stop typing:", err);
  }
}

/**
 * Add a user to the typing list for a channel.
 */
function addTypingUser(channelId: string, userId: string): void {
  // Clear existing timer for this user
  const timerKey = `${channelId}:${userId}`;
  if (typingTimers[timerKey]) {
    clearTimeout(typingTimers[timerKey]);
  }

  // Add user to typing set
  setTypingState("byChannel", channelId, (prev) => {
    const next = new Set(prev || []);
    next.add(userId);
    return next;
  });

  // Set timeout to remove user
  typingTimers[timerKey] = setTimeout(() => {
    removeTypingUser(channelId, userId);
    delete typingTimers[timerKey];
  }, TYPING_TIMEOUT);
}

/**
 * Remove a user from the typing list for a channel.
 */
function removeTypingUser(channelId: string, userId: string): void {
  setTypingState("byChannel", channelId, (prev) => {
    if (!prev) return prev;
    const next = new Set(prev);
    next.delete(userId);
    return next;
  });
}

/**
 * Get users currently typing in a channel.
 */
export function getTypingUsers(channelId: string): string[] {
  const users = typingState.byChannel[channelId];
  return users ? Array.from(users) : [];
}

/**
 * Check if connected.
 */
export function isConnected(): boolean {
  return wsState.status === "connected";
}

// Export stores for reading
export { wsState, typingState };
