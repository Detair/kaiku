/**
 * Voice Store
 *
 * Manages voice channel state including connection, participants, and audio settings.
 */

import { createStore, produce } from "solid-js/store";
import { createVoiceAdapter, type VoiceError } from "@/lib/webrtc";
import type { ScreenShareInfo, ScreenShareQuality } from "@/lib/webrtc/types";
import type { VoiceParticipant } from "@/lib/types";
import { channelsState } from "@/stores/channels";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

// Helper to get error message from VoiceError
function getErrorMessage(error: VoiceError): string {
  switch (error.type) {
    case "permission_denied":
    case "device_not_found":
    case "device_in_use":
    case "ice_failed":
    case "unknown":
      return error.message;
    case "server_rejected":
      return `Server rejected: ${error.message} (${error.code})`;
    case "connection_failed":
      return `Connection failed: ${error.reason}`;
    case "timeout":
      return `Timeout during ${error.operation}`;
    case "already_connected":
      return `Already connected to channel ${error.channelId}`;
    case "not_connected":
      return "Not connected to voice channel";
  }
}

// Voice connection state
type VoiceState = "disconnected" | "connecting" | "connected";

interface VoiceStoreState {
  // Current state
  state: VoiceState;
  // Connected channel ID
  channelId: string | null;
  // Local user state
  muted: boolean;
  deafened: boolean;
  speaking: boolean;
  // Participants in the channel
  participants: Record<string, VoiceParticipant>;
  // Error
  error: VoiceError | null;

  // Screen sharing
  screenSharing: boolean;
  screenShareInfo: ScreenShareInfo | null;
  screenShares: ScreenShareInfo[]; // All active screen shares in channel
}

// Create the store
const [voiceState, setVoiceState] = createStore<VoiceStoreState>({
  state: "disconnected",
  channelId: null,
  muted: false,
  deafened: false,
  speaking: false,
  participants: {},
  error: null,
  screenSharing: false,
  screenShareInfo: null,
  screenShares: [],
});

// Event listeners
let unlisteners: UnlistenFn[] = [];

/**
 * Initialize voice event listeners.
 */
export async function initVoice(): Promise<void> {
  // Clean up existing listeners
  await cleanupVoice();

  // Tauri-specific event listeners
  if (!isTauri) {
    // Browser mode - WebSocket events are handled in websocket store
    return;
  }

  const { listen } = await import("@tauri-apps/api/event");

  // Voice user events
  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>("ws:voice_user_joined", (event) => {
      const { channel_id, user_id } = event.payload;
      if (channel_id === voiceState.channelId) {
        addParticipant(user_id);
      }
    })
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>("ws:voice_user_left", (event) => {
      const { channel_id, user_id } = event.payload;
      if (channel_id === voiceState.channelId) {
        removeParticipant(user_id);
      }
    })
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>("ws:voice_user_muted", (event) => {
      const { channel_id, user_id } = event.payload;
      if (channel_id === voiceState.channelId) {
        updateParticipant(user_id, { muted: true });
      }
    })
  );

  unlisteners.push(
    await listen<{ channel_id: string; user_id: string }>("ws:voice_user_unmuted", (event) => {
      const { channel_id, user_id } = event.payload;
      if (channel_id === voiceState.channelId) {
        updateParticipant(user_id, { muted: false });
      }
    })
  );

  unlisteners.push(
    await listen<{ channel_id: string; participants: VoiceParticipant[] }>(
      "ws:voice_room_state",
      (event) => {
        const { channel_id, participants } = event.payload;
        if (channel_id === voiceState.channelId) {
          setParticipants(participants);
        }
      }
    )
  );

  unlisteners.push(
    await listen<{ code: string; message: string }>("ws:voice_error", (event) => {
      console.error("Voice error:", event.payload);
      const error: VoiceError = {
        type: "server_rejected",
        code: event.payload.code,
        message: event.payload.message,
      };
      setVoiceState({ error });
    })
  );
}

/**
 * Cleanup voice listeners.
 */
export async function cleanupVoice(): Promise<void> {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners = [];
}

/**
 * Join a voice channel.
 */
export async function joinVoice(channelId: string): Promise<void> {
  // Leave current channel if connected
  if (voiceState.state !== "disconnected") {
    await leaveVoice();
  }

  setVoiceState({ state: "connecting", channelId, error: null });

  const adapter = await createVoiceAdapter();

  // Set up adapter event handlers
  adapter.setEventHandlers({
    onStateChange: (state) => {
      const stateMap = {
        disconnected: "disconnected" as const,
        requesting_media: "connecting" as const,
        connecting: "connecting" as const,
        connected: "connected" as const,
        reconnecting: "connecting" as const,
      };
      setVoiceState({ state: stateMap[state] });
    },
    onError: (error) => {
      setVoiceState({ error });
    },
    onLocalMuteChange: (muted) => {
      setVoiceState({ muted });
    },
    onSpeakingChange: (speaking) => {
      setVoiceState({ speaking });
    },
    onScreenShareTrack: (userId, track) => {
      console.log("[Voice] Screen share track received:", userId);
      // Import and call viewer store
      import("@/stores/screenShareViewer").then(({ startViewing }) => {
        startViewing(userId, track);
      });
    },
    onScreenShareTrackRemoved: (userId) => {
      console.log("[Voice] Screen share track removed:", userId);
      import("@/stores/screenShareViewer").then(({ viewerState, stopViewing }) => {
        // Only stop if we were viewing this user's share
        if (viewerState.viewingUserId === userId) {
          stopViewing();
        }
      });
    },
  });

  const result = await adapter.join(channelId);
  if (!result.ok) {
    setVoiceState({ state: "disconnected", channelId: null, error: result.error });
    throw new Error(getErrorMessage(result.error));
  }
}

