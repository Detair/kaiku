/**
 * Messages Store
 *
 * Manages message state for channels including loading, sending, and real-time updates.
 * Legacy encrypted messages are decrypted when possible; E2EE sending is removed for v1.0.
 */

import { createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import type { Message, E2EEContent, MegolmE2EEContent } from "@/lib/types";
import { t } from "@/i18n";
import { CHAT_SEND_ERRORS, errorText } from "@/i18n/errors";
import * as tauri from "@/lib/tauri";
import { e2eeStore } from "@/stores/e2ee";
import { showToast } from "@/components/ui/Toast";
import { currentUser } from "@/stores/auth";

// ============================================================================
// E2EE Decryption Helpers
// ============================================================================

// Cache our Curve25519 key to avoid repeated Tauri calls
let cachedOurCurve25519Key: string | null = null;

/**
 * Get our Curve25519 key (cached).
 */
async function getOurCurve25519Key(): Promise<string | null> {
  if (cachedOurCurve25519Key) {
    return cachedOurCurve25519Key;
  }
  cachedOurCurve25519Key = await tauri.getOurCurve25519Key();
  return cachedOurCurve25519Key;
}

/**
 * Clear the cached Curve25519 key (call when E2EE is re-initialized).
 */
export function clearCurve25519KeyCache(): void {
  cachedOurCurve25519Key = null;
}

// ============================================================================
// Edit State
// ============================================================================

/**
 * Tracks which message is currently being edited (only one at a time).
 * null means no message is being edited.
 */
const [editingMessageId, setEditingMessageId] = createSignal<string | null>(null);
export { editingMessageId, setEditingMessageId };

/**
 * Decrypt a message if it's encrypted and E2EE is available.
 * Supports both 1:1 Olm messages and Megolm group messages.
 * Also processes Megolm session key distribution messages automatically.
 * Returns the message with decrypted content, or a placeholder if decryption fails.
 */
export async function decryptMessageIfNeeded(message: Message): Promise<Message> {
  // If not encrypted, return as-is
  if (!message.encrypted) {
    return message;
  }

  // Check if E2EE is initialized
  const status = e2eeStore.status();
  if (!status.initialized) {
    console.warn("[E2EE] Message is encrypted but E2EE not initialized");
    return { ...message, content: t("chat.message.encryptedPlaceholder") };
  }

  try {
    // Parse the encrypted content to determine type
    const parsed = JSON.parse(message.content);

    // Megolm group message — identified by `megolm_ciphertext` key
    if ("megolm_ciphertext" in parsed) {
      const megolm = parsed as MegolmE2EEContent;
      const plaintext = await e2eeStore.decryptGroup(
        megolm.room_id,
        megolm.sender_key,
        megolm.megolm_ciphertext
      );
      return { ...message, content: plaintext };
    }

    // 1:1 Olm message — identified by `recipients` key
    const e2ee = parsed as E2EEContent;

    // Get our identity info
    const ourKey = await getOurCurve25519Key();
    const user = currentUser();

    if (!ourKey || !user) {
      console.warn("[E2EE] Cannot decrypt - missing our key or user info");
      return { ...message, content: t("chat.message.encryptedPlaceholder") };
    }

    const ourUserId = user.id;

    // Find our ciphertext: recipients[our_user_id][our_curve25519_key]
    const userDevices = e2ee.recipients[ourUserId];
    if (!userDevices) {
      console.warn("[E2EE] No ciphertext for our user ID:", ourUserId);
      return { ...message, content: t("chat.message.encryptedPlaceholder") };
    }

    const ourCiphertext = userDevices[ourKey];
    if (!ourCiphertext) {
      console.warn("[E2EE] No ciphertext for our device key");
      return { ...message, content: t("chat.message.encryptedPlaceholder") };
    }

    // Decrypt
    const plaintext = await e2eeStore.decrypt(
      message.author.id, // sender_user_id
      e2ee.sender_key,
      ourCiphertext.message_type,
      ourCiphertext.ciphertext
    );

    // Check if this is a Megolm session key distribution message
    const processed = await processMegolmKeyIfPresent(plaintext, e2ee.sender_key);
    if (processed) {
      // Replace content with a non-visible indicator; this message is internal plumbing
      return { ...message, content: "[Encryption key exchange]" };
    }

    return { ...message, content: plaintext };
  } catch (err) {
    console.error("[E2EE] Decryption failed:", err);
    return { ...message, content: t("chat.message.encryptedPlaceholder") };
  }
}

/**
 * Check if a decrypted Olm message contains a Megolm session key payload.
 * If so, automatically store it as an inbound group session.
 *
 * @returns true if this was a session key message (and was processed), false otherwise
 */
async function processMegolmKeyIfPresent(
  plaintext: string,
  senderKey: string
): Promise<boolean> {
  try {
    const payload = JSON.parse(plaintext);
    if (payload?.type !== "__megolm_session_key__") {
      return false;
    }

    const roomId = payload.room_id;
    const sessionKey = payload.session_key;

    if (!roomId || !sessionKey) {
      console.warn("[E2EE/Megolm] Received malformed session key payload");
      return false;
    }

    // Store the inbound session
    await e2eeStore.addInboundSession(roomId, senderKey, sessionKey);
    console.log(`[E2EE/Megolm] Stored inbound session key for room ${roomId} from ${senderKey.slice(0, 8)}...`);
    return true;
  } catch {
    // Not JSON or not a key payload — that's fine, it's a regular message
    return false;
  }
}

/**
 * Decrypt multiple messages in parallel.
 */
async function decryptMessages(messages: Message[]): Promise<Message[]> {
  return Promise.all(messages.map(decryptMessageIfNeeded));
}

// Messages state interface
interface MessagesState {
  // Messages indexed by channel ID
  byChannel: Record<string, Message[]>;
  // Loading state per channel (using Record for SolidJS reactivity - Sets don't work)
  loadingChannels: Record<string, boolean>;
  // Whether there are more messages to load per channel
  hasMore: Record<string, boolean>;
  // Last load failure (shown by the message list)
  loadError: string | null;
  // Last send failure (shown by the composer)
  sendError: string | null;
}

// Create the store
const [messagesState, setMessagesState] = createStore<MessagesState>({
  byChannel: {},
  loadingChannels: {},
  hasMore: {},
  loadError: null,
  sendError: null,
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
  setMessagesState("loadError", null);

  try {
    // Get existing messages to find the oldest one for pagination
    const existing = messagesState.byChannel[channelId] || [];
    const before = existing.length > 0 ? existing[0].id : undefined;

    const response = await tauri.getMessages(channelId, before, MESSAGE_LIMIT);
    const messageList = response.items;

    // Decrypt encrypted messages
    const decryptedMessages = await decryptMessages(messageList);

    // Initialize channel if needed
    if (!messagesState.byChannel[channelId]) {
      setMessagesState("byChannel", channelId, []);
    }

    // Prepend older messages (they come from server newest-first, but we want oldest-first)
    const reversed = [...decryptedMessages].reverse();
    const currentMessages = messagesState.byChannel[channelId] || [];
    setMessagesState("byChannel", channelId, [...reversed, ...currentMessages]);

    // Use server's pagination info
    setMessagesState("hasMore", channelId, response.has_more);
    setMessagesState("loadingChannels", channelId, false);
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to load messages:", error);
    setMessagesState("loadingChannels", channelId, false);
    setMessagesState("loadError", errorText(err, "chat.list.loadFailed"));
  }
}

/**
 * Load messages centered around a specific message ID.
 * Used when entering a channel with unreads to center on the read cursor.
 */
export async function loadMessagesAround(channelId: string, aroundMessageId: string): Promise<void> {
  if (messagesState.loadingChannels[channelId]) return;

  setMessagesState("loadingChannels", channelId, true);
  setMessagesState("loadError", null);

  try {
    const window = await fetchAroundWindow(channelId, aroundMessageId);
    setMessagesState("byChannel", channelId, window.items);
    setMessagesState("hasMore", channelId, window.hasMore);
    setMessagesState("loadingChannels", channelId, false);
  } catch (err) {
    reportLoadError(channelId, "Failed to load messages around:", err);
  }
}

function reportLoadError(channelId: string, label: string, err: unknown): void {
  const error = err instanceof Error ? err.message : String(err);
  console.error(label, error);
  setMessagesState("loadingChannels", channelId, false);
  setMessagesState("loadError", errorText(err, "chat.list.loadFailed"));
}

/** Fetch (without committing to the store) one page centred on a message. */
async function fetchAroundWindow(
  channelId: string,
  aroundMessageId: string,
): Promise<{ items: Message[]; hasMore: boolean }> {
  const response = await tauri.getMessages(channelId, undefined, MESSAGE_LIMIT, aroundMessageId);
  // Around-mode returns messages in ASC order (oldest-first) from server
  return { items: await decryptMessages(response.items), hasMore: response.has_more };
}

// Track ongoing initial loads to prevent duplicates
const initialLoadInProgress: Record<string, boolean> = {};

/**
 * Live (WebSocket) changes that arrive while a channel's initial load is in
 * flight. They are held back so `byChannel` is written once, by the commit,
 * rather than showing a lone live message as if it were the initial load.
 */
interface LiveBuffer {
  added: Message[];
  updated: Map<string, Message>;
  removed: Set<string>;
}
const liveBuffers: Record<string, LiveBuffer | undefined> = {};

/** Merge buffered live changes into `messages` and drop the buffer. */
function applyLiveBuffer(channelId: string, messages: Message[]): Message[] {
  const buf = liveBuffers[channelId];
  delete liveBuffers[channelId];
  if (!buf) return messages;
  let result = mergeAscending(messages, buf.added);
  if (buf.updated.size > 0) {
    result = result.map((m) => buf.updated.get(m.id) ?? m);
  }
  if (buf.removed.size > 0) {
    result = result.filter((m) => !buf.removed.has(m.id));
  }
  return result;
}

/**
 * Load initial messages for a channel (clears existing).
 * When lastReadMessageId is provided and the channel has unreads,
 * loads messages centered around the read cursor so the "New Messages"
 * divider is visible.
 */
export async function loadInitialMessages(
  channelId: string,
  lastReadMessageId?: string | null,
  unreadCount?: number,
): Promise<void> {
  // Prevent duplicate initial loads
  if (initialLoadInProgress[channelId]) {
    return;
  }

  initialLoadInProgress[channelId] = true;
  liveBuffers[channelId] = { added: [], updated: new Map(), removed: new Set() };

  try {
    setMessagesState("byChannel", channelId, []);
    setMessagesState("hasMore", channelId, true);

    // Only use around-mode if unreads exist AND count <= 200 threshold
    if (lastReadMessageId && (unreadCount == null || unreadCount <= 200)) {
      await loadInitialAround(channelId, lastReadMessageId);
    } else {
      await loadMessages(channelId);
    }
  } finally {
    initialLoadInProgress[channelId] = false;
    // Anything not consumed by a commit (fallback or error paths).
    if (liveBuffers[channelId]) {
      const current = messagesState.byChannel[channelId] ?? [];
      const merged = applyLiveBuffer(channelId, current);
      if (merged.length > 0 || current.length > 0) {
        setMessagesState("byChannel", channelId, merged);
      }
    }
  }
}

const CATCH_UP_PAGE_LIMIT = 100;

function isPendingMessage(message: Message): boolean {
  return message.id.startsWith("pending:");
}

/** Server order: `(created_at, id)` ascending. */
function isNewerThan(a: Message, b: Message): boolean {
  const ta = Date.parse(a.created_at);
  const tb = Date.parse(b.created_at);
  if (ta !== tb) return ta > tb;
  return a.id > b.id;
}

/**
 * The newest confirmed (non-pending) message id of a channel — the anchor a
 * reconnect catches up from. Capture it before re-subscribing: live messages
 * that arrive afterwards are appended and must not move the anchor past the gap.
 */
export function catchUpAnchor(channelId: string): string | undefined {
  const list = messagesState.byChannel[channelId] ?? [];
  for (let i = list.length - 1; i >= 0; i--) {
    if (!isPendingMessage(list[i])) return list[i].id;
  }
  return undefined;
}

/**
 * Insert `incoming` (ascending) into `current`, skipping known ids. Each item
 * goes before the first pending or newer message, so messages that arrived
 * live during the catch-up stay after the ones they followed.
 */
function mergeAscending(current: Message[], incoming: Message[]): Message[] {
  const result = [...current];
  const known = new Set(result.map((m) => m.id));
  for (const item of incoming) {
    if (known.has(item.id)) continue;
    known.add(item.id);
    const idx = result.findIndex(
      (m) => isPendingMessage(m) || isNewerThan(m, item),
    );
    if (idx === -1) result.push(item);
    else result.splice(idx, 0, item);
  }
  return result;
}

function isAnchorRejected(err: unknown): boolean {
  return (
    typeof err === "object" &&
    err !== null &&
    (err as { status?: unknown }).status === 400
  );
}

/**
 * Fetch every message after `afterId` (C1 `?after=`), ascending. Returns
 * `null` when the server rejects the anchor (400) or more than `maxPages`
 * pages are missing; other errors propagate.
 */
async function fetchMessagesAfter(
  channelId: string,
  afterId: string,
  maxPages: number,
): Promise<Message[] | null> {
  const missed: Message[] = [];
  let cursor = afterId;
  let pages = 0;
  let hasMore = true;
  try {
    while (hasMore && pages < maxPages) {
      const page = await tauri.getMessagesAfter(
        channelId,
        cursor,
        CATCH_UP_PAGE_LIMIT,
      );
      pages += 1;
      missed.push(...(await decryptMessages(page.items)));
      hasMore = page.has_more;
      // next_cursor still advances when block filtering emptied the page.
      const next = page.next_cursor ?? page.items[page.items.length - 1]?.id;
      if (hasMore && !next) break;
      if (next) cursor = next;
    }
  } catch (err) {
    if (!isAnchorRejected(err)) throw err;
    return null;
  }
  return hasMore ? null : missed;
}

/**
 * Around-mode initial load: fetch the window around the read cursor plus every
 * newer page, then commit `byChannel` ONCE so the list sees a single initial
 * load (no window-then-live-append sequence). `loadingChannels` stays true
 * until that write. Messages that arrived live meanwhile stay after the loaded
 * ones. A rejected anchor (400) or an exceeded page bound falls back to the
 * latest page; other forward errors commit the window alone. Runs inside
 * `loadInitialMessages`, so it must not call it again.
 */
async function loadInitialAround(
  channelId: string,
  lastReadMessageId: string,
): Promise<void> {
  if (messagesState.loadingChannels[channelId]) return;
  setMessagesState("loadingChannels", channelId, true);
  setMessagesState("loadError", null);

  let window: { items: Message[]; hasMore: boolean };
  try {
    window = await fetchAroundWindow(channelId, lastReadMessageId);
  } catch (err) {
    reportLoadError(channelId, "Failed to load messages around:", err);
    await loadMessages(channelId);
    return;
  }
  if (window.items.length === 0) {
    // Deleted anchor or other edge case.
    setMessagesState("loadingChannels", channelId, false);
    await loadMessages(channelId);
    return;
  }

  let forward: Message[] = [];
  const anchor = window.items[window.items.length - 1].id;
  try {
    const missed = await fetchMessagesAfter(channelId, anchor, 5);
    if (!missed) {
      setMessagesState("loadingChannels", channelId, false);
      setMessagesState("byChannel", channelId, []);
      setMessagesState("hasMore", channelId, true);
      await loadMessages(channelId);
      return;
    }
    forward = missed;
  } catch (err) {
    console.error("Failed to load messages after unread window:", err);
  }

  const current = messagesState.byChannel[channelId] ?? [];
  setMessagesState(
    "byChannel",
    channelId,
    applyLiveBuffer(channelId, mergeAscending(current, [...window.items, ...forward])),
  );
  setMessagesState("hasMore", channelId, window.hasMore);
  setMessagesState("loadingChannels", channelId, false);
}

/**
 * Fetch the messages missed while disconnected (C1 `?after=`) and merge them
 * into the cache, deduplicated by id. Falls back to a fresh load
 * (`"reloaded"`) when the cache is empty, the server rejects the anchor (400:
 * unknown or deleted from the channel), or more than `maxPages` pages are
 * missing. Other errors propagate.
 *
 * @param afterId anchor; defaults to the newest confirmed cached message.
 */
export async function catchUpChannel(
  channelId: string,
  maxPages = 5,
  afterId: string | undefined = catchUpAnchor(channelId),
): Promise<"caught_up" | "reloaded"> {
  if (!afterId) {
    await loadInitialMessages(channelId);
    return "reloaded";
  }

  const missed = await fetchMessagesAfter(channelId, afterId, maxPages);
  if (!missed) {
    await loadInitialMessages(channelId);
    return "reloaded";
  }

  const current = messagesState.byChannel[channelId] ?? [];
  setMessagesState("byChannel", channelId, mergeAscending(current, missed));
  return "caught_up";
}

/**
 * Send a message to a channel.
 */
export async function sendMessage(
  channelId: string,
  content: string
): Promise<Message | null> {
  const trimmedContent = content.trim();

  if (!trimmedContent) {
    return null;
  }

  setMessagesState({ sendError: null });

  // Build an optimistic (pending) message so the UI updates instantly
  const user = currentUser();
  const nonce = crypto.randomUUID();
  const pendingId = `pending:${nonce}`;
  const optimisticMessage: Message = {
    id: pendingId,
    nonce,
    channel_id: channelId,
    author: user
      ? { id: user.id, username: user.username, display_name: user.display_name, avatar_url: user.avatar_url, status: user.status }
      : { id: "", username: "You", display_name: "You", avatar_url: null, status: "online" },
    content: trimmedContent,
    encrypted: false,
    attachments: [],
    reply_to: null,
    parent_id: null,
    thread_reply_count: 0,
    thread_last_reply_at: null,
    edited_at: null,
    created_at: new Date().toISOString(),
    mention_type: null,
    pinned: false,
    message_type: "user",
  };

  // Show immediately
  const prev = messagesState.byChannel[channelId] || [];
  setMessagesState("byChannel", channelId, [...prev, optimisticMessage]);

  try {
    const { message, status } = await tauri.sendMessageWithStatus(
      channelId,
      trimmedContent,
      { nonce },
    );

    // Slash command invocations return 202 Accepted and are not persisted as messages.
    const suppressLocalEcho = status === 202;

    // Replace the optimistic message with the real one from the server
    const current = messagesState.byChannel[channelId] || [];
    if (suppressLocalEcho) {
      // Remove optimistic message for slash commands
      setMessagesState("byChannel", channelId, current.filter((m) => m.id !== pendingId));
    } else {
      // Swap pending for real — also deduplicate in case WebSocket echo arrived first
      const withoutPending = current.filter((m) => m.id !== pendingId);
      if (!withoutPending.some((m) => m.id === message.id)) {
        setMessagesState("byChannel", channelId, [...withoutPending, message]);
      } else {
        // WebSocket echo already added the real message; just drop the pending one
        setMessagesState("byChannel", channelId, withoutPending);
      }
    }

    // Mark channel as read (sending implies seeing)
    try {
      const { markChannelAsRead } = await import("@/stores/channels");
      markChannelAsRead(channelId, message.id);
    } catch {
      // Non-critical — don't fail the send
    }

    return message;
  } catch (err) {
    // Remove the optimistic message on failure
    const current = messagesState.byChannel[channelId] || [];
    setMessagesState("byChannel", channelId, current.filter((m) => m.id !== pendingId));

    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to send message:", error);
    const text = errorText(
      err,
      "chat.send.failed.message",
      CHAT_SEND_ERRORS.byCode,
      CHAT_SEND_ERRORS.byStatus,
    );
    showToast({
      type: "error",
      title: t("chat.send.failed.title"),
      message: text,
      duration: 8000,
    });
    setMessagesState({ sendError: text });
    return null;
  }
}

/**
 * Add a message received from WebSocket.
 * Decrypts the message if it's encrypted before adding to the store.
 */
export async function addMessage(message: Message): Promise<void> {
  const channelId = message.channel_id;

  // Early duplicate check (best-effort before async work)
  const earlyExisting = messagesState.byChannel[channelId] || [];
  if (earlyExisting.some((m) => m.id === message.id)) {
    return;
  }

  // Decrypt if needed (async — store may change during this await)
  const processedMessage = await decryptMessageIfNeeded(message);

  // Re-read store after await to avoid losing concurrent prepends from loadMessages
  const existing = messagesState.byChannel[channelId] || [];
  if (existing.some((m) => m.id === processedMessage.id)) {
    return;
  }
  // Initial load in flight: hold the message back for the commit.
  if (initialLoadInProgress[channelId]) {
    const buf = liveBuffers[channelId];
    if (buf && !buf.added.some((m) => m.id === processedMessage.id)) {
      buf.added.push(processedMessage);
    }
    return;
  }
  // If this is a confirmed echo of our own message, replace the matching pending placeholder by nonce
  const me = currentUser();
  if (me && processedMessage.author.id === me.id && processedMessage.nonce) {
    const pendingIdx = existing.findIndex(
      (m) => m.id.startsWith("pending:") && m.nonce === processedMessage.nonce,
    );
    if (pendingIdx !== -1) {
      const base = [...existing];
      base.splice(pendingIdx, 1);
      setMessagesState("byChannel", channelId, [...base, processedMessage]);
      return;
    }
  }
  setMessagesState("byChannel", channelId, [...existing, processedMessage]);
}

/**
 * Update an existing message (for edits).
 */
export function updateMessage(message: Message): void {
  const channelId = message.channel_id;
  const buf = initialLoadInProgress[channelId] ? liveBuffers[channelId] : undefined;
  if (buf) {
    const i = buf.added.findIndex((m) => m.id === message.id);
    if (i !== -1) buf.added[i] = message;
    else buf.updated.set(message.id, message);
  }
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
  const buf = initialLoadInProgress[channelId] ? liveBuffers[channelId] : undefined;
  if (buf) {
    buf.added = buf.added.filter((m) => m.id !== messageId);
    buf.updated.delete(messageId);
    buf.removed.add(messageId);
  }
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

// Export the store for reading and writing (setMessagesState is used by websocket.ts for reaction updates)
export { messagesState, setMessagesState };
