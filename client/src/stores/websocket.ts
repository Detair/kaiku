/**
 * WebSocket Store
 *
 * Manages WebSocket connection and routes events to appropriate stores.
 */

import { createStore } from "solid-js/store";
import * as tauri from "@/lib/tauri";
import type {
  Activity,
  Message,
  ServerEvent,
  ThreadInfo,
  UserStatus,
} from "@/lib/types";
import { updateUserActivity, updateUserPresence } from "./presence";
import {
  addMessage,
  removeMessage,
  messagesState,
  setMessagesState,
} from "./messages";
import {
  addThreadReply,
  removeThreadReply,
  setThreadReadState,
  updateThreadInfo,
  updateParentThreadIndicator,
  markThreadUnread,
  clearThreadUnread,
  threadsState,
} from "./threads";
import { handlePreferencesUpdated } from "./preferences";
import {
  receiveIncomingCall,
  callConnected,
  callEndedExternally,
  participantJoined,
  participantLeft,
  type EndReason,
} from "./call";
import {
  loadFriends,
  loadPendingRequests,
  handleUserBlocked,
  handleUserUnblocked,
} from "./friends";
import { playNotification } from "@/lib/sound";
import {
  getChannel,
  channelsState,
  handleChannelReadEvent,
  incrementUnreadCount,
} from "./channels";
import { currentUser } from "./auth";
import {
  guildsState,
  getGuildIdForChannel,
  incrementGuildUnread,
} from "./guilds";
import type { MentionType, SoundEventType } from "@/lib/sound/types";
import { handleDMReadEvent, handleDMNameUpdated } from "./dms";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

// Connection state
type ConnectionState =
  | "disconnected"
  | "connecting"
  | "connected"
  | "reconnecting";

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
 * Handle notification sound for incoming message.
 */
function handleMessageNotification(message: Message): void {
  const user = currentUser();

  // Don't notify for own messages
  if (user && message.author.id === user.id) {
    return;
  }

  // Don't notify for currently focused channel when the window is visible
  if (
    channelsState.selectedChannelId === message.channel_id &&
    !document.hidden
  ) {
    return;
  }

  // Determine if this is a DM
  const channel = getChannel(message.channel_id);
  const isDm = channel?.channel_type === "dm" || channel?.guild_id === null;

  // Determine event type based on channel and mention
  let eventType: SoundEventType;
  if (isDm) {
    eventType = "message_dm";
  } else if (
    message.mention_type === "direct" ||
    message.mention_type === "everyone" ||
    message.mention_type === "here"
  ) {
    eventType = "message_mention";
  } else {
    eventType = "message_channel";
  }

  // Play notification
  playNotification({
    type: eventType,
    channelId: message.channel_id,
    isDm,
    mentionType: message.mention_type as MentionType,
    authorId: message.author.id,
    content: message.content,
  });
}

/**
 * Initialize WebSocket event listeners.
 * Call this once when the app starts (after auth).
 */
