/**
 * WebSocket Store
 *
 * Manages WebSocket connection and routes events to appropriate stores.
 */

import { createStore } from "solid-js/store";
import * as tauri from "@/lib/tauri";
import type { Message, ServerEvent } from "@/lib/types";
import {
  updateUserActivity,
  updateUserCustomStatus,
  updateUserPresence,
} from "../presence";
import {
  addMessage,
  removeMessage,
  messagesState,
  loadInitialMessages,
  catchUpChannel,
  catchUpAnchor,
  clearChannelMessages,
} from "../messages";
import { handlePreferencesUpdated } from "../preferences";
import {
  receiveIncomingCall,
  callConnected,
  callEndedExternally,
  participantJoined,
  participantLeft,
  callState,
  type EndReason,
} from "../call";
import {
  loadFriends,
  loadPendingRequests,
  handleUserBlocked,
  handleUserUnblocked,
} from "../friends";
import {
  getChannel,
  channelsState,
  handleChannelReadEvent,
  incrementUnreadCount,
  updateChannelLastMessageId,
  refreshChannelsForGuild,
} from "../channels";
import { currentUser } from "../auth";
import {
  guildsState,
  getGuildIdForChannel,
  incrementGuildUnread,
  DISCOVERY_SENTINEL,
} from "../guilds";
import {
  dmsState,
  handleDMReadEvent,
  handleDMNameUpdated,
  updateDMLastMessage,
  loadDMs,
  handleDmCreated,
} from "../dms";
import { handlePinAdded, handlePinRemoved } from "../channelPins";

import {
  applyMessageEdit,
  handleMessageNotification,
  handleThreadRead,
  handleThreadReplyDelete,
  handleThreadReplyNew,
} from "./messageEvents";
import {
  handleVoiceIceCandidate,
  handleVoiceLayerChanged,
  handleVoicePublisherAnswer,
  handleVoiceRoomState,
  handleVoiceSubscriberOffer,
  handleVoiceUserJoined,
  handleVoiceUserLeft,
  handleVoiceUserMuted,
  handleVoiceUserStatsEvent,
  handleVoiceUserUnmuted,
} from "./voiceEvents";
import {
  handleScreenShareQualityChanged,
  handleScreenShareStarted,
  handleScreenShareStopped,
  handleWebcamStarted,
  handleWebcamStopped,
} from "./mediaEvents";
import {
  handleAdminGuildDeleted,
  handleAdminGuildSuspended,
  handleAdminGuildUnsuspended,
  handleAdminReportCreated,
  handleAdminReportResolved,
  handleAdminUserBanned,
  handleAdminUserDeleted,
  handleAdminUserUnbanned,
} from "./adminEvents";
import {
  handleGuildEmojiUpdated,
  handlePatchEvent,
} from "./guildEvents";
import {
  handleReactionAdd,
  handleReactionRemove,
  updateMessagePinStatus,
} from "./reactionEvents";

// Re-export media handlers so existing test imports
// (`import { handleScreenShareStarted } from "@/stores/websocket"`) keep working.
export {
  handleScreenShareStarted,
  handleScreenShareStopped,
  handleScreenShareQualityChanged,
  handleWebcamStarted,
  handleWebcamStopped,
};

import * as Sentry from "@sentry/browser";
import {
  createConnectionSupervisor,
  type ConnectionDriver,
  type ConnectionState,
  type ConnectionSupervisor,
} from "./connection";
import { isTauri } from "@/lib/tauri/common";

export type { ConnectionState } from "./connection";

// Type for unlisten function
type UnlistenFn = () => void;

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

// Track connection start time for WS connect duration
let connectStartTime = 0;

// Reconnect supervisor (browser: managed, native: mirrors Rust) and, in the
// browser, the one message handler attached to the current socket.
let supervisor: ConnectionSupervisor | null = null;
let detachBrowserMessageHandler: (() => void) | null = null;

// --- C2 resync state ------------------------------------------------------

/** How long reconnect recovery waits for `ready` and for `subscribed` acks. */
const RESYNC_WAIT_MS = 3000;
/** Longer gaps reload the open channel instead of paging through the gap. */
const MAX_CATCH_UP_GAP_MS = 7 * 24 * 60 * 60 * 1000;

