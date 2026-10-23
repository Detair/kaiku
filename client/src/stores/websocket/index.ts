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

    case "subscribed":
    case "unsubscribed":
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
      console.log("Unhandled server event:", event.type);
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
 * Handle post-reconnect recovery: re-subscribe channels, reload messages.
 * Snapshots current subscribed channels before resetting state, then re-subscribes and reloads.
 */
/**
 * Refresh channel/DM unread metadata after a reconnect. Exported for tests.
 *
 * - Active guild: re-fetch its channel list (selection-preserving) so the
 *   sidebar unread dots reflect what happened while offline.
 * - DMs: reload the DM list, which carries per-conversation unread state.
 *
 * Failures are logged and swallowed — reconnect recovery must not abort
 * because one refresh call failed.
 */
export async function refreshUnreadStateAfterReconnect(): Promise<void> {
  const tasks: Promise<unknown>[] = [];

  const guildId = guildsState.activeGuildId;
  if (guildId && guildId !== DISCOVERY_SENTINEL) {
    tasks.push(
      refreshChannelsForGuild(guildId).catch((err) =>
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

async function handleReconnect(): Promise<void> {
  const channelsToResubscribe = [...wsState.subscribedChannels];
  const channelsToReload = Object.keys(messagesState.byChannel).filter(
    (id) => (messagesState.byChannel[id]?.length ?? 0) > 0,
  );

  // Clear subscribed set so subscribeChannel() early-return guard allows re-subscription
  setWsState("subscribedChannels", new Set());
  setWsState({ status: "connected", reconnectAttempt: 0, error: null });

  // Re-subscribe, reload messages, and refresh unread metadata in parallel.
  // The metadata refresh is what clears stale unread dots: messages that
  // arrived (or were read on another device) while disconnected changed
  // last_read_message_id / last_message_id server-side, and no WS event
  // replays them after reconnect.
  await Promise.all([
    Promise.all(
      channelsToResubscribe.map((id) =>
        subscribeChannel(id).catch((err) =>
          console.error("[WS] Failed to re-subscribe channel", id, err),
        ),
      ),
    ),
    Promise.all(
      channelsToReload.map((id) =>
        loadInitialMessages(id).catch((err) =>
          console.error("[WS] Failed to reload messages for channel", id, err),
        ),
      ),
    ),
    refreshUnreadStateAfterReconnect(),
  ]);

  console.info(
    `[WS] Reconnect recovery complete: ${channelsToResubscribe.length} channels re-subscribed, ${channelsToReload.length} channels reloaded`,
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

function getSupervisor(): ConnectionSupervisor {
  supervisor ??= createConnectionSupervisor({
    driver: isTauri() ? nativeDriver : browserDriver,
    mode: isTauri() ? "external" : "managed",
    onStatus: (status, attempt) => {
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
    const error = err instanceof Error ? err.message : String(err);
    setWsState({ error });
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