export async function initWebSocket(): Promise<void> {
  // Clean up existing listeners
  await cleanupWebSocket();

  if (isTauri) {
    // Tauri mode - use Tauri event system
    const { listen } = await import("@tauri-apps/api/event");

    // Connection status events
    unlisteners.push(
      await listen("ws:connecting", () => {
        setWsState({ status: "connecting", error: null });
      }),
    );

    unlisteners.push(
      await listen("ws:connected", () => {
        setWsState({ status: "connected", reconnectAttempt: 0, error: null });
      }),
    );

    unlisteners.push(
      await listen("ws:disconnected", () => {
        setWsState({ status: "disconnected" });
      }),
    );

    unlisteners.push(
      await listen<number>("ws:reconnecting", (event) => {
        setWsState({ status: "reconnecting", reconnectAttempt: event.payload });
      }),
    );

    // Message events
    unlisteners.push(
      await listen<{ channel_id: string; message: Message }>(
        "ws:message_new",
        async (event) => {
          await addMessage(event.payload.message);
          handleMessageNotification(event.payload.message);
          // Increment unread count if message is not in the selected channel
          if (event.payload.channel_id !== channelsState.selectedChannelId) {
            const channel = getChannel(event.payload.channel_id);
            if (
              channel &&
              channel.guild_id &&
              channel.channel_type === "text"
            ) {
              incrementUnreadCount(event.payload.channel_id);
            }
            const guildId = getGuildIdForChannel(event.payload.channel_id);
            if (guildId && guildId !== guildsState.activeGuildId) {
              incrementGuildUnread(guildId);
            }
            // Notify unread module of new message
            window.dispatchEvent(new CustomEvent("unread-update"));
          }
        },
      ),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        message_id: string;
        content: string;
        edited_at: string;
      }>("ws:message_edit", (event) => {
        const { channel_id, message_id, content, edited_at } = event.payload;
        const messages = messagesState.byChannel[channel_id];
        if (messages) {
          const index = messages.findIndex((m) => m.id === message_id);
          if (index !== -1) {
            setMessagesState(
              "byChannel",
              channel_id,
              index,
              "content",
              content,
            );
            setMessagesState(
              "byChannel",
              channel_id,
              index,
              "edited_at",
              edited_at,
            );
          }
        }
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; message_id: string }>(
        "ws:message_delete",
        (event) => {
          removeMessage(event.payload.channel_id, event.payload.message_id);
        },
      ),
    );

    // Typing events
    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:typing_start",
        (event) => {
          const { channel_id, user_id } = event.payload;
          addTypingUser(channel_id, user_id);
        },
      ),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:typing_stop",
        (event) => {
          const { channel_id, user_id } = event.payload;
          removeTypingUser(channel_id, user_id);
        },
      ),
    );

    // Presence events
    unlisteners.push(
      await listen<{ user_id: string; status: UserStatus }>(
        "ws:presence_update",
        (event) => {
          updateUserPresence(event.payload.user_id, event.payload.status);
        },
      ),
    );

    // Rich presence events
    unlisteners.push(
      await listen<{ user_id: string; activity: Activity | null }>(
        "ws:rich_presence_update",
        (event) => {
          console.log(
            "Rich presence update:",
            event.payload.user_id,
            event.payload.activity,
          );
          updateUserActivity(event.payload.user_id, event.payload.activity);
        },
      ),
    );

    // Error events
    unlisteners.push(
      await listen<{ code: string; message: string }>("ws:error", (event) => {
        console.error("WebSocket error:", event.payload);
        setWsState({ error: event.payload.message });
      }),
    );

    // Call events
    unlisteners.push(
      await listen<{
        channel_id: string;
        initiator: string;
        initiator_name: string;
      }>("ws:incoming_call", (event) => {
        receiveIncomingCall(
          event.payload.channel_id,
          event.payload.initiator,
          event.payload.initiator_name,
        );
      }),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        reason: string;
        duration_secs: number | null;
      }>("ws:call_ended", (event) => {
        callEndedExternally(
          event.payload.channel_id,
          event.payload.reason as EndReason,
          event.payload.duration_secs ?? undefined,
        );
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string; username: string }>(
        "ws:call_participant_joined",
        (event) => {
          participantJoined(event.payload.channel_id, event.payload.user_id);
          callConnected(event.payload.channel_id, [event.payload.user_id]);
        },
      ),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:call_participant_left",
        (event) => {
          participantLeft(event.payload.channel_id, event.payload.user_id);
        },
      ),
    );

    // Voice events (Tauri → frontend parity with browser mode)
    unlisteners.push(
      await listen<{ channel_id: string; sdp: string }>(
        "ws:voice_offer",
        async (event) => {
          await handleVoiceOffer(event.payload.channel_id, event.payload.sdp);
        },
      ),
    );

    unlisteners.push(
      await listen<{ channel_id: string; candidate: string }>(
        "ws:voice_ice_candidate",
        async (event) => {
          await handleVoiceIceCandidate(
            event.payload.channel_id,
            event.payload.candidate,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        user_id: string;
        username: string;
        display_name: string;
      }>("ws:voice_user_joined", async (event) => {
        await handleVoiceUserJoined(
          event.payload.channel_id,
          event.payload.user_id,
          event.payload.username,
          event.payload.display_name,
        );
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:voice_user_left",
        async (event) => {
          await handleVoiceUserLeft(
            event.payload.channel_id,
            event.payload.user_id,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:voice_user_muted",
        async (event) => {
          await handleVoiceUserMuted(
            event.payload.channel_id,
            event.payload.user_id,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:voice_user_unmuted",
        async (event) => {
          await handleVoiceUserUnmuted(
            event.payload.channel_id,
            event.payload.user_id,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        participants: any[];
        screen_shares: any[];
      }>("ws:voice_room_state", async (event) => {
        await handleVoiceRoomState(
          event.payload.channel_id,
          event.payload.participants,
          event.payload.screen_shares,
        );
      }),
    );

    unlisteners.push(
      await listen<{ code: string; message: string }>(
        "ws:voice_error",
        (event) => {
          console.error(
            "Voice error:",
            event.payload.code,
            event.payload.message,
          );
        },
      ),
    );

    // Reaction events (Tauri → frontend parity with browser mode)
    unlisteners.push(
      await listen<{
        channel_id: string;
        message_id: string;
        user_id: string;
        emoji: string;
      }>("ws:reaction_add", (event) => {
        handleReactionAdd(
          event.payload.channel_id,
          event.payload.message_id,
          event.payload.user_id,
          event.payload.emoji,
        );
      }),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        message_id: string;
        user_id: string;
        emoji: string;
      }>("ws:reaction_remove", (event) => {
        handleReactionRemove(
          event.payload.channel_id,
          event.payload.message_id,
          event.payload.user_id,
          event.payload.emoji,
        );
      }),
    );

    // Guild emoji events
    unlisteners.push(
      await listen<{ guild_id: string; emojis: any[] }>(
        "ws:guild_emoji_updated",
        (event) => {
          handleGuildEmojiUpdated(event.payload.guild_id, event.payload.emojis);
        },
      ),
    );

    // Read sync events (Tauri → frontend parity with browser mode)
    unlisteners.push(
      await listen<{ channel_id: string }>("ws:channel_read", (event) => {
        handleChannelReadEvent(event.payload.channel_id);
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string }>("ws:dm_read", (event) => {
        handleDMReadEvent(event.payload.channel_id);
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; name: string }>(
        "ws:dm_name_updated",
        (event) => {
          handleDMNameUpdated(event.payload.channel_id, event.payload.name);
        },
      ),
    );

    // Call events (Tauri → complete call support)
    // Note: These were partially implemented in earlier commits
    // This completes the full call event coverage
    unlisteners.push(
      await listen<{ channel_id: string }>("ws:call_started", (event) => {
        console.log(
          "[WebSocket] Call started in channel:",
          event.payload.channel_id,
        );
        // Call started means the initiator is now connected
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:call_declined",
        (event) => {
          console.log("[WebSocket] Call declined by:", event.payload.user_id);
          // The call store will handle this through the API response
        },
      ),
    );

    // Screen share events (Tauri → frontend parity with browser mode)
    unlisteners.push(
      await listen<{
        channel_id: string;
        user_id: string;
        username: string;
        source_label: string;
        has_audio: boolean;
        quality: string;
        started_at?: string;
      }>("ws:screen_share_started", async (event) => {
        await handleScreenShareStarted(event.payload);
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string; reason: string }>(
        "ws:screen_share_stopped",
        async (event) => {
          await handleScreenShareStopped(event.payload);
        },
      ),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        user_id: string;
        new_quality: string;
        reason: string;
      }>("ws:screen_share_quality_changed", async (event) => {
        await handleScreenShareQualityChanged(event.payload);
      }),
    );

    // Webcam events (Tauri → frontend parity with browser mode)
    unlisteners.push(
      await listen<{
        channel_id: string;
        user_id: string;
        username: string;
        quality: string;
      }>("ws:webcam_started", async (event) => {
        await handleWebcamStarted(event.payload);
      }),
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string; reason: string }>(
        "ws:webcam_stopped",
        async (event) => {
          await handleWebcamStopped(event.payload);
        },
      ),
    );

    // Voice stats events
    unlisteners.push(
      await listen<{
        channel_id: string;
        user_id: string;
        latency: number;
        packet_loss: number;
        jitter: number;
        quality: number;
      }>("ws:voice_user_stats", async (event) => {
        await handleVoiceUserStatsEvent(event.payload);
      }),
    );

    // Admin events
    unlisteners.push(
      await listen<{ user_id: string; username: string }>(
        "ws:admin_user_banned",
        async (event) => {
          await handleAdminUserBanned(
            event.payload.user_id,
            event.payload.username,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{ user_id: string; username: string }>(
        "ws:admin_user_unbanned",
        async (event) => {
          await handleAdminUserUnbanned(
            event.payload.user_id,
            event.payload.username,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{ guild_id: string; guild_name: string }>(
        "ws:admin_guild_suspended",
        async (event) => {
          await handleAdminGuildSuspended(
            event.payload.guild_id,
            event.payload.guild_name,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{ guild_id: string; guild_name: string }>(
        "ws:admin_guild_unsuspended",
        async (event) => {
          await handleAdminGuildUnsuspended(
            event.payload.guild_id,
            event.payload.guild_name,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{
        report_id: string;
        category: string;
        target_type: string;
      }>("ws:admin_report_created", async (event) => {
        await handleAdminReportCreated(
          event.payload.report_id,
          event.payload.category,
          event.payload.target_type,
        );
      }),
    );

    unlisteners.push(
      await listen<{ report_id: string }>(
        "ws:admin_report_resolved",
        async (event) => {
          await handleAdminReportResolved(event.payload.report_id);
        },
      ),
    );

    unlisteners.push(
      await listen<{ user_id: string; username: string }>(
        "ws:admin_user_deleted",
        async (event) => {
          await handleAdminUserDeleted(
            event.payload.user_id,
            event.payload.username,
          );
        },
      ),
    );

    unlisteners.push(
      await listen<{ guild_id: string; guild_name: string }>(
        "ws:admin_guild_deleted",
        async (event) => {
          await handleAdminGuildDeleted(
            event.payload.guild_id,
            event.payload.guild_name,
          );
        },
      ),
    );

    // Friend events
    unlisteners.push(
      await listen("ws:friend_request_received", () => {
        loadPendingRequests();
      }),
    );

    unlisteners.push(
      await listen("ws:friend_request_accepted", () => {
        Promise.all([loadFriends(), loadPendingRequests()]);
      }),
    );

    // Block events
    unlisteners.push(
      await listen<{ user_id: string }>("ws:user_blocked", (event) => {
        handleUserBlocked(event.payload.user_id);
      }),
    );

    unlisteners.push(
      await listen<{ user_id: string }>("ws:user_unblocked", (event) => {
        handleUserUnblocked(event.payload.user_id);
      }),
    );

    // Thread events
    unlisteners.push(
      await listen<{
        channel_id: string;
        parent_id: string;
        message: Message;
        thread_info: ThreadInfo;
      }>("ws:thread_reply_new", (event) => {
        handleThreadReplyNew(
          event.payload.channel_id,
          event.payload.parent_id,
          event.payload.message,
          event.payload.thread_info,
        );
      }),
    );

    unlisteners.push(
      await listen<{
        channel_id: string;
        parent_id: string;
        message_id: string;
        thread_info: ThreadInfo;
      }>("ws:thread_reply_delete", (event) => {
        handleThreadReplyDelete(
          event.payload.channel_id,
          event.payload.parent_id,
          event.payload.message_id,
          event.payload.thread_info,
        );
      }),
    );

    unlisteners.push(
      await listen<{
        thread_parent_id: string;
        last_read_message_id: string | null;
      }>("ws:thread_read", (event) => {
        handleThreadRead(
          event.payload.thread_parent_id,
          event.payload.last_read_message_id,
        );
      }),
    );

    // Preferences sync
    unlisteners.push(
      await listen<any>("ws:preferences_updated", (event) => {
        handlePreferencesUpdated(event.payload);
      }),
    );

    // State sync (patch)
    unlisteners.push(
      await listen<{
        entity_type: string;
        entity_id: string;
        diff: Record<string, unknown>;
      }>("ws:patch", async (event) => {
        await handlePatchEvent(
          event.payload.entity_type,
          event.payload.entity_id,
          event.payload.diff,
        );
      }),
    );

    // Bot command response events
    unlisteners.push(
      await listen<{
        interaction_id: string;
        content: string;
        command_name: string;
        bot_name: string;
        channel_id: string;
        ephemeral: boolean;
      }>("ws:command_response", async (event) => {
        if (event.payload.ephemeral) {
          console.log(
            "[WebSocket] Ephemeral command response:",
            event.payload.command_name,
            event.payload.content,
          );
          const syntheticMessage: Message = {
            id: crypto.randomUUID(),
            channel_id: event.payload.channel_id,
            author: {
              id: "system",
              username: event.payload.bot_name,
              display_name: event.payload.bot_name,
              avatar_url: null,
              status: "online",
            },
            content: event.payload.content,
            encrypted: false,
            attachments: [],
            reply_to: null,
            parent_id: null,
            thread_reply_count: 0,
            thread_last_reply_at: null,
            edited_at: null,
            created_at: new Date().toISOString(),
            mention_type: null,
          };
          await addMessage(syntheticMessage);
        } else {
          console.log(
            "[WebSocket] Non-ephemeral command response (handled via message_new):",
            event.payload.command_name,
          );
        }
      }),
    );

    unlisteners.push(
      await listen<{
        interaction_id: string;
        command_name: string;
        channel_id: string;
      }>("ws:command_response_timeout", async (event) => {
        console.warn(
          "[WebSocket] Command response timeout:",
          event.payload.command_name,
        );
        const { showToast } = await import("@/components/ui/Toast");
        showToast({
          type: "warning",
          title: "Command Timeout",
          message: `Command /${event.payload.command_name} did not respond within 30 seconds.`,
          duration: 5000,
          id: `cmd-timeout-${event.payload.command_name}`,
        });
      }),
    );
  } else {
    // Browser mode - use browser WebSocket events
    const attachMessageHandler = () => {
      const ws = tauri.getBrowserWebSocket();
      if (ws) {
        console.log("[WebSocket] Attaching message handler to WebSocket");
        const messageHandler = async (event: MessageEvent) => {
          try {
            const data = JSON.parse(event.data) as ServerEvent;
            await handleServerEvent(data);
          } catch (err) {
            console.error("Failed to parse WebSocket message:", err);
          }
        };
        ws.addEventListener("message", messageHandler);
        unlisteners.push(() =>
          ws.removeEventListener("message", messageHandler),
        );
      } else {
        console.warn("[WebSocket] No WebSocket instance found");
      }
    };

    // Attach initially
    attachMessageHandler();

    // Re-attach on reconnection
    const reconnectHandler = () => {
      console.log(
        "[WebSocket] Received ws-reconnected event, re-attaching message handler",
      );
      // Use setTimeout to ensure WebSocket is fully ready
      setTimeout(() => {
        attachMessageHandler();
      }, 100);
    };
    window.addEventListener("ws-reconnected", reconnectHandler);
    unlisteners.push(() =>
      window.removeEventListener("ws-reconnected", reconnectHandler),
    );
  }
}

/**
 * Handle server events in browser mode.
 * IMPORTANT: Voice events are handled asynchronously to ensure proper ICE candidate processing.
 */
async function handleServerEvent(event: ServerEvent): Promise<void> {
  console.log("[WebSocket] Received event:", event.type);

  switch (event.type) {
    case "message_new":
      await addMessage(event.message);
      handleMessageNotification(event.message);
      // Increment unread count if message is not in the selected channel
      if (event.channel_id !== channelsState.selectedChannelId) {
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
      break;

    case "message_edit": {
      const editMessages = messagesState.byChannel[event.channel_id];
      if (editMessages) {
        const editIndex = editMessages.findIndex(
          (m) => m.id === event.message_id,
        );
        if (editIndex !== -1) {
          setMessagesState(
            "byChannel",
            event.channel_id,
            editIndex,
            "content",
            event.content,
          );
          setMessagesState(
            "byChannel",
            event.channel_id,
            editIndex,
            "edited_at",
            event.edited_at,
          );
        }
      }
      break;
    }

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

    case "voice_offer":
      console.log(
        "[WebSocket] Handling voice_offer for channel:",
        event.channel_id,
      );
      await handleVoiceOffer(event.channel_id, event.sdp);
      break;

    case "voice_ice_candidate":
      // ICE candidates must be processed immediately for NAT traversal
      await handleVoiceIceCandidate(event.channel_id, event.candidate);
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
    case "incoming_call":
      console.log("[WebSocket] Incoming call from:", event.initiator_name);
      receiveIncomingCall(
        event.channel_id,
        event.initiator,
        event.initiator_name,
      );
      break;

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
      // When someone joins, transition to connected state if we're connecting
      callConnected(event.channel_id, [event.user_id]);
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
      handleDMReadEvent(event.channel_id);
      break;

    // DM name updated
    case "dm_name_updated":
      handleDMNameUpdated(event.channel_id, event.name);
      break;

    // Guild channel read sync event
    case "channel_read":
      handleChannelReadEvent(event.channel_id);
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

    // Guild emoji events
    case "guild_emoji_updated":
      handleGuildEmojiUpdated(event.guild_id, event.emojis);
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

    case "command_response_timeout":
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

  // Clear typing timers
  for (const timer of Object.values(typingTimers)) {
    clearTimeout(timer);
  }
}

/**
 * Re-initialize WebSocket listeners after reconnection (browser mode only).
 */
export async function reinitWebSocketListeners(): Promise<void> {
  if (isTauri) return;

  console.log("[WebSocket] Reinitializing WebSocket listeners");

  const ws = tauri.getBrowserWebSocket();
  if (!ws) {
    console.warn(
      "[WebSocket] No WebSocket instance available for reinitialization",
    );
    return;
  }

  // Remove old listener if it exists (prevent duplicates)
  const oldListeners = unlisteners.filter((ul) =>
    ul.toString().includes("message"),
  );
  oldListeners.forEach((ul) => ul());

  // Attach new message handler
  const messageHandler = (event: MessageEvent) => {
    try {
      const data = JSON.parse(event.data) as ServerEvent;
      handleServerEvent(data);
    } catch (err) {
      console.error("Failed to parse WebSocket message:", err);
    }
  };

  ws.addEventListener("message", messageHandler);
  unlisteners.push(() => ws.removeEventListener("message", messageHandler));

  console.log("[WebSocket] Message handler attached to WebSocket instance");
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

// Voice event handlers

async function handleVoiceOffer(channelId: string, sdp: string): Promise<void> {
  try {
    // Use getVoiceAdapter() for faster access (offer should arrive after join)
    const { getVoiceAdapter } = await import("@/lib/webrtc");
    const adapter = getVoiceAdapter();

    if (!adapter) {
      console.error("[WebSocket] No voice adapter available for offer");
      return;
    }

    const result = await adapter.handleOffer(channelId, sdp);

    if (result.ok) {
      // Send answer back to server
      await tauri.wsSend({
        type: "voice_answer",
        channel_id: channelId,
        sdp: result.value,
      });
      console.log("[WebSocket] Voice answer sent successfully");
    } else {
      console.error("Failed to handle voice offer:", result.error);
    }
  } catch (err) {
    console.error("Error handling voice offer:", err);
  }
}

async function handleVoiceIceCandidate(
  channelId: string,
  candidate: string,
): Promise<void> {
  const startTime = performance.now();

  try {
    // Use getVoiceAdapter() to avoid dynamic import overhead (critical for ICE timing)
    const { getVoiceAdapter } = await import("@/lib/webrtc");
    const adapter = getVoiceAdapter();

    if (!adapter) {
      console.warn("[WebSocket] No voice adapter available for ICE candidate");
      return;
    }

    const result = await adapter.handleIceCandidate(channelId, candidate);

    const elapsed = performance.now() - startTime;
    console.log(
      `[WebSocket] ICE candidate processed in ${elapsed.toFixed(2)}ms`,
    );

    if (!result.ok) {
      console.error("Failed to handle ICE candidate:", result.error);
    }
  } catch (err) {
    console.error("Error handling ICE candidate:", err);
  }
}

async function handleVoiceUserJoined(
  channelId: string,
  userId: string,
  username: string,
  displayName: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        state.participants[userId] = {
          user_id: userId,
          username: username,
          display_name: displayName,
          muted: false,
          speaking: false,
          screen_sharing: false,
        };
      }),
    );
  }
}

async function handleVoiceUserLeft(
  channelId: string,
  userId: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        delete state.participants[userId];
      }),
    );
  }
}

async function handleVoiceUserMuted(
  channelId: string,
  userId: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        if (state.participants[userId]) {
          state.participants[userId].muted = true;
        }
      }),
    );
  }
}

async function handleVoiceUserUnmuted(
  channelId: string,
  userId: string,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        if (state.participants[userId]) {
          state.participants[userId].muted = false;
        }
      }),
    );
  }
}

async function handleVoiceRoomState(
  channelId: string,
  participants: any[],
  screenShares?: any[],
  webcams?: any[],
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        state.participants = {};
        for (const p of participants) {
          state.participants[p.user_id] = p;
        }
        state.screenShares = screenShares ?? [];
        state.webcams = webcams ?? [];
      }),
    );
  }
}

// Screen share event handlers

export async function handleScreenShareStarted(event: any): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Screen share started:", event.user_id);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Add to screen shares list
        state.screenShares.push({
          user_id: event.user_id,
          username: event.username,
          source_label: event.source_label,
          has_audio: event.has_audio,
          quality: event.quality,
          started_at: event.started_at ?? new Date().toISOString(),
        });

        // Update participant's screen_sharing flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].screen_sharing = true;
        }
      }),
    );
  }
}

