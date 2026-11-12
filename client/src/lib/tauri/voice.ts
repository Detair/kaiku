/**
 * Voice commands (browser mode stubs — voice requires Tauri).
 *
 * Joining and leaving voice is not here: every voice session, channel or
 * DM call, is joined and ended by the voice owner (`stores/voiceOwner.ts`)
 * through the voice adapter, so nothing else can start or end one.
 */

import { isTauri } from "./common";

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