/** `server_time` of the latest `ready`, and of the one before it. */
let resyncWatermark: string | null = null;
let previousWatermark: string | null = null;
/** Whether the current connection has delivered `ready` yet. */
let readyReceived = false;
let readyWaiters: Array<() => void> = [];
const subscribeAckWaiters = new Map<string, Set<() => void>>();

/** Last `Ready.server_time` (RFC 3339), or null before the first `ready`. */
export function getResyncWatermark(): string | null {
  return resyncWatermark;
}

function handleReady(serverTime: string | undefined): void {
  if (serverTime) {
    previousWatermark = resyncWatermark;
    resyncWatermark = serverTime;
  }
  readyReceived = true;
  const waiters = readyWaiters;
  readyWaiters = [];
  for (const resolve of waiters) resolve();
}

/** Resolves true once this connection's `ready` arrived, false after `ms`. */
function waitForReady(ms: number): Promise<boolean> {
  if (readyReceived) return Promise.resolve(true);
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      resolve(true);
    };
    const timer = setTimeout(() => {
      readyWaiters = readyWaiters.filter((w) => w !== done);
      resolve(false);
    }, ms);
    readyWaiters.push(done);
  });
}

/**
 * Resolves true on the next `subscribed` ack for `channelId`, false after
 * `ms` (e.g. the server refused the subscription). Register before sending.
 */
function waitForSubscribeAck(channelId: string, ms: number): Promise<boolean> {
  return new Promise((resolve) => {
    let waiters = subscribeAckWaiters.get(channelId);
    if (!waiters) {
      waiters = new Set();
      subscribeAckWaiters.set(channelId, waiters);
    }
    const settle = (acked: boolean) => {
      clearTimeout(timer);
      const set = subscribeAckWaiters.get(channelId);
      set?.delete(done);
      if (set?.size === 0) subscribeAckWaiters.delete(channelId);
      resolve(acked);
    };
    const done = () => settle(true);
    const timer = setTimeout(() => settle(false), ms);
    waiters.add(done);
  });
}

function handleSubscribedAck(channelId: string): void {
  const waiters = subscribeAckWaiters.get(channelId);
  if (!waiters) return;
  for (const resolve of [...waiters]) resolve();
}

/**
 * Initialize WebSocket event listeners.
 * Call this once when the app starts (after auth).
 */
export async function initWebSocket(): Promise<void> {
  // Clean up existing listeners
  await cleanupWebSocket();

  if (isTauri()) {
    // Native mode: Rust owns the socket and its retries. Every server frame
    // arrives once as `ws:server_event` and goes through the same handler as
    // in the browser; the status events drive the supervisor.
    const { listen } = await import("@tauri-apps/api/event");
    const supervisor = getSupervisor();
    unlisteners.push(
      ...(await Promise.all([
        listen<ServerEvent>("ws:server_event", (event) => {
          handleServerEvent(event.payload).catch((err) =>
            console.error("[WS] Failed to handle server event:", err),
          );
        }),
        listen("ws:connecting", () => {
          Sentry.addBreadcrumb({ category: "ws", message: "connecting", level: "info" });
        }),
        listen("ws:connected", () => {
          Sentry.addBreadcrumb({
            category: "ws",
            message: "connected",
            data: { duration_ms: Date.now() - connectStartTime },
            level: "info",
          });
          // After ws:reconnecting this runs handleReconnect (onRecovered).
          supervisor.notifyOpened();
        }),
        listen<number>("ws:reconnecting", (event) => {
          supervisor.notifyClosed(false);
          if (wsState.status === "reconnecting") {
            setWsState({ reconnectAttempt: event.payload });
          }
        }),
        // Rust ended the loop by itself (no session any more).
        listen("ws:disconnected", () => supervisor.stop()),
        listen("ws:auth_rejected", () => handleNativeAuthRejected()),
      ])),
    );
  } else {
    // Browser mode: the handler is (re-)attached on every open by the
    // reconnect supervisor's driver; attach now if a socket already exists.
    if (tauri.getBrowserWebSocket()) attachBrowserMessageHandler();
  }
}