export async function handleScreenShareStopped(event: any): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Screen share stopped:", event.user_id, event.reason);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Remove from screen shares list
        state.screenShares = state.screenShares.filter(
          (s) => s.user_id !== event.user_id,
        );

        // Update participant's screen_sharing flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].screen_sharing = false;
        }

        // If it was us, clear local state
        if (state.screenShareInfo?.user_id === event.user_id) {
          state.screenSharing = false;
          state.screenShareInfo = null;
        }
      }),
    );
  }
}

export async function handleScreenShareQualityChanged(
  event: any,
): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log(
    "[WebSocket] Screen share quality changed:",
    event.user_id,
    event.new_quality,
  );

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        const share = state.screenShares.find(
          (s) => s.user_id === event.user_id,
        );
        if (share) {
          share.quality = event.new_quality;
        }
      }),
    );
  }
}

// Webcam event handlers

export async function handleWebcamStarted(event: any): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Webcam started:", event.user_id);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Add to webcams list
        state.webcams.push({
          user_id: event.user_id,
          username: event.username,
          quality: event.quality,
        });

        // Update participant's webcam_active flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].webcam_active = true;
        }
      }),
    );
  }
}

export async function handleWebcamStopped(event: any): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Webcam stopped:", event.user_id, event.reason);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Remove from webcams list
        state.webcams = state.webcams.filter(
          (w) => w.user_id !== event.user_id,
        );

        // Update participant's webcam_active flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].webcam_active = false;
        }

        // If it was us, clear local state
        // (authState comparison not available here, so the voice store handles it via WS event)
      }),
    );
  }
}

