/**
 * Messages Store
 *
 * Manages message state for channels including loading, sending, and real-time updates.
 */

import { createStore, produce } from "solid-js/store";
import type { Message } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// Messages state interface
interface MessagesState {
  // Messages indexed by channel ID
  byChannel: Record<string, Message[]>;
  // Loading state per channel
  loadingChannels: Set<string>;
  // Whether there are more messages to load per channel
  hasMore: Record<string, boolean>;
  // Current error
  error: string | null;
}

// Create the store
const [messagesState, setMessagesState] = createStore<MessagesState>({
  byChannel: {},
  loadingChannels: new Set(),
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
  if (messagesState.loadingChannels.has(channelId)) {
    return;
  }

  setMessagesState(
    produce((state) => {
      state.loadingChannels.add(channelId);
      state.error = null;
    })
  );

  try {
    // Get existing messages to find the oldest one for pagination
    const existing = messagesState.byChannel[channelId] || [];
    const before = existing.length > 0 ? existing[0].id : undefined;

    const messages = await tauri.getMessages(channelId, before, MESSAGE_LIMIT);

    setMessagesState(
      produce((state) => {
        // Initialize channel if needed
        if (!state.byChannel[channelId]) {
          state.byChannel[channelId] = [];
        }

        // Prepend older messages (they come from server newest-first, but we want oldest-first)
        const reversed = [...messages].reverse();
        state.byChannel[channelId] = [...reversed, ...state.byChannel[channelId]];

        // Check if there are more messages to load
        state.hasMore[channelId] = messages.length === MESSAGE_LIMIT;

        state.loadingChannels.delete(channelId);
      })
    );
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load messages:", error);
    setMessagesState(
      produce((state) => {
        state.loadingChannels.delete(channelId);
        state.error = error;
      })
    );
  }
}

/**
 * Load initial messages for a channel (clears existing).
 */
export async function loadInitialMessages(channelId: string): Promise<void> {
  setMessagesState(
    produce((state) => {
      state.byChannel[channelId] = [];
      state.hasMore[channelId] = true;
    })
  );
  await loadMessages(channelId);
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

    // Add the sent message to the store
    setMessagesState(
      produce((state) => {
        if (!state.byChannel[channelId]) {
          state.byChannel[channelId] = [];
        }
        state.byChannel[channelId].push(message);
      })
    );

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
  setMessagesState(
    produce((state) => {
      const channelId = message.channel_id;

      if (!state.byChannel[channelId]) {
        state.byChannel[channelId] = [];
      }

      // Avoid duplicates
      const exists = state.byChannel[channelId].some((m) => m.id === message.id);
      if (!exists) {
        state.byChannel[channelId].push(message);
      }
    })
  );
}

/**
 * Update an existing message (for edits).
 */
export function updateMessage(message: Message): void {
  setMessagesState(
    produce((state) => {
      const channelId = message.channel_id;
      const messages = state.byChannel[channelId];

      if (messages) {
        const index = messages.findIndex((m) => m.id === message.id);
        if (index !== -1) {
          messages[index] = message;
        }
      }
    })
  );
}

/**
 * Remove a message (for deletes).
 */
export function removeMessage(channelId: string, messageId: string): void {
  setMessagesState(
    produce((state) => {
      const messages = state.byChannel[channelId];
      if (messages) {
        const index = messages.findIndex((m) => m.id === messageId);
        if (index !== -1) {
          messages.splice(index, 1);
        }
      }
    })
  );
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
  return messagesState.loadingChannels.has(channelId);
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
  setMessagesState(
    produce((state) => {
      delete state.byChannel[channelId];
      delete state.hasMore[channelId];
    })
  );
}

// Export the store for reading
export { messagesState };
