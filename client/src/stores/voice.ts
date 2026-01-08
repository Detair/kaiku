/**
 * Voice Store
 *
 * Manages voice channel state including connection, participants, and audio settings.
 */

import { createStore, produce } from "solid-js/store";
import * as tauri from "@/lib/tauri";
import type { VoiceParticipant } from "@/lib/types";

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

// Type for unlisten function
type UnlistenFn = () => void;

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
  // Error message
  error: string | null;
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
});

// Event listeners
let unlisteners: UnlistenFn[] = [];

/**
 * Initialize voice event listeners.
 */
export async function initVoice(): Promise<void> {
  // Clean up existing listeners
  await cleanupVoice();

  // Voice requires Tauri - in browser mode, voice is not supported
  if (!isTauri) {
    console.warn("Voice chat requires the native Tauri app");
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
      setVoiceState({ error: event.payload.message });
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
  if (voiceState.state !== "disconnected") {
    // Leave current channel first
    await leaveVoice();
  }

  setVoiceState({ state: "connecting", channelId, error: null });

  try {
    await tauri.joinVoice(channelId);
    setVoiceState({ state: "connected" });
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    setVoiceState({ state: "disconnected", channelId: null, error });
    throw err;
  }
}

/**
 * Leave the current voice channel.
 */
export async function leaveVoice(): Promise<void> {
  if (voiceState.state === "disconnected") return;

  try {
    await tauri.leaveVoice();
  } catch (err) {
    console.error("Failed to leave voice:", err);
  }

  setVoiceState({
    state: "disconnected",
    channelId: null,
    participants: {},
    speaking: false,
  });
}

/**
 * Toggle mute state.
 */
export async function toggleMute(): Promise<void> {
  const newMuted = !voiceState.muted;
  try {
    await tauri.setMute(newMuted);
    setVoiceState({ muted: newMuted });
  } catch (err) {
    console.error("Failed to toggle mute:", err);
  }
}

/**
 * Toggle deafen state.
 */
export async function toggleDeafen(): Promise<void> {
  const newDeafened = !voiceState.deafened;
  try {
    await tauri.setDeafen(newDeafened);
    setVoiceState({
      deafened: newDeafened,
      // Deafening also mutes
      muted: newDeafened ? true : voiceState.muted,
    });
  } catch (err) {
    console.error("Failed to toggle deafen:", err);
  }
}

/**
 * Set mute state directly.
 */
export async function setMute(muted: boolean): Promise<void> {
  try {
    await tauri.setMute(muted);
    setVoiceState({ muted });
  } catch (err) {
    console.error("Failed to set mute:", err);
  }
}

/**
 * Set deafen state directly.
 */
export async function setDeafen(deafened: boolean): Promise<void> {
  try {
    await tauri.setDeafen(deafened);
    setVoiceState({ deafened, muted: deafened ? true : voiceState.muted });
  } catch (err) {
    console.error("Failed to set deafen:", err);
  }
}

// Participant management

function addParticipant(userId: string): void {
  setVoiceState(
    produce((state) => {
      state.participants[userId] = {
        user_id: userId,
        muted: false,
        speaking: false,
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

// Export the store for reading
export { voiceState };