/**
 * Leave the current voice channel.
 */
export async function leaveVoice(): Promise<void> {
  if (voiceState.state === "disconnected") return;

  const adapter = await createVoiceAdapter();
  const result = await adapter.leave();

  if (!result.ok) {
    console.error("Failed to leave voice:", result.error);
  }

  setVoiceState({
    state: "disconnected",
    channelId: null,
    participants: {},
    speaking: false,
    screenSharing: false,
    screenShareInfo: null,
    screenShares: [],
  });
}

/**
 * Toggle mute state.
 */
export async function toggleMute(): Promise<void> {
  const newMuted = !voiceState.muted;
  const adapter = await createVoiceAdapter();
  const result = await adapter.setMute(newMuted);

  if (!result.ok) {
    console.error("Failed to toggle mute:", result.error);
    setVoiceState({ error: result.error });
  }
}

/**
 * Toggle deafen state.
 */
export async function toggleDeafen(): Promise<void> {
  const newDeafened = !voiceState.deafened;
  const adapter = await createVoiceAdapter();
  const result = await adapter.setDeafen(newDeafened);

  if (!result.ok) {
    console.error("Failed to toggle deafen:", result.error);
    setVoiceState({ error: result.error });
  } else {
    setVoiceState({
      deafened: newDeafened,
      // Deafening also mutes
      muted: newDeafened ? true : voiceState.muted,
    });
  }
}

/**
 * Set mute state directly.
 */
export async function setMute(muted: boolean): Promise<void> {
  const adapter = await createVoiceAdapter();
  const result = await adapter.setMute(muted);

  if (!result.ok) {
    console.error("Failed to set mute:", result.error);
    setVoiceState({ error: result.error });
  }
}

/**
 * Set deafen state directly.
 */
export async function setDeafen(deafened: boolean): Promise<void> {
  const adapter = await createVoiceAdapter();
  const result = await adapter.setDeafen(deafened);

  if (!result.ok) {
    console.error("Failed to set deafen:", result.error);
    setVoiceState({ error: result.error });
  } else {
    setVoiceState({ deafened, muted: deafened ? true : voiceState.muted });
  }
}

/**
 * Set speaking state (temporary for testing until VAD is implemented).
 * @phase1 - Backend needs to implement Voice Activity Detection (VAD)
 */
export function setSpeaking(speaking: boolean): void {
  setVoiceState({ speaking });
}

/**
 * Start screen sharing.
 */
export async function startScreenShare(
  quality?: ScreenShareQuality
): Promise<{ ok: boolean; error?: string }> {
  if (voiceState.state !== "connected" || !voiceState.channelId) {
    return { ok: false, error: "Not connected to voice channel" };
  }

  if (voiceState.screenSharing) {
    return { ok: false, error: "Already sharing screen" };
  }

  const adapter = await createVoiceAdapter();

  const result = await adapter.startScreenShare({ quality });

  if (!result.ok) {
    console.error("Failed to start screen share:", result.error);
    return { ok: false, error: getErrorMessage(result.error) };
  }

  setVoiceState({ screenSharing: true });
  return { ok: true };
}

/**
 * Stop screen sharing.
 */
export async function stopScreenShare(): Promise<void> {
  if (!voiceState.screenSharing) return;

  const adapter = await createVoiceAdapter();
  const result = await adapter.stopScreenShare();

  if (!result.ok) {
    console.error("Failed to stop screen share:", result.error);
  }

  setVoiceState({
    screenSharing: false,
    screenShareInfo: null,
  });
}

// Participant management

function addParticipant(userId: string): void {
  setVoiceState(
    produce((state) => {
      state.participants[userId] = {
        user_id: userId,
        muted: false,
        speaking: false,
        screen_sharing: false,
      };
    })
  );
}

function removeParticipant(userId: string): void {
  setVoiceState(
    produce((state) => {
      delete state.participants[userId];
    })
  );
}

function updateParticipant(userId: string, update: Partial<VoiceParticipant>): void {
  setVoiceState(
    produce((state) => {
      if (state.participants[userId]) {
        Object.assign(state.participants[userId], update);
      }
    })
  );
}

function setParticipants(participants: VoiceParticipant[]): void {
  setVoiceState(
    produce((state) => {
      state.participants = {};
      for (const p of participants) {
        state.participants[p.user_id] = p;
      }
    })
  );
}

/**
 * Get list of participants.
 */
export function getParticipants(): VoiceParticipant[] {
  return Object.values(voiceState.participants);
}

/**
 * Check if connected to voice.
 */
export function isInVoice(): boolean {
  return voiceState.state === "connected";
}

/**
 * Check if connected to a specific channel.
 */
export function isInChannel(channelId: string): boolean {
  return voiceState.state === "connected" && voiceState.channelId === channelId;
}

/**
 * Get current voice channel information
 * Returns null if not connected
 *
 * @note This is a derived helper to decouple VoiceIsland from channelsState.
 * When Phase 3 arrives and channels become guild-scoped, only this function
 * needs updating instead of every component that displays channel info.
 */
export function getVoiceChannelInfo(): { id: string; name: string } | null {
  if (!voiceState.channelId) return null;

  const channel = channelsState.channels.find(
    (c: { id: string }) => c.id === voiceState.channelId
  );

  if (!channel) {
    return { id: voiceState.channelId, name: "Unknown Channel" };
  }

  return { id: channel.id, name: channel.name };
}

// Export the store for reading and writing
export { voiceState, setVoiceState };