async function handleVoiceUserStatsEvent(event: {
  channel_id: string;
  user_id: string;
  latency: number;
  packet_loss: number;
  jitter: number;
  quality: number;
}): Promise<void> {
  const { handleVoiceUserStats } = await import("@/stores/voice");
  handleVoiceUserStats(event);
}

// Admin event handlers

async function handleAdminUserBanned(
  userId: string,
  username: string,
): Promise<void> {
  const { handleUserBannedEvent } = await import("@/stores/admin");
  handleUserBannedEvent(userId, username);
}

async function handleAdminUserUnbanned(
  userId: string,
  username: string,
): Promise<void> {
  const { handleUserUnbannedEvent } = await import("@/stores/admin");
  handleUserUnbannedEvent(userId, username);
}

async function handleAdminGuildSuspended(
  guildId: string,
  guildName: string,
): Promise<void> {
  const { handleGuildSuspendedEvent } = await import("@/stores/admin");
  handleGuildSuspendedEvent(guildId, guildName);
}

async function handleAdminGuildUnsuspended(
  guildId: string,
  guildName: string,
): Promise<void> {
  const { handleGuildUnsuspendedEvent } = await import("@/stores/admin");
  handleGuildUnsuspendedEvent(guildId, guildName);
}

async function handleAdminUserDeleted(
  userId: string,
  username: string,
): Promise<void> {
  const { handleUserDeletedEvent } = await import("@/stores/admin");
  handleUserDeletedEvent(userId, username);
}

