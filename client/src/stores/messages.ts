/**
 * Messages Store
 *
 * Manages message state for channels including loading, sending, and real-time updates.
 */

import { createStore } from "solid-js/store";
import type { Message } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// Messages state interface
interface MessagesState {
  // Messages indexed by channel ID
  byChannel: Record<string, Message[]>;
  // Loading state per channel (using Record for SolidJS reactivity - Sets don't work)
  loadingChannels: Record<string, boolean>;
  // Whether there are more messages to load per channel
  hasMore: Record<string, boolean>;
  // Current error
  error: string | null;
}

// Create the store
const [messagesState, setMessagesState] = createStore<MessagesState>({
  byChannel: {},
  loadingChannels: {},
  hasMore: {},
  error: null,
});

// Default message limit per request
const MESSAGE_LIMIT = 50;

// Actions

/**
 * Load messages for a channel.
 * If messages already exist, this fetches older messages (pagination).
 */
export async function loadMessages(channelId: string): Promise<void> {
  // Prevent duplicate loads
  if (messagesState.loadingChannels[channelId]) {
    return;
  }

  setMessagesState("loadingChannels", channelId, true);
  setMessagesState("error", null);

  try {
    // Get existing messages to find the oldest one for pagination
    const existing = messagesState.byChannel[channelId] || [];
    const before = existing.length > 0 ? existing[0].id : undefined;

    const messages = await tauri.getMessages(channelId, before, MESSAGE_LIMIT);
    const messageList = Array.isArray(messages) ? messages : [];

    // Initialize channel if needed
    if (!messagesState.byChannel[channelId]) {
      setMessagesState("byChannel", channelId, []);
    }

    // Prepend older messages (they come from server newest-first, but we want oldest-first)
    const reversed = [...messageList].reverse();
    const currentMessages = messagesState.byChannel[channelId] || [];
    setMessagesState("byChannel", channelId, [...reversed, ...currentMessages]);

    // Check if there are more messages to load
    setMessagesState("hasMore", channelId, messageList.length === MESSAGE_LIMIT);
    setMessagesState("loadingChannels", channelId, false);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load messages:", error);
    setMessagesState("loadingChannels", channelId, false);
    setMessagesState("error", error);
  }
}

// Track ongoing initial loads to prevent duplicates
const initialLoadInProgress: Record<string, boolean> = {};

/**
 * Load initial messages for a channel (clears existing).
 */
export async function loadInitialMessages(channelId: string): Promise<void> {
  // Prevent duplicate initial loads
  if (initialLoadInProgress[channelId]) {
    return;
  }

  initialLoadInProgress[channelId] = true;

  try {
    setMessagesState("byChannel", channelId, []);
    setMessagesState("hasMore", channelId, true);
    await loadMessages(channelId);
  } finally {
    initialLoadInProgress[channelId] = false;
  }
}

/**
 * Send a message to a channel.
 */
export async function sendMessage(
  channelId: string,
  content: string
): Promise<Message | null> {
  if (!content.trim()) {
    return null;
  }

  setMessagesState({ error: null });

  try {
    const message = await tauri.sendMessage(channelId, content.trim());

    // Add the sent message to the store (use path-based setter for proper reactivity)
    const prev = messagesState.byChannel[channelId] || [];
    setMessagesState("byChannel", channelId, [...prev, message]);

    return message;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to send message:", error);
    setMessagesState({ error });
    return null;
  }
}

/**
 * Add a message received from WebSocket.
 */
export function addMessage(message: Message): void {
  const channelId = message.channel_id;
  const existing = messagesState.byChannel[channelId] || [];

  // Avoid duplicates
  if (!existing.some((m) => m.id === message.id)) {
    setMessagesState("byChannel", channelId, [...existing, message]);
  }
}

/**
 * Update an existing message (for edits).
 */
export function updateMessage(message: Message): void {
  const channelId = message.channel_id;
  const messages = messagesState.byChannel[channelId];

  if (messages) {
    const index = messages.findIndex((m) => m.id === message.id);
    if (index !== -1) {
      // Use path-based setter for proper reactivity
      setMessagesState("byChannel", channelId, index, message);
    }
  }
}

/**
 * Remove a message (for deletes).
 */
export function removeMessage(channelId: string, messageId: string): void {
  const existing = messagesState.byChannel[channelId];
  if (existing) {
    setMessagesState("byChannel", channelId, existing.filter((m) => m.id !== messageId));
  }
}

/**
 * Get messages for a channel.
 */
export function getChannelMessages(channelId: string): Message[] {
  return messagesState.byChannel[channelId] || [];
}

/**
 * Check if a channel is loading messages.
 */
export function isLoadingMessages(channelId: string): boolean {
  return !!messagesState.loadingChannels[channelId];
}

/**
 * Check if a channel has more messages to load.
 */
export function hasMoreMessages(channelId: string): boolean {
  return messagesState.hasMore[channelId] ?? true;
}

/**
 * Clear messages for a channel.
 */
export function clearChannelMessages(channelId: string): void {
  setMessagesState("byChannel", channelId, undefined!);
  setMessagesState("hasMore", channelId, undefined!);
}

// Export the store for reading
export { messagesState };
