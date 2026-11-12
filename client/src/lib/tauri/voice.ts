/**
 * Voice commands (browser mode stubs — voice requires Tauri).
 */

import { isTauri } from "./common";
import {
  bumpVoiceEpoch,
  isNativeJoinCancelled,
  isVoiceEpochStale,
} from "@/lib/webrtc/voiceSession";

/**
 * Native join (DM-call voice) under `token`: the voice session token the
 * caller got when it started or accepted the call (`startCall`/`joinCall`,
 * or a recovery attempt's epoch). It is never looked up here; a leave since
 * then makes it stale and the join does nothing (never reaches
 * `join_voice`). Calling without a token is a programming error.
 */
export async function joinVoice(channelId: string, token?: number): Promise<void> {
  if (token === undefined) {
    const message = "joinVoice needs the call's voice session token";
    if (import.meta.env.DEV) throw new Error(message);
    console.error(`[Voice] ${message}; join refused`);
    return;
  }
  if (!isTauri()) {
    console.warn("Voice chat requires the native app");
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  if (isVoiceEpochStale(token)) return;
  const nativeEpoch = await invoke<number>("begin_voice_session");
  if (isVoiceEpochStale(token)) return;
  try {
    await invoke("join_voice", { channelId, epoch: nativeEpoch });
  } catch (err) {
    // Cancelled by the leave that made this session stale: not an error.
    if (isVoiceEpochStale(token) && isNativeJoinCancelled(err)) return;
    throw err;
  }
}

/** Leave native voice. Starts a new voice epoch first: pending joins stop. */
export async function leaveVoice(): Promise<void> {
  bumpVoiceEpoch();
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_voice");
  }
}

/**
 * Drop the native call without `voice_leave` (C31/C32): the server already
 * removed this peer, and a leave from here could remove another device's.
 */
export async function leaveVoiceSilently(): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_voice", { silent: true });
  }
}

export async function setMute(muted: boolean): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_mute", { muted });
  }
}

export async function setDeafen(deafened: boolean): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("set_deafen", { deafened });
  }
}