async function handleAdminGuildDeleted(
  guildId: string,
  guildName: string,
): Promise<void> {
  const { handleGuildDeletedEvent } = await import("@/stores/admin");
  handleGuildDeletedEvent(guildId, guildName);
}

// Guild emoji event handler

async function handleGuildEmojiUpdated(
  guildId: string,
  emojis: any[],
): Promise<void> {
  const { setGuildEmojis } = await import("@/stores/emoji");
  setGuildEmojis(guildId, emojis);
}

// Thread event handlers

function handleThreadReplyNew(
  channelId: string,
  parentId: string,
  message: Message,
  threadInfo: ThreadInfo,
): void {
  // Add reply to thread store
  addThreadReply(parentId, message);

  // Update thread info cache
  updateThreadInfo(parentId, threadInfo);

  // Mark thread as unread if not currently open and reply is from another user
  const user = currentUser();
  if (
    threadsState.activeThreadId !== parentId &&
    (!user || message.author.id !== user.id)
  ) {
    markThreadUnread(parentId);
  }

  // Update parent message's thread indicator in main messages store
  updateParentThreadIndicator(channelId, parentId, threadInfo);

  // Play notification for thread reply
  handleThreadNotification(message);
}

function handleThreadReplyDelete(
  channelId: string,
  parentId: string,
  messageId: string,
  threadInfo: ThreadInfo,
): void {
  // Remove reply from thread store
  removeThreadReply(parentId, messageId);

  // Update thread info cache (updateThreadInfo preserves existing has_unread)
  updateThreadInfo(parentId, threadInfo);

  // Update parent message's thread indicator in main messages store
  updateParentThreadIndicator(channelId, parentId, threadInfo);

  // If the deleted message was the one that made the thread "read",
  // and there are still newer unread replies, the unread state is preserved
  // by updateThreadInfo's has_unread preservation logic.
}

