/**
 * Voice commands (browser mode stubs — voice requires Tauri).
 */

import { isTauri } from "./common";
import { bumpVoiceEpoch, currentVoiceEpoch } from "@/lib/webrtc/voiceSession";

/**
 * Native join (DM-call voice). Runs under the current voice session epoch
 * unless one is given; native drops the join when the epoch goes stale.
 */
export async function joinVoice(channelId: string, epoch?: number): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_voice", { channelId, epoch: epoch ?? currentVoiceEpoch() });
  }
  console.warn("Voice chat requires the native app");
}

/** Leave native voice. Starts a new voice epoch first: pending joins stop. */
export async function leaveVoice(): Promise<void> {
  const epoch = bumpVoiceEpoch();
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_voice", { epoch });
  }
}

/**
 * Drop the native call without `voice_leave` (C31/C32): the server already
 * removed this peer, and a leave from here could remove another device's.
 */
export async function leaveVoiceSilently(): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("leave_voice", { silent: true, epoch: currentVoiceEpoch() });
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
