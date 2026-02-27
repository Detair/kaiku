/**
 * Messages Store
 *
 * Manages message state for channels including loading, sending, and real-time updates.
 * Supports E2EE for DM channels when encryption is initialized.
 */

import { createStore } from "solid-js/store";
import type { Message, ClaimedPrekeyInput, DMListItem, E2EEContent, MegolmE2EEContent } from "@/lib/types";
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

/**
 * Decrypt a message if it's encrypted and E2EE is available.
 * Supports both 1:1 Olm messages and Megolm group messages.
 * Also processes Megolm session key distribution messages automatically.
 * Returns the message with decrypted content, or a placeholder if decryption fails.
 */
async function decryptMessageIfNeeded(message: Message): Promise<Message> {
  // If not encrypted, return as-is
  if (!message.encrypted) {
    return message;
  }

  // Check if E2EE is initialized
  const status = e2eeStore.status();
  if (!status.initialized) {
    console.warn("[E2EE] Message is encrypted but E2EE not initialized");
    return { ...message, content: "[Encrypted message - E2EE not initialized]" };
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
      return { ...message, content: "[Unable to decrypt - identity not available]" };
    }

    const ourUserId = user.id;

    // Find our ciphertext: recipients[our_user_id][our_curve25519_key]
    const userDevices = e2ee.recipients[ourUserId];
    if (!userDevices) {
      console.warn("[E2EE] No ciphertext for our user ID:", ourUserId);
      return { ...message, content: "[Unable to decrypt - no ciphertext for your account]" };
    }

    const ourCiphertext = userDevices[ourKey];
    if (!ourCiphertext) {
      console.warn("[E2EE] No ciphertext for our device key");
      return { ...message, content: "[Unable to decrypt - no ciphertext for this device]" };
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
    return { ...message, content: "[Unable to decrypt message]" };
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
  const trimmedContent = content.trim();

  if (!trimmedContent) {
    return null;
  }

  setMessagesState({ error: null });

  try {
    const { message, status } = await tauri.sendMessageWithStatus(
      channelId,
      trimmedContent
    );

    // Slash command invocations return 202 Accepted and are not persisted as messages.
    const suppressLocalEcho = status === 202;

    // Add the sent message to the store (check for duplicates since WebSocket may have already added it)
    const prev = messagesState.byChannel[channelId] || [];
    if (!suppressLocalEcho && !prev.some((m) => m.id === message.id)) {
      setMessagesState("byChannel", channelId, [...prev, message]);
    }

    return message;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to send message:", error);
    showToast({ type: "error", title: "Send Failed", message: "Could not send message. Please try again.", duration: 8000 });
    setMessagesState({ error });
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
  setMessagesState("byChannel", channelId, [...existing, processedMessage]);
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

// ============================================================================
// E2EE DM Functions
// ============================================================================

/**
 * Claim prekeys for all participants in a DM.
 * Returns the claimed prekeys needed for encryption.
 */
async function claimPrekeysForRecipients(
  recipientUserIds: string[]
): Promise<ClaimedPrekeyInput[]> {
  const claimedKeys: ClaimedPrekeyInput[] = [];

  for (const userId of recipientUserIds) {
    try {
      // Get the user's devices
      const userKeys = await tauri.getUserKeys(userId);

      if (userKeys.devices.length === 0) {
        console.warn(`[E2EE] User ${userId} has no registered devices`);
        continue;
      }

      // Claim prekeys from each device
      for (const device of userKeys.devices) {
        try {
          const claimed = await tauri.claimPrekey(userId, device.device_id);
          claimedKeys.push({
            user_id: userId,
            device_id: claimed.device_id,
            identity_key_ed25519: claimed.identity_key_ed25519,
            identity_key_curve25519: claimed.identity_key_curve25519,
            one_time_prekey: claimed.one_time_prekey,
          });
        } catch (err) {
          console.warn(`[E2EE] Failed to claim prekey for device ${device.device_id}:`, err);
        }
      }
    } catch (err) {
      console.warn(`[E2EE] Failed to get keys for user ${userId}:`, err);
    }
  }

  return claimedKeys;
}

/**
 * Send an encrypted message to a DM channel.
 * This is used internally by sendDMMessage when E2EE is available.
 *
 * @param channelId - The DM channel ID
 * @param content - The plaintext message content
 * @param recipientUserIds - The user IDs of the DM participants (excluding self)
 * @returns The sent message or null on failure
 */
export async function sendEncryptedDM(
  channelId: string,
  content: string,
  recipientUserIds: string[]
): Promise<Message | null> {
  if (!content.trim()) {
    return null;
  }

  // Check E2EE status
  const status = e2eeStore.status();
  if (!status.initialized) {
    throw new Error("E2EE not initialized - cannot send encrypted message");
  }

  setMessagesState({ error: null });

  try {
    // Claim prekeys for all recipients
    const recipients = await claimPrekeysForRecipients(recipientUserIds);

    if (recipients.length === 0) {
      throw new Error("No recipients available for encryption - they may not have E2EE enabled");
    }

    // Encrypt the message
    const encrypted = await e2eeStore.encrypt(content.trim(), recipients);

    // Send the encrypted message with the encrypted flag set
    // The content contains the encrypted E2EE payload (JSON serialized)
    const encryptedContent = JSON.stringify(encrypted);
    const message = await tauri.sendMessage(channelId, encryptedContent, {
      encrypted: true,
    });

    // Note: The actual message content will be the encrypted JSON
    // The server stores it as-is and clients decrypt on receive

    // Add to local store
    const prev = messagesState.byChannel[channelId] || [];
    if (!prev.some((m) => m.id === message.id)) {
      setMessagesState("byChannel", channelId, [...prev, message]);
    }

    return message;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to send encrypted message:", error);
    showToast({ type: "error", title: "Send Failed", message: "Could not send encrypted message. Please try again.", duration: 8000 });
    setMessagesState({ error });
    return null;
  }
}

/**
 * Send a message to a DM channel, automatically using E2EE if available.
 * For group DMs (3+ participants), uses Megolm group encryption.
 * For 1:1 DMs (2 participants), uses Olm 1:1 encryption.
 *
 * @param channelId - The DM channel ID
 * @param content - The plaintext message content
 * @param dm - The DM channel info with participant data
 * @param currentUserId - The current user's ID (to exclude from encryption recipients)
 * @returns The sent message or null on failure
 */
export async function sendDMMessage(
  channelId: string,
  content: string,
  dm: DMListItem,
  currentUserId: string
): Promise<Message | null> {
  if (!content.trim()) {
    return null;
  }

  // Check if E2EE is initialized
  const status = e2eeStore.status();

  if (status.initialized) {
    // Get recipient IDs (all participants except current user)
    const recipientUserIds = dm.participants
      .map((p) => p.user_id)
      .filter((id) => id !== currentUserId);

    if (recipientUserIds.length > 0) {
      try {
        // Group DMs (3+ participants including self) use Megolm
        if (dm.participants.length > 2) {
          return await sendEncryptedGroupDM(channelId, content, recipientUserIds);
        }
        // 1:1 DMs use Olm
        return await sendEncryptedDM(channelId, content, recipientUserIds);
      } catch (err) {
        // Log but fall back to unencrypted
        console.warn("[E2EE] Encryption failed, falling back to unencrypted:", err);
      }
    }
  }

  // Fall back to unencrypted message
  return sendMessage(channelId, content);
}

// ============================================================================
// Megolm Group E2EE Functions
// ============================================================================

/**
 * Tracks the outbound Megolm session state per room.
 * Sessions are reused until the message count reaches the rotation limit
 * or the participant list changes.
 */
interface MegolmSessionState {
  /** Number of messages encrypted with this session. */
  messageCount: number;
  /** Sorted participant IDs at the time the session was created. */
  participantHash: string;
}

/** Rotate the outbound session after this many messages (Matrix uses 100). */
const MEGOLM_ROTATION_LIMIT = 100;

/** In-memory cache of outbound Megolm session state. Keyed by channelId. */
const megolmSessionCache: Map<string, MegolmSessionState> = new Map();

/**
 * Compute a simple hash of the sorted participant list.
 * Used to detect when members change and a new session is needed.
 */
function participantHash(recipientUserIds: string[]): string {
  return [...recipientUserIds].sort().join(",");
}

/**
 * Send an encrypted group message using Megolm.
 *
 * Flow:
 * 1. Create or reuse Megolm outbound session for this channel
 * 2. If new session: distribute the session key to all members via 1:1 Olm messages
 * 3. Encrypt the actual message with Megolm
 * 4. Send the encrypted payload with the `encrypted` flag
 *
 * @param channelId - The channel/room ID
 * @param content - The plaintext message content
 * @param recipientUserIds - The user IDs of the members (excluding self)
 * @returns The sent message or null on failure
 */
export async function sendEncryptedGroupDM(
  channelId: string,
  content: string,
  recipientUserIds: string[]
): Promise<Message | null> {
  if (!content.trim()) {
    return null;
  }

  const status = e2eeStore.status();
  if (!status.initialized) {
    throw new Error("E2EE not initialized - cannot send encrypted group message");
  }

  setMessagesState({ error: null });

  try {
    // Step 1: Determine if we need a new session or can reuse the existing one.
    const currentHash = participantHash(recipientUserIds);
    const cached = megolmSessionCache.get(channelId);
    const needsNewSession =
      !cached ||
      cached.messageCount >= MEGOLM_ROTATION_LIMIT ||
      cached.participantHash !== currentHash;

    if (needsNewSession) {
      // Create a new outbound Megolm session
      const sessionKey = await e2eeStore.createGroupSession(channelId);

      // Distribute the session key to all group members via 1:1 Olm messages
      await distributeGroupSessionKey(channelId, sessionKey, recipientUserIds);

      // Cache the new session state
      megolmSessionCache.set(channelId, {
        messageCount: 0,
        participantHash: currentHash,
      });

      console.log(`[E2EE/Megolm] Created new outbound session for room ${channelId}`);
    }

    // Step 2: Encrypt the actual message with Megolm.
    const megolmCiphertext = await e2eeStore.encryptGroup(channelId, content.trim());

    // Increment message counter
    const state = megolmSessionCache.get(channelId)!;
    state.messageCount++;

    // Step 3: Get our sender key for the content envelope.
    const senderKey = await tauri.getOurCurve25519Key();
    if (!senderKey) {
      throw new Error("Cannot get our Curve25519 key");
    }

    // Build the Megolm E2EE content envelope.
    const megolmContent: MegolmE2EEContent = {
      sender_key: senderKey,
      room_id: channelId,
      megolm_ciphertext: megolmCiphertext,
    };

    // Step 4: Send the encrypted message.
    const encryptedContent = JSON.stringify(megolmContent);
    const message = await tauri.sendMessage(channelId, encryptedContent, {
      encrypted: true,
    });

    // Add to local store
    const prev = messagesState.byChannel[channelId] || [];
    if (!prev.some((m) => m.id === message.id)) {
      setMessagesState("byChannel", channelId, [...prev, message]);
    }

    return message;
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    console.error("Failed to send encrypted group message:", error);
    showToast({ type: "error", title: "Send Failed", message: "Could not send encrypted group message. Please try again.", duration: 8000 });
    setMessagesState({ error });
    return null;
  }
}

/**
 * Distribute a Megolm session key to all group members via 1:1 Olm.
 *
 * Sends the session key as an actual encrypted message to the channel
 * so recipients receive it through the normal WebSocket `message_new` flow.
 * The key payload uses a well-known `__megolm_session_key__` marker so
 * receivers automatically detect and process it in `decryptMessageIfNeeded()`.
 */
async function distributeGroupSessionKey(
  roomId: string,
  sessionKey: string,
  recipientUserIds: string[]
): Promise<void> {
  // Build the key-sharing payload
  const keyPayload = JSON.stringify({
    type: "__megolm_session_key__",
    room_id: roomId,
    session_key: sessionKey,
  });

  // Claim prekeys and encrypt for each recipient
  const recipients = await claimPrekeysForRecipients(recipientUserIds);

  if (recipients.length === 0) {
    console.warn("[E2EE/Megolm] No recipients available for session key distribution");
    return;
  }

  // Encrypt the session key payload via Olm for each recipient's devices
  const encrypted = await e2eeStore.encrypt(keyPayload, recipients);

  // Send the Olm-encrypted session key as a real message to the channel.
  // This ensures recipients receive it via WebSocket `message_new` events
  // and can automatically process it in `decryptMessageIfNeeded()`.
  const encryptedContent = JSON.stringify(encrypted);
  await tauri.sendMessage(roomId, encryptedContent, { encrypted: true });

  // Also store the session key as an inbound session for ourselves
  // so we can decrypt our own Megolm messages (e.g., on other devices)
  const ourKey = await getOurCurve25519Key();
  if (ourKey) {
    await e2eeStore.addInboundSession(roomId, ourKey, sessionKey);
  }

  console.log(`[E2EE/Megolm] Distributed session key to ${recipients.length} device(s) for room ${roomId}`);
}

/**
 * Check if E2EE is available for sending encrypted messages.
 */
export function isE2EEAvailable(): boolean {
  return e2eeStore.status().initialized;
}

// Export the store for reading and writing (setMessagesState is used by websocket.ts for reaction updates)
export { messagesState, setMessagesState };