function handleThreadRead(
  parentId: string,
  lastReadMessageId: string | null,
): void {
  setThreadReadState(parentId, lastReadMessageId);
  clearThreadUnread(parentId);
}

function handleThreadNotification(message: Message): void {
  const user = currentUser();
  if (user && message.author.id === user.id) return;

  const channel = getChannel(message.channel_id);
  const isDm = channel?.channel_type === "dm" || channel?.guild_id === null;

  playNotification({
    type: "message_thread",
    channelId: message.channel_id,
    isDm,
    mentionType: message.mention_type as MentionType,
    authorId: message.author.id,
    content: message.content,
  });
}

// Reaction event handlers

function handleReactionAdd(
  channelId: string,
  messageId: string,
  userId: string,
  emoji: string,
): void {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;

  const messageIndex = messages.findIndex((m) => m.id === messageId);
  if (messageIndex === -1) return;

  const message = messages[messageIndex];
  const reactions = message.reactions ? [...message.reactions] : [];

  // Find existing reaction for this emoji
  const reactionIndex = reactions.findIndex((r) => r.emoji === emoji);

  if (reactionIndex !== -1) {
    // Update existing reaction
    const reaction = { ...reactions[reactionIndex] };
    const users = reaction.users ?? [];
    if (!users.includes(userId)) {
      reaction.users = [...users, userId];
      reaction.count = reaction.users.length;
      // Check if it's the current user
      const user = currentUser();
      if (user && userId === user.id) {
        reaction.me = true;
      }
      reactions[reactionIndex] = reaction;
    }
  } else {
    // Add new reaction
    const user = currentUser();
    reactions.push({
      emoji,
      count: 1,
      users: [userId],
      me: user ? userId === user.id : false,
    });
  }

  // Update the message in the store
  setMessagesState(
    "byChannel",
    channelId,
    messageIndex,
    "reactions",
    reactions,
  );
}

