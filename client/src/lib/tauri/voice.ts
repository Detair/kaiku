/**
 * Voice commands (browser mode stubs — voice requires Tauri).
 */

import { isTauri } from "./common";
import {
  bumpVoiceEpoch,
  currentVoiceEpoch,
  isNativeJoinCancelled,
  isVoiceEpochStale,
  takeCallJoinEpoch,
} from "@/lib/webrtc/voiceSession";

/**
 * Native join (DM-call voice). Runs under the session epoch captured when
 * the call was started or accepted (`captureCallJoinEpoch`), or the one
 * given; read synchronously before any await. A leave since then makes it
 * stale and the join does nothing (it never reaches `join_voice`).
 */
export async function joinVoice(channelId: string, epoch?: number): Promise<void> {
  const sessionEpoch = epoch ?? takeCallJoinEpoch(channelId) ?? currentVoiceEpoch();
  if (!isTauri()) {
    console.warn("Voice chat requires the native app");
    return;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  if (isVoiceEpochStale(sessionEpoch)) return;
  const nativeEpoch = await invoke<number>("begin_voice_session");
  if (isVoiceEpochStale(sessionEpoch)) return;
  try {
    await invoke("join_voice", { channelId, epoch: nativeEpoch });
  } catch (err) {
    // Cancelled by the leave that made this session stale: not an error.
    if (isVoiceEpochStale(sessionEpoch) && isNativeJoinCancelled(err)) return;
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