/**
 * Attach the server-event handler to the current browser socket, removing the
 * one attached to any previous socket. Exactly one handler exists at a time.
 */
function attachBrowserMessageHandler(): void {
  detachBrowserMessageHandler?.();
  detachBrowserMessageHandler = null;

  const ws = tauri.getBrowserWebSocket();
  if (!ws) {
    console.warn("[WebSocket] No WebSocket instance found");
    return;
  }
  const messageHandler = async (event: MessageEvent) => {
    try {
      const data = JSON.parse(event.data) as ServerEvent;
      await handleServerEvent(data);
    } catch (err) {
      console.error("Failed to parse WebSocket message:", err);
    }
  };
  ws.addEventListener("message", messageHandler);
  detachBrowserMessageHandler = () =>
    ws.removeEventListener("message", messageHandler);
}

/**
 * Handle server events in browser mode.
 * IMPORTANT: Voice events are handled asynchronously to ensure proper ICE candidate processing.
 */
export async function handleServerEvent(event: ServerEvent): Promise<void> {
  console.log("[WebSocket] Received event:", event.type);

  switch (event.type) {
    case "message_new":
      await addMessage(event.message);
      updateDMLastMessage(event.channel_id, event.message);
      handleMessageNotification(event.message);
      // Increment unread count if message is not in the selected channel
      {
        const isActiveGuildChannel = event.channel_id === channelsState.selectedChannelId;
        const isActiveDM =
          event.channel_id === dmsState.selectedDMId && !dmsState.isShowingFriends;

        if (!isActiveGuildChannel && !isActiveDM) {
          const channel = getChannel(event.channel_id);
          if (channel && channel.guild_id && channel.channel_type === "text") {
            incrementUnreadCount(event.channel_id);
          }
        // Increment guild-level unread for non-active guilds
        const guildId = getGuildIdForChannel(event.channel_id);
        if (guildId && guildId !== guildsState.activeGuildId) {
          incrementGuildUnread(guildId);
        }
        // Notify unread module of new message
        window.dispatchEvent(new CustomEvent("unread-update"));
        }
        // Always update last_message_id for the channel, even if it's the active channel
        updateChannelLastMessageId(event.channel_id, event.message.id);
      }
      break;

    case "ready":
      handleReady(event.server_time);
      break;

    case "subscribed":
      handleSubscribedAck(event.channel_id);
      break;

    case "unsubscribed":
      break;

    case "dm_created":
      handleDmCreated(event.channel);
      break;

    case "message_edit":
      await applyMessageEdit(event.channel_id, event.message_id, event.content, event.edited_at);
      break;

    case "message_delete":
      removeMessage(event.channel_id, event.message_id);
      break;

    case "typing_start":
      addTypingUser(event.channel_id, event.user_id);
      break;

    case "typing_stop":
      removeTypingUser(event.channel_id, event.user_id);
      break;

    case "presence_update":
      updateUserPresence(event.user_id, event.status);
      break;

    case "rich_presence_update":
      console.log("Rich presence update:", event.user_id, event.activity);
      updateUserActivity(event.user_id, event.activity);
      break;

    case "custom_status_update":
      updateUserCustomStatus(event.user_id, event.custom_status);
      break;

    case "voice_publisher_answer":
      console.log(
        "[WebSocket] Handling voice_publisher_answer for channel:",
        event.channel_id,
      );
      await handleVoicePublisherAnswer(event.channel_id, event.sdp);
      break;

    case "voice_subscriber_offer":
      console.log(
        "[WebSocket] Handling voice_subscriber_offer for channel:",
        event.channel_id,
      );
      await handleVoiceSubscriberOffer(event.channel_id, event.sdp);
      break;

    case "voice_ice_candidate":
      // ICE candidates must be processed immediately for NAT traversal
      await handleVoiceIceCandidate(event.channel_id, event.candidate, event.pc_type || "publisher");
      break;

    case "voice_user_joined":
      await handleVoiceUserJoined(
        event.channel_id,
        event.user_id,
        event.username,
        event.display_name,
      );
      break;

    case "voice_user_left":
      await handleVoiceUserLeft(event.channel_id, event.user_id);
      break;

    case "voice_user_muted":
      await handleVoiceUserMuted(event.channel_id, event.user_id);
      break;

    case "voice_user_unmuted":
      await handleVoiceUserUnmuted(event.channel_id, event.user_id);
      break;

    case "voice_room_state":
      await handleVoiceRoomState(
        event.channel_id,
        event.participants,
        event.screen_shares,
        event.webcams,
      );
      break;

    case "screen_share_started":
      await handleScreenShareStarted(event);
      break;

    case "screen_share_stopped":
      await handleScreenShareStopped(event);
      break;

    case "screen_share_quality_changed":
      await handleScreenShareQualityChanged(event);
      break;

    case "webcam_started":
      await handleWebcamStarted(event);
      break;

    case "webcam_stopped":
      await handleWebcamStopped(event);
      break;

    case "voice_error":
      console.error("Voice error:", event.code, event.message);

      // Auto-retry for "Already in voice channel" error
      if (event.message === "Already in voice channel") {
        const { voiceState } = await import("@/stores/voice");
        const channelId = voiceState.channelId;

        if (channelId) {
          console.log(
            "[WebSocket] Auto-retry: leaving and rejoining channel",
            channelId,
          );

          // Send leave message
          await tauri.wsSend({
            type: "voice_leave",
            channel_id: channelId,
          });

          // Wait a bit for server to process leave
          await new Promise((resolve) => setTimeout(resolve, 150));

          // Retry join
          await tauri.wsSend({
            type: "voice_join",
            channel_id: channelId,
          });
        }
      }
      break;

    // Call events
    case "incoming_call": {
      const me = currentUser();
      if (me && event.initiator === me.id) {
        break;
      }
      console.log("[WebSocket] Incoming call from:", event.initiator_name);
      receiveIncomingCall(
        event.channel_id,
        event.initiator,
        event.initiator_name,
      );
      break;
    }

    case "call_started":
      console.log("[WebSocket] Call started in channel:", event.channel_id);
      // Call started means the initiator is now connected
      // Other participants will receive incoming_call
      break;

    case "call_ended":
      console.log("[WebSocket] Call ended:", event.reason);
      callEndedExternally(
        event.channel_id,
        event.reason as EndReason,
        event.duration_secs ?? undefined,
      );
      break;

    case "call_participant_joined":
      console.log("[WebSocket] Participant joined call:", event.username);
      participantJoined(event.channel_id, event.user_id);
      // Only transition to connected on the first join (not on subsequent participants)
      {
        const status = callState.currentCall.status;
        if (status === "outgoing_ringing" || status === "connecting") {
          callConnected(event.channel_id, [event.user_id]);
        }
      }
      break;

    case "call_participant_left":
      console.log("[WebSocket] Participant left call:", event.user_id);
      participantLeft(event.channel_id, event.user_id);
      break;

    case "call_declined":
      console.log("[WebSocket] Call declined by:", event.user_id);
      // The call store will handle this through the API response
      // This event is informational for other participants
      break;

    case "voice_user_stats":
      await handleVoiceUserStatsEvent(event);
      break;

    case "voice_layer_changed":
      await handleVoiceLayerChanged(event);
      break;

    // Admin events
    case "admin_user_banned":
      await handleAdminUserBanned(event.user_id, event.username);
      break;

    case "admin_user_unbanned":
      await handleAdminUserUnbanned(event.user_id, event.username);
      break;

    case "admin_guild_suspended":
      await handleAdminGuildSuspended(event.guild_id, event.guild_name);
      break;

    case "admin_guild_unsuspended":
      await handleAdminGuildUnsuspended(event.guild_id, event.guild_name);
      break;

    case "admin_user_deleted":
      await handleAdminUserDeleted(event.user_id, event.username);
      break;

    case "admin_guild_deleted":
      await handleAdminGuildDeleted(event.guild_id, event.guild_name);
      break;

    // DM read sync event
    case "dm_read":
      handleDMReadEvent(event.channel_id, event.last_read_message_id);
      break;

    // DM name updated
    case "dm_name_updated":
      handleDMNameUpdated(event.channel_id, event.name);
      break;

    // Guild channel read sync event
    case "channel_read":
      handleChannelReadEvent(event.channel_id, event.last_read_message_id);
      break;

    // Preferences events
    case "preferences_updated":
      handlePreferencesUpdated(event);
      break;

    // Reaction events
    case "reaction_add":
      handleReactionAdd(
        event.channel_id,
        event.message_id,
        event.user_id,
        event.emoji,
      );
      break;

    case "reaction_remove":
      handleReactionRemove(
        event.channel_id,
        event.message_id,
        event.user_id,
        event.emoji,
      );
      break;

    // Channel pin events
    case "channel_pin_added":
      handlePinAdded(
        event.channel_id,
        event.message_id,
        event.pinned_by,
        event.pinned_at,
      );
      // Update message pinned status in messages store
      updateMessagePinStatus(event.channel_id, event.message_id, true);
      break;

    case "channel_pin_removed":
      handlePinRemoved(event.channel_id, event.message_id);
      // Update message pinned status in messages store
      updateMessagePinStatus(event.channel_id, event.message_id, false);
      break;

    // Guild emoji events
    case "guild_emoji_updated":
      handleGuildEmojiUpdated(event.guild_id, event.emojis);
      break;

    case "member_roles_updated": {
      void import("@/stores/members").then(({ applyMemberRoles }) =>
        applyMemberRoles(event.guild_id, event.user_id, event.role_ids),
      );
      break;
    }

    case "guild_event_reminder": {
      const { name } = event;
      void import("@/components/ui/Toast").then(({ showToast }) =>
        showToast({
          type: "info",
          title: "Event starting soon",
          message: name,
          duration: 8000,
          id: `event-reminder-${event.event_id}`,
        }),
      );
      break;
    }

    // Event list refreshes when the Events modal is (re)opened.
    case "guild_event_created":
    case "guild_event_updated":
    case "guild_event_cancelled":
      break;

    // Friend events
    case "friend_request_received":
      // New incoming friend request — refresh pending list
      loadPendingRequests();
      break;

    case "friend_request_accepted":
      // Someone accepted our friend request — refresh both lists
      Promise.all([loadFriends(), loadPendingRequests()]);
      break;

    case "friend_request_rejected":
      // Other party rejected/cancelled — refresh pending list
      loadPendingRequests();
      break;

    // Block events
    case "user_blocked":
      handleUserBlocked(event.user_id);
      break;

    case "user_unblocked":
      handleUserUnblocked(event.user_id);
      break;

    // Admin report events
    case "admin_report_created":
      await handleAdminReportCreated(
        event.report_id,
        event.category,
        event.target_type,
      );
      break;

    case "admin_report_resolved":
      await handleAdminReportResolved(event.report_id);
      break;

    // Thread events
    case "thread_reply_new":
      handleThreadReplyNew(
        event.channel_id,
        event.parent_id,
        event.message,
        event.thread_info,
      );
      break;

    case "thread_reply_delete":
      handleThreadReplyDelete(
        event.channel_id,
        event.parent_id,
        event.message_id,
        event.thread_info,
      );
      break;

    case "thread_read":
      handleThreadRead(event.thread_parent_id, event.last_read_message_id);
      break;

    // State sync events
    case "patch":
      await handlePatchEvent(event.entity_type, event.entity_id, event.diff);
      break;

    // Bot command response events
    case "command_response":
      if (event.ephemeral) {
        // For ephemeral responses, create a local system message
        console.log(
          "[WebSocket] Ephemeral command response:",
          event.command_name,
          event.content,
        );
        const syntheticMessage: Message = {
          id: crypto.randomUUID(),
          channel_id: event.channel_id,
          author: {
            id: "system",
            username: event.bot_name,
            display_name: event.bot_name,
            avatar_url: null,
            status: "online",
          },
          content: event.content,
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
        await addMessage(syntheticMessage);
      } else {
        // Non-ephemeral responses arrive as regular message_new events
        console.log(
          "[WebSocket] Non-ephemeral command response (handled via message_new):",
          event.command_name,
        );
      }
      break;

    case "command_response_timeout": {
      console.warn("[WebSocket] Command response timeout:", event.command_name);
      const { showToast } = await import("@/components/ui/Toast");
      showToast({
        type: "warning",
        title: "Command Timeout",
        message: `Command /${event.command_name} did not respond within 30 seconds.`,
        duration: 5000,
        id: `cmd-timeout-${event.command_name}`,
      });
      break;
    }

    case "pong":
      // Watchdog ping reply; liveness is tracked by the socket hooks.
      break;

    case "error":
      console.error("WebSocket error:", event.code, event.message);
      setWsState({ error: event.message });
      break;

    default:
      // Exhaustive today; keeps logging event types the client predates.
      console.log("Unhandled server event:", (event as { type: string }).type);
  }
}

/**
 * Cleanup WebSocket listeners.
 */
export async function cleanupWebSocket(): Promise<void> {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
  detachBrowserMessageHandler?.();
  detachBrowserMessageHandler = null;

  // Clear typing timers
  for (const timer of Object.values(typingTimers)) {
    clearTimeout(timer);
  }
}

/**
 * Text channels of the active guild that are in the channel list. A guild
 * opened while the socket was down got its channels over REST, but
 * `loadChannelsForGuild` skipped the subscriptions; they are not in the
 * subscribed set, so the reconnect must add them.
 */
function activeGuildTextChannelIds(): string[] {
  const guildId = guildsState.activeGuildId;
  if (!guildId || guildId === DISCOVERY_SENTINEL) return [];
  return (channelsState.channels ?? [])
    .filter((c) => c.guild_id === guildId && c.channel_type === "text")
    .map((c) => c.id);
}

function subscribeAll(ids: Iterable<string>): Promise<unknown> {
  return Promise.all(
    [...ids].map((id) =>
      subscribeChannel(id).catch((err) =>
        console.error("[WS] Failed to re-subscribe channel", id, err),
      ),
    ),
  );
}

/**
 * Refresh channel/DM unread metadata after a reconnect. Exported for tests.
 *
 * - Active guild: re-fetch its channel list (selection-preserving) so the
 *   sidebar unread dots reflect what happened while offline, then subscribe
 *   its text channels (no-op for the ones already subscribed; picks up
 *   channels created while offline).
 * - DMs: reload `GET /api/dm`, which carries per-conversation unread state;
 *   `loadDMs` subscribes every DM (the server's `DmCreated` auto-subscribe
 *   only covers sockets that were open at the time).
 *
 * Failures are logged and swallowed — reconnect recovery must not abort
 * because one refresh call failed.
 */
export async function refreshUnreadStateAfterReconnect(): Promise<void> {
  const tasks: Promise<unknown>[] = [];

  const guildId = guildsState.activeGuildId;
  if (guildId && guildId !== DISCOVERY_SENTINEL) {
    tasks.push(
      refreshChannelsForGuild(guildId)
        .then(() => subscribeAll(activeGuildTextChannelIds()))
        .catch((err) =>
          console.error("[WS] Failed to refresh guild channels", guildId, err),
        ),
    );
  }

  tasks.push(
    loadDMs().catch((err) =>
      console.error("[WS] Failed to refresh DM list", err),
    ),
  );

  await Promise.all(tasks);
}

/**
 * Channels whose messages are on screen: the selected guild channel and, when
 * a conversation (not the friends list) is shown, the selected DM.
 */
function openChannelIds(): string[] {
  const ids = new Set<string>();
  if (channelsState.selectedChannelId) ids.add(channelsState.selectedChannelId);
  if (dmsState.selectedDMId && !dmsState.isShowingFriends) {
    ids.add(dmsState.selectedDMId);
  }
  return [...ids];
}

/** Whether the time between the last two `ready`s is too long to page through. */
function gapTooLongForCatchUp(): boolean {
  if (!resyncWatermark || !previousWatermark) return false;
  const gap = Date.parse(resyncWatermark) - Date.parse(previousWatermark);
  return gap > MAX_CATCH_UP_GAP_MS;
}

/**
 * Bring one open channel up to date after a reconnect (C2 order: subscribed
 * ack → `GET ?after=` → dedupe). Never rejects.
 */
async function resyncOpenChannel(
  channelId: string,
  anchor: string | undefined,
  acked: Promise<boolean>,
): Promise<void> {
  if (!(await acked)) {
    console.warn("[WS] No subscribe ack before catch-up for channel", channelId);
  }
  try {
    if (gapTooLongForCatchUp()) {
      await loadInitialMessages(channelId);
    } else {
      await catchUpChannel(channelId, undefined, anchor);
    }
  } catch (err) {
    console.error("[WS] Catch-up failed, reloading channel", channelId, err);
    await loadInitialMessages(channelId).catch((e) =>
      console.error("[WS] Failed to reload messages for channel", channelId, e),
    );
  }
}

/**
 * Handle post-reconnect recovery.
 *
 * Snapshots the subscribed channels (plus the active guild's text channels,
 * which a guild opened while offline never subscribed, and the open channels)
 * and the catch-up anchors before resetting state. Open channels: subscribe →
 * wait for `ready` and the `subscribed` ack → catch up (C2). Other cached
 * channels are dropped and reload when opened, which also picks up edits and
 * deletes from the gap. Unread metadata is refreshed in parallel.
 */
async function handleReconnect(): Promise<void> {
  const open = openChannelIds();
  const cached = Object.keys(messagesState.byChannel).filter(
    (id) => (messagesState.byChannel[id]?.length ?? 0) > 0,
  );
  // Every open channel, cached or not: with an empty cache (new or quiet
  // channel, or a load in flight) catchUpChannel falls back to a fresh load,
  // so messages from the gap still appear.
  const toCatchUp = open;
  const toClear = cached.filter((id) => !open.includes(id));
  const anchors = new Map(toCatchUp.map((id) => [id, catchUpAnchor(id)]));
  const channelsToResubscribe = [
    ...new Set([
      ...wsState.subscribedChannels,
      ...activeGuildTextChannelIds(),
      ...toCatchUp,
    ]),
  ];

  // Clear subscribed set so subscribeChannel() early-return guard allows re-subscription
  setWsState("subscribedChannels", new Set());
  setWsState({ status: "connected", reconnectAttempt: 0, error: null });

  for (const id of toClear) clearChannelMessages(id);

  // Register the ack waiters before the subscribe frames go out.
  const acks = new Map(
    toCatchUp.map((id) => [id, waitForSubscribeAck(id, RESYNC_WAIT_MS)]),
  );
  await subscribeAll(channelsToResubscribe);

  // The metadata refresh is what clears stale unread dots: messages that
  // arrived (or were read on another device) while disconnected changed
  // last_read_message_id / last_message_id server-side, and no WS event
  // replays them after reconnect.
  await Promise.all([
    (async () => {
      if (toCatchUp.length === 0) return;
      if (!(await waitForReady(RESYNC_WAIT_MS))) {
        console.warn("[WS] No ready event after reconnect; catching up anyway");
      }
      await Promise.all(
        toCatchUp.map((id) =>
          resyncOpenChannel(id, anchors.get(id), acks.get(id)!),
        ),
      );
    })(),
    refreshUnreadStateAfterReconnect(),
  ]);

  console.info(
    `[WS] Reconnect recovery complete: ${channelsToResubscribe.length} channels re-subscribed, ${toCatchUp.length} caught up, ${toClear.length} cleared`,
  );

  // Signal connection ready after subscriptions are restored
  window.dispatchEvent(new Event("ws-connected"));
}

/** Browser transport for the reconnect supervisor. */
const browserDriver: ConnectionDriver = {
  async open() {
    connectStartTime = Date.now();
    await tauri.wsConnect();
    attachBrowserMessageHandler();
  },
  close() {
    tauri.wsDisconnect().catch((err) =>
      console.error("[WS] Failed to close socket:", err),
    );
  },
  sendPing() {
    tauri.wsPing().catch((err) => console.warn("[WS] Ping failed:", err));
  },
};

/**
 * Native transport: Rust retries, pings and watches for silence itself; the
 * supervisor only mirrors its status events and forwards "try now" triggers.
 */
const nativeDriver: ConnectionDriver = {
  async open() {
    connectStartTime = Date.now();
    // Resolves once Rust is connected; rejects if the first attempt failed
    // (Rust keeps retrying and reports ws:connected later).
    await tauri.wsConnect();
    window.dispatchEvent(new Event("ws-connected"));
  },
  close() {
    tauri.wsDisconnect().catch((err) =>
      console.error("[WS] Failed to close socket:", err),
    );
  },
  sendPing() {
    // Rust sends the pings.
  },
  reconnectNow: () => tauri.wsReconnectNow(),
};

/**
 * Native: the server refused the socket even with a freshly refreshed token,
 * so retrying cannot help. Stop the supervisor and the Rust retry loop (the
 * native driver's close → `ws_disconnect`) instead of showing "Reconnecting…"
 * forever, and send the user down the "session expired → sign in again" path.
 *
 * Known gap: once server C4 refuses pending-password users at the upgrade
 * (HTTP 403), those users land here too. The forced password-change screen
 * (plan Task 13) must ship together with server C4 and route them to the
 * change screen instead of this generic sign-in prompt.
 */
function handleNativeAuthRejected(): void {
  console.warn("[WS] Server rejected a fresh token — ending the session");
  supervisor?.stop();
  nativeDriver.close();
  // Clears the cached bearer and shares the dedupe flag with the other
  // expiry paths, so exactly one `kaiku:session-expired` fires.
  tauri.expireNativeSession();
}

function getSupervisor(): ConnectionSupervisor {
  supervisor ??= createConnectionSupervisor({
    driver: isTauri() ? nativeDriver : browserDriver,
    mode: isTauri() ? "external" : "managed",
    onStatus: (status, attempt) => {
      // A new connection must deliver its own `ready` before catch-up.
      if (status !== "connected") readyReceived = false;
      setWsState({
        status,
        reconnectAttempt: attempt,
        ...(status === "connected" ? { error: null } : {}),
      });
    },
    onRecovered: handleReconnect,
    ensureFreshToken: () => tauri.ensureFreshAccessToken(),
  });
  return supervisor;
}

/** Retry the connection immediately (ConnectionIndicator "Retry now"). */
export function retryConnectionNow(): void {
  supervisor?.triggerNow("user");
}

/** Whether "Retry now" can do anything in this runtime. */
export function canRetryConnectionNow(): boolean {
  return true;
}

/**
 * Connect to the WebSocket server.
 */
export async function connect(): Promise<void> {
  // The supervisor opens the connection; in the browser it also keeps it
  // alive, natively it mirrors the Rust loop.
  try {
    setWsState({ error: null });
    if (!isTauri()) {
      tauri.setBrowserSocketHooks({
        onClose: (intentional) => supervisor?.notifyClosed(intentional),
        onFrame: () => supervisor?.notifyFrame(),
      });
    }
    const opened = await getSupervisor().start();
    // Only after this call opened a fresh socket. When the supervisor was
    // already running (connected, or retrying) it owns recovery itself;
    // running it here would fake "connected" on a socket that isn't open.
    if (opened && wsState.subscribedChannels.size > 0) {
      await handleReconnect();
    }
  } catch (err) {
    // Status stays as the supervisor set it ("reconnecting": it retries).
    // Natively the first attempt's failure can arrive after a retry already
    // connected (ws:connected); don't show a stale error over a live socket.
    const error = err instanceof Error ? err.message : String(err);
    if (wsState.status !== "connected") setWsState({ error });
    throw err;
  }
}

/**
 * Wait for WebSocket to reach "connected" status.
 * Returns true if connected within timeout, false otherwise.
 * Uses event-driven notification instead of polling.
 */
export function waitForConnection(timeoutMs = 5000): Promise<boolean> {
  if (wsState.status === "connected") return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      window.removeEventListener("ws-connected", handler);
      resolve(false);
    }, timeoutMs);
    const handler = () => {
      clearTimeout(timer);
      resolve(true);
    };
    window.addEventListener("ws-connected", handler, { once: true });
  });
}

/**
 * Disconnect from the WebSocket server.
 */
export async function disconnect(): Promise<void> {
  try {
    supervisor?.stop();
    await tauri.wsDisconnect();
    detachBrowserMessageHandler?.();
    detachBrowserMessageHandler = null;
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
  // Don't show own typing indicator
  if (userId === currentUser()?.id) return;

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
export { wsState, setWsState, typingState, setTypingState };