function handleReactionRemove(
  channelId: string,
  messageId: string,
  userId: string,
  emoji: string,
): void {
  const messages = messagesState.byChannel[channelId];
  if (!messages) return;

  const messageIndex = messages.findIndex((m) => m.id === messageId);
  if (messageIndex === -1) return;

  const message = messages[messageIndex];
  if (!message.reactions) return;

  const reactions = [...message.reactions];
  const reactionIndex = reactions.findIndex((r) => r.emoji === emoji);

  if (reactionIndex === -1) return;

  const reaction = { ...reactions[reactionIndex] };
  const users = reaction.users ?? [];
  const userIndex = users.indexOf(userId);

  if (userIndex !== -1) {
    reaction.users = users.filter((id) => id !== userId);
    reaction.count = reaction.users.length;

    // Check if it's the current user
    const user = currentUser();
    if (user && userId === user.id) {
      reaction.me = false;
    }

    if (reaction.count === 0) {
      // Remove the reaction entirely
      reactions.splice(reactionIndex, 1);
    } else {
      reactions[reactionIndex] = reaction;
    }

    // Update the message in the store
    setMessagesState(
      "byChannel",
      channelId,
      messageIndex,
      "reactions",
      reactions.length > 0 ? reactions : undefined,
    );
  }
}

