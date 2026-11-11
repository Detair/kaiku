/**
 * Voice commands (browser mode stubs — voice requires Tauri).
 */

import { isTauri } from "./common";

export async function joinVoice(channelId: string): Promise<void> {
  if (isTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke("join_voice", { channelId });
  }
  console.warn("Voice chat requires the native app");
}

export async function leaveVoice(): Promise<void> {
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
