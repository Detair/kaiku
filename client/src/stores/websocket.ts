/**
 * WebSocket Store
 *
 * Manages WebSocket connection and routes events to appropriate stores.
 */

import { createStore } from "solid-js/store";
import * as tauri from "@/lib/tauri";
import type { Message, ServerEvent, UserStatus } from "@/lib/types";
import { addMessage, removeMessage } from "./messages";
import {
  receiveIncomingCall,
  callConnected,
  callEndedExternally,
  participantJoined,
  participantLeft,
  type EndReason,
} from "./call";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

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

  if (isTauri) {
    // Tauri mode - use Tauri event system
    const { listen } = await import("@tauri-apps/api/event");

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
          const { message_id, content } = event.payload;
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

    // Presence events
    unlisteners.push(
      await listen<{ user_id: string; status: UserStatus }>("ws:presence_update", (event) => {
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

    // Call events
    unlisteners.push(
      await listen<{ channel_id: string; initiator: string; initiator_name: string }>(
        "ws:incoming_call",
        (event) => {
          receiveIncomingCall(
            event.payload.channel_id,
            event.payload.initiator,
            event.payload.initiator_name
          );
        }
      )
    );

    unlisteners.push(
      await listen<{ channel_id: string; reason: string; duration_secs: number | null }>(
        "ws:call_ended",
        (event) => {
          callEndedExternally(
            event.payload.channel_id,
            event.payload.reason as EndReason,
            event.payload.duration_secs ?? undefined
          );
        }
      )
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string; username: string }>(
        "ws:call_participant_joined",
        (event) => {
          participantJoined(event.payload.channel_id, event.payload.user_id);
          callConnected(event.payload.channel_id, [event.payload.user_id]);
        }
      )
    );

    unlisteners.push(
      await listen<{ channel_id: string; user_id: string }>(
        "ws:call_participant_left",
        (event) => {
          participantLeft(event.payload.channel_id, event.payload.user_id);
        }
      )
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
        unlisteners.push(() => ws.removeEventListener("message", messageHandler));
      } else {
        console.warn("[WebSocket] No WebSocket instance found");
      }
    };

    // Attach initially
    attachMessageHandler();

    // Re-attach on reconnection
    const reconnectHandler = () => {
      console.log("[WebSocket] Received ws-reconnected event, re-attaching message handler");
      // Use setTimeout to ensure WebSocket is fully ready
      setTimeout(() => {
        attachMessageHandler();
      }, 100);
    };
    window.addEventListener("ws-reconnected", reconnectHandler);
    unlisteners.push(() => window.removeEventListener("ws-reconnected", reconnectHandler));
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
      addMessage(event.message);
      break;

    case "message_edit":
      console.log("Message edited:", event.message_id, event.content);
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
      console.log("Presence update:", event.user_id, event.status);
      break;

    case "voice_offer":
      console.log("[WebSocket] Handling voice_offer for channel:", event.channel_id);
      await handleVoiceOffer(event.channel_id, event.sdp);
      break;

    case "voice_ice_candidate":
      // ICE candidates must be processed immediately for NAT traversal
      await handleVoiceIceCandidate(event.channel_id, event.candidate);
      break;

    case "voice_user_joined":
      await handleVoiceUserJoined(event.channel_id, event.user_id, event.username, event.display_name);
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
      await handleVoiceRoomState(event.channel_id, event.participants, event.screen_shares);
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

    case "voice_error":
      console.error("Voice error:", event.code, event.message);

      // Auto-retry for "Already in voice channel" error
      if (event.message === "Already in voice channel") {
        const { voiceState } = await import("@/stores/voice");
        const channelId = voiceState.channelId;

        if (channelId) {
          console.log("[WebSocket] Auto-retry: leaving and rejoining channel", channelId);

          // Send leave message
          await tauri.wsSend({
            type: "voice_leave",
            channel_id: channelId,
          });

          // Wait a bit for server to process leave
          await new Promise(resolve => setTimeout(resolve, 150));

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
      receiveIncomingCall(event.channel_id, event.initiator, event.initiator_name);
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
        event.duration_secs ?? undefined
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
      await handleVoiceUserStatsEvent(event as any);
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
    console.warn("[WebSocket] No WebSocket instance available for reinitialization");
    return;
  }

  // Remove old listener if it exists (prevent duplicates)
  const oldListeners = unlisteners.filter(ul => ul.toString().includes("message"));
  oldListeners.forEach(ul => ul());

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

async function handleVoiceIceCandidate(channelId: string, candidate: string): Promise<void> {
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
    console.log(`[WebSocket] ICE candidate processed in ${elapsed.toFixed(2)}ms`);

    if (!result.ok) {
      console.error("Failed to handle ICE candidate:", result.error);
    }
  } catch (err) {
    console.error("Error handling ICE candidate:", err);
  }
}

async function handleVoiceUserJoined(channelId: string, userId: string, username: string, displayName: string): Promise<void> {
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
      })
    );
  }
}

async function handleVoiceUserLeft(channelId: string, userId: string): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        delete state.participants[userId];
      })
    );
  }
}

async function handleVoiceUserMuted(channelId: string, userId: string): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        if (state.participants[userId]) {
          state.participants[userId].muted = true;
        }
      })
    );
  }
}

async function handleVoiceUserUnmuted(channelId: string, userId: string): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  if (voiceState.channelId === channelId) {
    setVoiceState(
      produce((state) => {
        if (state.participants[userId]) {
          state.participants[userId].muted = false;
        }
      })
    );
  }
}

async function handleVoiceRoomState(channelId: string, participants: any[], screenShares?: any[]): Promise<void> {
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
      })
    );
  }
}

// Screen share event handlers

async function handleScreenShareStarted(event: any): Promise<void> {
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
      })
    );
  }
}

async function handleScreenShareStopped(event: any): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Screen share stopped:", event.user_id, event.reason);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        // Remove from screen shares list
        state.screenShares = state.screenShares.filter(s => s.user_id !== event.user_id);

        // Update participant's screen_sharing flag
        if (state.participants[event.user_id]) {
          state.participants[event.user_id].screen_sharing = false;
        }

        // If it was us, clear local state
        if (state.screenShareInfo?.user_id === event.user_id) {
          state.screenSharing = false;
          state.screenShareInfo = null;
        }
      })
    );
  }
}

async function handleScreenShareQualityChanged(event: any): Promise<void> {
  const { voiceState, setVoiceState } = await import("@/stores/voice");
  const { produce } = await import("solid-js/store");

  console.log("[WebSocket] Screen share quality changed:", event.user_id, event.new_quality);

  if (voiceState.channelId === event.channel_id) {
    setVoiceState(
      produce((state) => {
        const share = state.screenShares.find(s => s.user_id === event.user_id);
        if (share) {
          share.quality = event.new_quality;
        }
      })
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

// Export stores for reading
export { wsState, typingState };