// State sync event handler

async function handlePatchEvent(
  entityType: string,
  entityId: string,
  diff: Record<string, unknown>,
): Promise<void> {
  console.log(`[WebSocket] Patch event: ${entityType}/${entityId}`, diff);

  switch (entityType) {
    case "user":
      {
        const { patchUser } = await import("@/stores/presence");
        patchUser(entityId, diff);
      }
      break;

    case "guild":
      {
        const { patchGuild } = await import("@/stores/guilds");
        patchGuild(entityId, diff);
      }
      break;

    case "member":
      {
        const { patchMember } = await import("@/stores/members");
        patchMember(entityId, diff);
      }
      break;

    default:
      console.warn(`[WebSocket] Unknown patch entity type: ${entityType}`);
  }
}

// Admin report event handlers

async function handleAdminReportCreated(
  reportId: string,
  category: string,
  targetType: string,
): Promise<void> {
  const { handleReportCreatedEvent } = await import("@/stores/admin");
  handleReportCreatedEvent(reportId, category, targetType);
}

async function handleAdminReportResolved(reportId: string): Promise<void> {
  const { handleReportResolvedEvent } = await import("@/stores/admin");
  handleReportResolvedEvent(reportId);
}

// Export stores for reading
export { wsState, setWsState, typingState, setTypingState };
